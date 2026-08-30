# Agent-State Integration — FusionRouter 0.14.5

**Rule:** `FusionRouter executes; agent-state reasons about Σ.`

`crates/fusion-agent-state` is Σ-aware; `crates/fusion-runtime` stays Σ-blind. All wiring lives in `src/` (host binary).

## What shipped

- `crates/fusion-agent-state:0.14.5` — `SkillSpec(P)`, `ExecutionState(Σ)`, `Observation(O)`, `StatePatch(ΔΣ)`, `StateStore` (transactional), `merge_state` ⊕ (null-delete, deterministic), `build_model_input(P,Σ,O)`, `EventLog` (out-of-band), `BudgetTracker`, `benchmark` (T=10..200, 52.8× at T=200), `runner::AgentRunner` (500-step test), `persistence::SqliteStateStore` (restart recovery).
- `Cargo.toml` workspace members — single line added; no `fusion-runtime` dep from `fusion-agent-state` except `fusion-core`.
- Zero changes to `crates/fusion-runtime`, `crates/fusion-compiler`, `crates/fusion-scheduler`.

## Minimal host diff (to wire real provider/tools)

Create `src/agent/host.rs` (example skeletons):

```rust
// Llm adapter: bounded ChatRequest(P,Σ,O) → ChatProvider → AgentStep
use fusion_agent_state::{AgentLlm, ChatRequest, AgentStep, StatePatch, AgentAction};
use std::sync::Arc;
pub struct RouterLlm { provider: Arc<dyn crate::providers::ChatProvider> , model: String }

#[async_trait::async_trait]
impl AgentLlm for RouterLlm {
    async fn complete(&self, req: ChatRequest) -> Result<AgentStep,String> {
        // Map crate::ChatRequest → crate::types::ChatCompletionRequest
        let cc_req = crate::types::ChatCompletionRequest {
            model: self.model.clone(),
            messages: req.messages.into_iter().map(|m| crate::types::ChatMessage{role:m.role, content:m.content}).collect(),
            stream: false, temperature: Some(0.0), max_tokens: Some(1024),
            tools: None, files: None, execution: None, output: None, strategy: None,
        };
        let resp = self.provider.chat_completion(&cc_req).await.map_err(|e| e.to_string())?;
        let text = resp.choices.first().map(|c| c.message.content.clone()).unwrap_or_default();
        // Expect JSON block {"state_patch":{...},"action":"..."} + ephemeral reasoning before it
        let (reasoning, patch, action) = parse_skill_json(&text)?; // your parser
        Ok(AgentStep::new(reasoning, patch, action))
    }
}

// Tool adapter: Action → ToolRegistry (fail-closed) → Observation
pub struct RouterTools { registry: crate::tools::registry::ToolRegistry, allow_auto_exec: bool }
#[async_trait::async_trait]
impl fusion_agent_state::runner::AgentToolExecutor for RouterTools {
    async fn execute(&self, action: &AgentAction) -> Result<fusion_agent_state::Observation,String> {
        if !self.allow_auto_exec { return Ok(fusion_agent_state::Observation::new(serde_json::json!({"skipped":action.raw}))); }
        // Enforce allowlist + ApprovalGate exactly as `ProviderExecutor::roundtrip` does
        // Call your existing tool dispatch, return Observation
        todo!("delegate to src/tools/registry.rs + src/resource/guard.rs")
    }
}
```

Wire in `src/server/handlers/agent.rs`:

```rust
use fusion_agent_state::{SkillSpec, InMemoryStateStore, ExecutionState, Observation};
use fusion_agent_state::runner::{AgentRunner, RunnerConfig};
use fusion_agent_state::persistence::SqliteStateStore;

pub async fn run_agent_skill(
    skill: SkillSpec,
    initial: Observation,
    runner_config: RunnerConfig,
    // injected host adapters
) -> anyhow::Result<ExecutionState> {
    // Persistent: survives restart
    let mut store = SqliteStateStore::open(skill.clone(), "agent_state.db", ExecutionState::new(serde_json::json!({"counter":0}))?)?;
    // or ephemeral: InMemoryStateStore::new(skill.clone(), initial_state)?
    let mut runner = AgentRunner::new(skill, store, llm_adapter, tool_adapter, runner_config);
    let cancel = tokio_util::sync::CancellationToken::new(); // from PipelineContext
    runner.run_until(initial, |s,_| s.value.get("done")==Some(&serde_json::json!(true)), Some(cancel)).await
        .map_err(|e| anyhow::anyhow!(e))
}
```

## Hardening checklist (1:1 with your list)

1. **Real execution path** — use `RouterLlm` + `RouterTools` above; no mock in prod. Action execution goes through existing `ToolRegistry`/`ApprovalGate`.
2. **Real ChatProvider** — reuse `src/providers::ChatProvider` (OpenAI/OpenRouter/Ollama/Zen) via `ProviderRegistry`; temperature 0.0, top_p 1.0 for deterministic tests.
3. **Tools** — `ToolRegistry::get` + `tool_allowlist` check; deny = observation error, not panic (ADR-037).
4. **Retry/failure/recovery** — `StateStore::commit` atomic; invalid patch ⇒ no tool call, retry with next LLM turn. Use `pipeline.rs: ResourceReservationStep` quota for retry budget; log to `EventLog` for replay.
5. **500/1k/5k horizons** — `runner::tests::runner_linear_to_500_steps` already covers 500; increase `RunnerConfig.max_steps` to 5000 and stream `EventLog` to SQLite to bound memory.
6. **Cancellation/resource limits** — pass `PipelineContext.cancellation_token` and `BudgetEnvelope` into `runner.step(..., Some(token))`; runner checks `is_cancelled()` pre-LLM and pre-tool, and enforces `RunnerConfig.max_tokens` + host `BudgetEnvelope::record_and_check`.
7. **Restart recovery** — swap `InMemoryStateStore` → `SqliteStateStore::open(..., path, initial)`; existing row validated against current schema on reopen (fail closed).
8. **Σ-blind** — `crates/fusion-runtime` has no import of `fusion-agent-state`; `crates/fusion-agent-state` has no import of `fusion-runtime` (only `fusion-core` + `tokio-util`). Boundary is `AgentLlm`/`AgentToolExecutor` traits.

## What NOT to do

- Don't add `Σ` fields to `ExecutionGraph`/`WorkflowIR`/`ExecutionNode`.
- Don't make `ProviderExecutor::build_request` emit `Σ`.
- Don't turn `EventLog` into prompt history.
