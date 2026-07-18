# setup-opencode.ps1 — Configure OpenCode to use FusionRouter as its provider

param(
    [string]$FusionUrl = "http://localhost:8080",
    [string]$ApiKey = $env:FUSION_ROUTER_API_KEY
)

Write-Host "Checking FusionRouter at $FusionUrl..."
try {
    $null = Invoke-WebRequest -Uri "$FusionUrl/health" -UseBasicParsing -TimeoutSec 5
    Write-Host "  ✓ FusionRouter is running"
}
catch {
    Write-Host "  ✗ FusionRouter not reachable at $FusionUrl"
    Write-Host "    Start it with: cargo run"
    Write-Host "    Then re-run this script."
    exit 1
}

$OpenCodeDir = "$env:USERPROFILE\.config\opencode"
$null = New-Item -ItemType Directory -Path $OpenCodeDir -Force

$ConfigFile = "$OpenCodeDir\project.json"
$Config = @{
    provider = @{
        baseURL = "$FusionUrl/v1"
        apiKey  = $ApiKey
    }
}

$Config | ConvertTo-Json | Set-Content -Path $ConfigFile -Encoding UTF8

Write-Host ""
Write-Host "  ✓ OpenCode configured to use FusionRouter at $FusionUrl"
Write-Host ""
Write-Host "Next steps:"
Write-Host "  1. Restart OpenCode to pick up the new config."
Write-Host "  2. Start chatting — FusionRouter handles model selection automatically."
Write-Host "  3. (Optional) Set FUSION_ROUTER_API_KEY env var for authenticated access."
