<#
.SYNOPSIS
Multi-model consensus review via FusionRouter — parallel architects + judge synthesis.

.DESCRIPTION
Sends the same review prompt to N architect models in parallel, then a judge model
synthesizes their responses into a consensus report. All calls route through the
FusionRouter's full pipeline (planning, compilation, execution, strategies).

.PARAMETER HostUrl
FusionRouter endpoint (default: http://localhost:8080/v1/chat/completions).

.PARAMETER ArchitectModels
Array of model IDs for the architect phase. Each gets the ReviewPrompt.
Default: @("zen/deepseek-v4-flash-free", "zen/ling-3.0-flash-free")

.PARAMETER JudgeModel
Model ID used for final synthesis (default: first architect model).

.PARAMETER ReviewPrompt
Prompt sent to each architect model. Has a built-in default for codebase
architecture review — override for other use cases.

.PARAMETER JudgeSystemPrompt
Template for the judge synthesis prompt. Uses {reviews} placeholder which is
replaced with the collected architect responses. Has a built-in default.

.PARAMETER OutputFile
Optional path to write the final consensus report.

.PARAMETER TimeoutSec
Per-request timeout in seconds (default: 300).

.EXAMPLE
# Default: architecture review with two zen models
.\consensus_review.ps1

.EXAMPLE
# Three models, custom prompt, save output
.\consensus_review.ps1 `
    -ArchitectModels @("zen/deepseek-v4-flash-free", "zen/ling-3.0-flash-free", "zen/gemma-4-26b-it:free") `
    -JudgeModel "zen/deepseek-v4-flash-free" `
    -OutputFile "review.md"

.EXAMPLE
# Different provider, different task
.\consensus_review.ps1 `
    -HostUrl "http://localhost:8080/v1/chat/completions" `
    -ArchitectModels @("openrouter/free", "openrouter/free") `
    -ReviewPrompt "Review the security posture of this Rust codebase..."
#>
param(
    [string]$HostUrl = "http://localhost:8080/v1/chat/completions",
    [string[]]$ArchitectModels = @("zen/deepseek-v4-flash-free", "zen/ling-3.0-flash-free"),
    [string]$JudgeModel = "",
    [string]$ReviewPrompt = "",
    [string]$JudgeSystemPrompt = "",
    [string]$OutputFile = "",
    [int]$TimeoutSec = 300
)

$tmpDir = "$env:TEMP"

# --- Default prompts ---

if (-not $ReviewPrompt) {
    $ReviewPrompt = @'
You are a senior software architect reviewing the FusionRouter codebase. Provide a structured architecture review covering:

1. **Strengths** — What architectural decisions are well-made?
2. **Risks & Concerns** — What patterns, couplings, or missing abstractions could become problems?
3. **Data Flow Gaps** — Any missing error paths, dropped context, or race conditions?
4. **Modularity Assessment** — How clean are the module boundaries?
5. **Recommendations** — Top 3-5 concrete improvements with rationale.

The codebase is a Rust LLM request router. Core traits: Planner, Compiler (with CompilerPass pipeline), Scheduler, Executor, Strategy, ChatProvider (with Model + Transport adapters), ContextAssembler, RequirementsExtractor, ResourceManager, EvidenceRepository.

Key files:
- src/server/handlers.rs — Axum handlers, 11-step process_request pipeline, SSE streaming
- src/planner/intent_planner.rs — ExecutionIntent x Complexity to WorkflowIR templates
- src/compiler/passes.rs — Validation, ModelResolution, ControlFlowValidation (3-color cycle detection), BudgetOptimisation
- src/scheduler/default.rs + work_queue.rs — DAG scheduler, buffer_unordered(16), exponential backoff retry (max 2)
- src/executor/mod.rs — Strategy resolution + LLM execution with tool call parsing
- src/providers/router.rs — Model-prefix routing, OnceCell lazy init, circuit breaker fallthrough
- src/transport/http.rs — reqwest HTTP transport with backoff, SSE via bytes_stream
- src/types/mod.rs — All domain types
- src/middleware/rate_limit.rs — Token-bucket per client

Data flow: HTTP POST -> middleware -> context assembly -> requirements -> evidence snapshot -> planning -> compilation (4 passes) -> model override -> resource reservation -> scheduling -> execution loop (strategy resolution -> provider routing -> transport I/O) -> telemetry -> response.

Strategies: Single, Consensus (N generate + 1 judge), Reflection (generate -> review -> gate), Chain (sequential), Debate (parallel debaters + judge), ReAct (loop -> generate -> loop-back).
'@
}

if (-not $JudgeModel) {
    $JudgeModel = $ArchitectModels[0]
}

# --- Helpers ---

function Invoke-Model {
    param([string]$Model, [string]$Content, [int]$TimeoutSec)
    $body = @{
        model = $Model
        messages = @(@{ role = "user"; content = $Content })
        stream = $false
    } | ConvertTo-Json -Depth 10
    $tmpFile = Join-Path $tmpDir "consensus_$([System.IO.Path]::GetRandomFileName()).json"
    Set-Content -Path $tmpFile -Value $body -Encoding ASCII
    try {
        $raw = curl.exe -s -X POST $HostUrl -H "Content-Type: application/json" `
            -d "@$tmpFile" --max-time $TimeoutSec 2>&1
        return $raw
    } catch {
        return "CURL_ERROR: $_"
    } finally {
        Remove-Item -LiteralPath $tmpFile -Force -ErrorAction SilentlyContinue
    }
}

function Parse-Response {
    param([string]$Raw)
    if (-not $Raw) { return "ERROR: empty response" }
    if ($Raw -match "^CURL_ERROR") { return $Raw }
    try {
        $p = $Raw | ConvertFrom-Json
        if ($p.choices -and $p.choices[0].message) {
            return $p.choices[0].message.content
        }
        return "ERROR: unexpected response shape: $($Raw.Substring(0, [Math]::Min(200, $Raw.Length)))"
    } catch {
        return "ERROR: parse failed: $($_.Exception.Message)"
    }
}

function Write-Step {
    param([string]$Label, [string]$Value, [string]$Color = "Yellow")
    Write-Host "`n$("-" * 60)" -ForegroundColor DarkGray
    Write-Host "  $Label" -ForegroundColor $Color
    Write-Host "$("-" * 60)" -ForegroundColor DarkGray
    Write-Host $Value
}

# --- Step 1: Parallel architects ---

Write-Host "`n=== Consensus Review: $($ArchitectModels.Count) architects + judge ===" -ForegroundColor Cyan
Write-Host "Architects:" -ForegroundColor DarkGray
for ($i = 0; $i -lt $ArchitectModels.Count; $i++) {
    Write-Host "  [$($i+1)] $($ArchitectModels[$i])" -ForegroundColor DarkGray
}
Write-Host "Judge: $JudgeModel" -ForegroundColor DarkGray

$results = @{}
$jobs = @()

for ($i = 0; $i -lt $ArchitectModels.Count; $i++) {
    $model = $ArchitectModels[$i]
    $idx = $i
    Write-Host "Launching architect [$($idx+1)] $model ..." -ForegroundColor DarkGray
    $job = Start-Job -ScriptBlock {
        param($u, $m, $p, $t)
        $body = @{ model = $m; messages = @(@{ role = "user"; content = $p }); stream = $false } | ConvertTo-Json -Depth 10
        $tmp = Join-Path $env:TEMP "consensus_$([System.IO.Path]::GetRandomFileName()).json"
        Set-Content -Path $tmp -Value $body -Encoding ASCII
        try {
            $r = curl.exe -s -X POST $u -H "Content-Type: application/json" -d "@$tmp" --max-time $t 2>&1
            return $r
        } catch { return "CURL_ERROR: $_" }
        finally { Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue }
    } -ArgumentList $HostUrl, $model, $ReviewPrompt, $TimeoutSec
    $jobs += @{ Job = $job; Index = $idx; Model = $model }
}

foreach ($j in $jobs) {
    $null = $j.Job | Wait-Job -Timeout ($TimeoutSec + 30) -ErrorAction SilentlyContinue
    $raw = Receive-Job -Job $j.Job | Out-String
    $content = Parse-Response $raw
    $results[$j.Index] = @{ Model = $j.Model; Content = $content }
    Write-Host "  Done [$($j.Index+1)] $($j.Model)" -ForegroundColor Green
    Remove-Job -Job $j.Job -Force -ErrorAction SilentlyContinue
}

# --- Print architect reviews ---

for ($i = 0; $i -lt $ArchitectModels.Count; $i++) {
    $r = $results[$i]
    Write-Step -Label "Architect [$($i+1)]: $($r.Model)" -Value $r.Content -Color "Yellow"
}

# --- Step 2: Judge synthesis ---

$reviewsBlock = @()
for ($i = 0; $i -lt $ArchitectModels.Count; $i++) {
    $r = $results[$i]
    $reviewsBlock += "**Architect [$($i+1)] ($($r.Model)) Review:**`n$($r.Content)"
}
$reviewsText = $reviewsBlock -join "`n`n---`n`n"

if (-not $JudgeSystemPrompt) {
    $JudgeSystemPrompt = @"
You are a synthesis judge. Several senior architects have independently reviewed a codebase. Synthesize their analyses into a single **Consensus Architecture Review**.

Include sections:
1. **Areas of Agreement** — Where reviews converge (highest-confidence findings)
2. **Notable Differences** — Where reviews disagree or emphasize different aspects
3. **Synthesized Recommendations** — Top 3-5 recommendations combining the best insights, with rationale
4. **Blind Spots** — Any significant issues all reviews missed (based on your own analysis)

---

$reviewsText
"@
}

Write-Host "`n=== Judge synthesis ($JudgeModel) ===" -ForegroundColor Cyan
$rawJudge = Invoke-Model -Model $JudgeModel -Content $JudgeSystemPrompt -TimeoutSec $TimeoutSec
$judgeReview = Parse-Response $rawJudge

Write-Step -Label "CONSENSUS SYNTHESIS" -Value $judgeReview -Color "Magenta"

# --- Optional file output ---

if ($OutputFile) {
    $header = "# Consensus Architecture Review`n"
    $header += "Date: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')`n"
    $header += "Architects: $($ArchitectModels -join ', ')`n"
    $header += "Judge: $JudgeModel`n"
    $header += "`n---`n`n"

    $architectSection = @()
    for ($i = 0; $i -lt $ArchitectModels.Count; $i++) {
        $r = $results[$i]
        $architectSection += "## Architect [$($i+1)]: $($r.Model)`n`n$($r.Content)`n"
    }
    $architectText = $architectSection -join "`n---`n`n"

    $fullReport = $header + $architectText + "`n---`n`n" + "## Consensus Synthesis`n`n$judgeReview"
    $fullReport | Out-File -FilePath $OutputFile -Encoding utf8
    Write-Host "Report saved to: $OutputFile" -ForegroundColor Green
}