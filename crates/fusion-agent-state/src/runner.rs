//! Production runner: `fusion-agent-state` above `fusion-runtime`.
//!
//! Invariant: FusionRouter executes; agent-state reasons about Σ.
//! This module only depends on *traits* — concrete `ChatProvider` / `ToolRegistry`
//! adapters live in `src/` (host binary), keeping `crates/` Σ-blind.

use crate::{
    apply_transition, build_model_input, AgentAction, AgentStep, EventLog, EventLogEntry,
    ExecutionState, Observation, SkillSpec, StatePatch, StateStore,
};
use serde_json::json;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Host-supplied traits (Σ-blind boundary)
// ---------------------------------------------------------------------------

/// Host LLM adapter. Takes the bounded `ChatRequest(P,Σ,O)` and returns a
/// validated `AgentStep`. The host is responsible for prompt→model dispatch
/// (via `fusion-runtime` `ChatProvider` or `src/providers::ChatProvider`)
/// and JSON extraction (`{"state_patch":…, "action":…}`).
#[async_trait::async_trait]
pub trait AgentLlm: Send + Sync {
    async fn complete(&self, request: crate::ChatRequest) -> Result<AgentStep, String>;
}

/// Host tool/action executor. Receives `AgentAction` *after* `StateStore::commit`
/// and returns the next `Observation`. Host enforces `ToolRegistry` allowlist,
/// `ApprovalGate`, and `ResourceManager` — this crate never bypasses them.
#[async_trait::async_trait]
pub trait AgentToolExecutor: Send + Sync {
    async fn execute(&self, action: &AgentAction) -> Result<Observation, String>;
}

// ---------------------------------------------------------------------------
// Runner config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RunnerConfig {
    pub max_steps: u64,
    pub model: String,
    /// Optional per-run token/cost ceiling (enforced via host `BudgetEnvelope`);
    /// when `None`, only `fusion-runtime` global quota applies.
    pub max_tokens: Option<u64>,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            max_steps: 200,
            model: "mock-model".into(),
            max_tokens: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StepOutcome {
    pub t: u64,
    pub committed_state: ExecutionState,
    pub observation: Observation,
    pub total_tokens: u64,
}

// ---------------------------------------------------------------------------
// Core loop — generic over StateStore
// ---------------------------------------------------------------------------

/// Long-horizon loop. Owns `SkillSpec`, `StateStore`, `EventLog`, and the
/// host adapters. `R_t` is ephemeral (retained only in `EventLog`).
pub struct AgentRunner<S: StateStore> {
    skill: SkillSpec,
    store: S,
    log: EventLog,
    llm: Arc<dyn AgentLlm>,
    tools: Arc<dyn AgentToolExecutor>,
    config: RunnerConfig,
    total_tokens: u64,
}

impl<S: StateStore> AgentRunner<S> {
    pub fn new(
        skill: SkillSpec,
        store: S,
        llm: Arc<dyn AgentLlm>,
        tools: Arc<dyn AgentToolExecutor>,
        config: RunnerConfig,
    ) -> Self {
        Self {
            skill,
            store,
            log: EventLog::new(),
            llm,
            tools,
            config,
            total_tokens: 0,
        }
    }

    pub fn store(&self) -> &S {
        &self.store
    }
    pub fn store_mut(&mut self) -> &mut S {
        &mut self.store
    }
    pub fn log(&self) -> &EventLog {
        &self.log
    }
    pub fn skill(&self) -> &SkillSpec {
        &self.skill
    }

    /// Single validated transition:
    /// `O_t → BuildModelInput → LLM → validate ΔΣ → commit Σ' → execute a_t → O_{t+1}`
    /// Returns `Err` on patch validation failure (no state mutation, no tool execution).
    pub async fn step(
        &mut self,
        observation: Observation,
        cancellation: Option<&tokio_util::sync::CancellationToken>,
    ) -> Result<StepOutcome, String> {
        if let Some(tok) = cancellation {
            if tok.is_cancelled() {
                return Err("cancelled".into());
            }
        }

        let t = self.log.len() as u64 + 1;
        if t > self.config.max_steps {
            return Err(format!("max_steps {} exceeded", self.config.max_steps));
        }

        // 1. Bounded model input (P,Σ,O only)
        let req = build_model_input(&self.skill, &self.store.load(), &observation, &self.config.model);
        let input_tokens = crate::benchmark::tokens_for_request(&req);

        // 2. LLM proposes R_t + ΔΣ + a_t
        let step = self.llm.complete(req).await?;

        // 3. Cancellation before commit — fail closed, no mutation on cancel
        if let Some(tok) = cancellation {
            if tok.is_cancelled() {
                return Err("cancelled before commit".into());
            }
        }

        // 4. Transactional commit (validates ΔΣ → Σ' before any side effect)
        let prev = self.store.load();
        let committed = apply_transition(&mut self.store, &step).map_err(|e| e.to_string())?;

        // 5. Cancellation before tool — no tool side effect, but Σ' remains committed
        // State transition precedes side effect (SKILL.state semantic). Caller may retry
        // tool execution from committed Σ' if needed.
        if let Some(tok) = cancellation {
            if tok.is_cancelled() {
                // Log the committed transition even when tool was cancelled to keep store/log consistent
                self.log.push(EventLogEntry {
                    step: t,
                    prev_state: prev.value.clone(),
                    patch: step.patch.value.clone(),
                    next_state: committed.next_state.value.clone(),
                    action: committed.action.raw.clone(),
                    observation: Observation::new(json!({"cancelled": true, "before_tool": true})).value,
                    reasoning: committed.reasoning.clone(),
                });
                return Err("cancelled before tool execution".into());
            }
        }
        // Pre-tool budget check (fail closed before side effect). Uses predicted output if usage not yet known.
        if let Some(max) = self.config.max_tokens {
            let predicted = self.total_tokens.saturating_add(input_tokens).saturating_add(12);
            if predicted > max {
                return Err(format!("token budget {} would exceed: predicted {} > max", max, predicted));
            }
        }

        let next_obs = self.tools.execute(&committed.action).await?;

        // Real usage when host provided it, else heuristic 12
        let output_tokens = committed.usage.as_ref().map(|u| u.completion_tokens as u64).unwrap_or(12);
        let total_step_tokens = input_tokens.saturating_add(output_tokens);
        self.total_tokens = self.total_tokens.saturating_add(total_step_tokens);

        if let Some(max) = self.config.max_tokens {
            if self.total_tokens > max {
                return Err(format!("token budget {} exceeded: {}", max, self.total_tokens));
            }
        }

        self.log.push(EventLogEntry {
            step: t,
            prev_state: prev.value,
            patch: step.patch.value.clone(),
            next_state: committed.next_state.value.clone(),
            action: committed.action.raw.clone(),
            observation: next_obs.value.clone(),
            reasoning: committed.reasoning.clone(),
        });

        Ok(StepOutcome {
            t,
            committed_state: committed.next_state,
            observation: next_obs,
            total_tokens: self.total_tokens,
        })
    }

    /// Run until `is_done(state, observation)` or `max_steps`/cancellation.
    pub async fn run_until<F>(
        &mut self,
        mut observation: Observation,
        is_done: F,
        cancellation: Option<tokio_util::sync::CancellationToken>,
    ) -> Result<ExecutionState, String>
    where
        F: Fn(&ExecutionState, &Observation) -> bool,
    {
        for _ in 0..self.config.max_steps {
            if let Some(tok) = &cancellation {
                if tok.is_cancelled() {
                    return Err("cancelled".into());
                }
            }
            let state = self.store.load();
            if is_done(&state, &observation) {
                return Ok(state);
            }
            let outcome = self.step(observation, cancellation.as_ref()).await?;
            observation = outcome.observation;
        }
        Err("run_until: max_steps reached without done".into())
    }
}

// ---------------------------------------------------------------------------
// Host adapters (to be wired in `src/`) — minimal reference impls for tests
// ---------------------------------------------------------------------------

/// Deterministic mock LLM: increments `counter` each step, emits valid patch+action.
pub struct MockLlm;

#[async_trait::async_trait]
impl AgentLlm for MockLlm {
    async fn complete(&self, request: crate::ChatRequest) -> Result<AgentStep, String> {
        // Extract current counter from the state message (second message)
        let state_msg = request.messages.get(1).map(|m| m.content.as_str()).unwrap_or("{}");
        let cur: u64 = extract_counter(state_msg).unwrap_or(0);
        let next = cur + 1;
        Ok(AgentStep::new(
            Some(format!("mock reasoning for {next}")),
            StatePatch::new(json!({"counter": next})).map_err(|e| e.to_string())?,
            AgentAction::new(format!("act {next}")),
        ))
    }
}

fn extract_counter(state_msg: &str) -> Option<u64> {
    // state_msg is "Skill Execution State:\n```json\n{...}\n```"
    let start = state_msg.find('{')?;
    let end = state_msg.rfind('}')?;
    let json_str = &state_msg[start..=end];
    let v: serde_json::Value = serde_json::from_str(json_str).ok()?;
    v.get("counter")?.as_u64()
}

/// No-op tool executor: returns observation echoing the action.
pub struct NoopToolExecutor;

#[async_trait::async_trait]
impl AgentToolExecutor for NoopToolExecutor {
    async fn execute(&self, action: &AgentAction) -> Result<Observation, String> {
        Ok(Observation::new(json!({"echo": action.raw})))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InMemoryStateStore, SkillSpec};
    use serde_json::json;

    fn skill() -> SkillSpec {
        SkillSpec::new(json!({}), "test skill", "1")
    }
    fn initial() -> ExecutionState {
        ExecutionState::new(json!({"counter": 0})).unwrap()
    }

    #[tokio::test]
    async fn runner_linear_to_500_steps() {
        let store = InMemoryStateStore::new(skill(), initial()).unwrap();
        let mut runner = AgentRunner::new(
            skill(),
            store,
            Arc::new(MockLlm),
            Arc::new(NoopToolExecutor),
            RunnerConfig { max_steps: 500, model: "mock".into(), max_tokens: None },
        );
        let mut obs = Observation::new(json!({"tick": 0}));
        for _ in 0..500 {
            let out = runner.step(obs, None).await.unwrap();
            obs = out.observation;
        }
        assert_eq!(runner.store().load().value.get("counter").and_then(|v| v.as_u64()), Some(500));
        assert_eq!(runner.log().len(), 500);
        // State stays bounded, log grew linearly but never entered prompt
        let state_len = serde_json::to_string(&runner.store().load().value).unwrap().len();
        assert!(state_len < 100);
    }

    #[tokio::test]
    async fn runner_cancellation_before_tool() {
        let store = InMemoryStateStore::new(skill(), initial()).unwrap();
        let mut runner = AgentRunner::new(skill(), store, Arc::new(MockLlm), Arc::new(NoopToolExecutor), RunnerConfig::default());
        let tok = tokio_util::sync::CancellationToken::new();
        tok.cancel();
        let err = runner.step(Observation::new(json!({})), Some(&tok)).await.unwrap_err();
        assert!(err.contains("cancelled"));
        // No mutation on pre-LLM cancellation path
        assert_eq!(runner.store().load().value.get("counter").and_then(|v| v.as_u64()), Some(0));
    }

    #[tokio::test]
    async fn runner_invalid_patch_no_tool_execution() {
        struct BadLlm;
        #[async_trait::async_trait]
        impl AgentLlm for BadLlm {
            async fn complete(&self, _: crate::ChatRequest) -> Result<AgentStep, String> {
                Ok(AgentStep::new(
                    None,
                    // Patch tries to merge object onto scalar `counter`
                    StatePatch::new(json!({"counter": {"bad": 1}})).unwrap(),
                    AgentAction::new("should_not_run"),
                ))
            }
        }
        struct PanicTools;
        #[async_trait::async_trait]
        impl AgentToolExecutor for PanicTools {
            async fn execute(&self, _: &AgentAction) -> Result<Observation, String> {
                panic!("tool must not be called on invalid patch")
            }
        }
        let store = InMemoryStateStore::new(skill(), initial()).unwrap();
        let mut runner = AgentRunner::new(skill(), store, Arc::new(BadLlm), Arc::new(PanicTools), RunnerConfig::default());
        let err = runner.step(Observation::new(json!({})), None).await.unwrap_err();
        assert!(err.contains("ambiguous") || err.contains("MergeError") || err.contains("merge"));
        assert_eq!(runner.store().load().value.get("counter").and_then(|v| v.as_u64()), Some(0));
        assert_eq!(runner.log().len(), 0);
    }

    #[tokio::test]
    async fn runner_token_budget_enforced() {
        let store = InMemoryStateStore::new(skill(), initial()).unwrap();
        let mut runner = AgentRunner::new(
            skill(),
            store,
            Arc::new(MockLlm),
            Arc::new(NoopToolExecutor),
            RunnerConfig { max_steps: 1000, model: "mock".into(), max_tokens: Some(100) },
        );
        let mut obs = Observation::new(json!({}));
        let mut hit_budget = false;
        for _ in 0..20 {
            match runner.step(obs, None).await {
                Ok(o) => obs = o.observation,
                Err(e) if e.contains("budget") => { hit_budget = true; break; }
                Err(e) => panic!("unexpected err {e}"),
            }
        }
        assert!(hit_budget, "should hit token budget");
    }
}
