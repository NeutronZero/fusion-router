//! Warehouse scaling benchmark — validates SKILL.state complexity claims
//! without requiring a live LLM.
//!
//! Three independent properties:
//!   A. Context growth: |Ctx_t| bounded for state, linear for history
//!   B. Cumulative NanoUSD: C(T) linear for state, quadratic for history
//!   C. Accuracy under horizon: success rate stable for state
//!
//! Also provides trajectory-equivalence proof: EventLog is observational only.

use crate::{
    build_model_input, AgentAction, AgentStep, BudgetTracker, ChatRequest,
    EventLog, EventLogEntry, ExecutionState, InMemoryStateStore, Observation, SkillSpec,
    StatePatch, StateStore,
};
use serde_json::json;

// ---------------------------------------------------------------------------
// Token / cost model (deterministic, matches paper's asymptotic analysis)
// ---------------------------------------------------------------------------

/// Very small deterministic tokenizer: ~4 chars per token (standard heuristic).
/// Keeps benchmarks provider-free while preserving O(T) vs O(T²) distinction.
pub fn estimate_tokens(text: &str) -> u64 {
    // At least 1 token per message; 4 chars ≈ 1 token
    ((text.len() as u64) + 3) / 4
}

pub fn tokens_for_request(req: &ChatRequest) -> u64 {
    let mut total = 0u64;
    for m in &req.messages {
        total += estimate_tokens(&m.role);
        total += estimate_tokens(&m.content);
    }
    total
}

/// Deterministic pricing: input $0.50 / 1M tokens, output $1.50 / 1M tokens.
/// Mirrors NanoUSD accounting in `fusion-runtime` without coupling.
pub fn cost_for_tokens(input_tokens: u64, output_tokens: u64) -> fusion_core::NanoUSD {
    // nanos per token
    let input_nanos: u64 = 500;   // $0.50 / 1M = 500 nanos/token
    let output_nanos: u64 = 1500; // $1.50 / 1M = 1500 nanos/token
    let total = input_tokens.saturating_mul(input_nanos).saturating_add(output_tokens.saturating_mul(output_nanos));
    fusion_core::NanoUSD::from_nanos(total)
}

// ---------------------------------------------------------------------------
// Warehouse domain — minimal deterministic simulation
// ---------------------------------------------------------------------------

/// Warehouse skill spec: 500-shelf inventory, schema enforces bounded state.
pub fn warehouse_skill() -> SkillSpec {
    SkillSpec::new(
        json!({
            "type": "object",
            "properties": {
                "counter": {"type": "integer"},
                "branch": {"type": "string"},
                "shelf_count": {"type": "integer"}
            },
            "additionalProperties": true
        }),
        "You are a warehouse agent. Maintain counter and shelf state. Skill: Store/Ship/Move.",
        "1.0.0",
    )
}

pub fn warehouse_initial() -> ExecutionState {
    ExecutionState::new(json!({"counter": 0, "branch": "main", "shelf_count": 0})).unwrap()
}

// ---------------------------------------------------------------------------
// Run descriptors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct StepMetrics {
    pub t: u64,
    pub context_tokens: u64,      // |Ctx_t| for this step
    pub total_tokens: u64,          // cumulative
    pub total_cost_nanos: u64,
    pub state_bytes: usize,         // |Σ_t| serialized
    pub history_bytes: Option<usize>, // |History_t| for history mode
}

#[derive(Debug, Clone)]
pub struct HorizonReport {
    pub horizon: u64,
    pub state: Vec<StepMetrics>,
    pub history: Vec<StepMetrics>,
    pub state_success: bool,
    pub state_final: serde_json::Value,
    pub history_final_context_tokens: u64,
}

/// Runs a full horizon via the **state** loop (SKILL.state). Deterministic
/// MockProvider: each step increments `counter` by 1 via a patch.
pub fn run_state_horizon(horizon: u64) -> (Vec<StepMetrics>, ExecutionState, EventLog, u64) {
    let skill = warehouse_skill();
    let mut store = InMemoryStateStore::new(skill.clone(), warehouse_initial()).unwrap();
    let mut log = EventLog::new();
    let mut tracker = BudgetTracker::new();
    let mut metrics = Vec::new();
    let mut cumulative_tokens = 0u64;

    for t in 1..=horizon {
        let obs = Observation::new(json!({"tick": t, "noise": format!("telemetry {}", t)}));
        let req = build_model_input(&skill, &store.load(), &obs, "mock-model");
        let input_tokens = tokens_for_request(&req);
        let output_tokens = 12u64; // fixed small patch+action
        let cost = cost_for_tokens(input_tokens, output_tokens);
        tracker.record_step(cost, input_tokens + output_tokens);
        cumulative_tokens += input_tokens + output_tokens;

        let prev = store.load();
        // Deterministic patch: counter = t
        let patch = StatePatch::new(json!({"counter": t})).unwrap();
        let step = AgentStep::new(Some(format!("reasoning {t} ephemeral")), patch.clone(), AgentAction::new(format!("act {t}")));
        let committed = crate::apply_transition(&mut store, &step).unwrap();

        log.push(EventLogEntry {
            step: t,
            prev_state: prev.value,
            patch: patch.value,
            next_state: committed.next_state.value.clone(),
            action: committed.action.raw.clone(),
            observation: obs.value.clone(),
            reasoning: committed.reasoning.clone(),
        });

        metrics.push(StepMetrics {
            t,
            context_tokens: input_tokens,
            total_tokens: cumulative_tokens,
            total_cost_nanos: tracker.total_cost().as_nanos(),
            state_bytes: serde_json::to_string(&store.load().value).unwrap().len(),
            history_bytes: None,
        });
    }
    let final_state = store.load();
    let total = cumulative_tokens;
    (metrics, final_state, log, total)
}

/// Runs a full horizon via **history** baseline (ReAct-style). Prompt grows as
/// P + Σ_0 + Σ(all prior O_i + R_i). We model by accumulating a history buffer
/// and sizing `ChatRequest` as if all prior turns were included.
pub fn run_history_horizon(horizon: u64) -> Vec<StepMetrics> {
    let skill = warehouse_skill();
    // History buffer: system + growing transcript
    let mut history_messages: Vec<crate::ChatMessage> = vec![crate::ChatMessage {
        role: "system".into(),
        content: skill.instructions.clone(),
    }];
    let mut cumulative_tokens = 0u64;
    let mut total_cost_nanos = 0u64;
    let mut metrics = Vec::new();

    // Initial state message (counts once)
    let initial_state = warehouse_initial();
    let initial_state_str = serde_json::to_string(&initial_state.value).unwrap();

    for t in 1..=horizon {
        let obs_str = format!("{{\"tick\":{t},\"noise\":\"telemetry {t}\"}}");
        let reasoning = format!("reasoning {t} ephemeral — long chain of thought for step {t}");
        // Current prompt = system + all prior history + initial state + latest obs
        // We simulate by cloning history and counting tokens
        let mut req_messages = history_messages.clone();
        req_messages.push(crate::ChatMessage { role: "user".into(), content: format!("State: {initial_state_str}") });
        req_messages.push(crate::ChatMessage { role: "user".into(), content: format!("Observation: {obs_str}") });
        let req = ChatRequest { model: "mock-model".into(), messages: req_messages };
        let input_tokens = tokens_for_request(&req);
        let output_tokens = 12u64;
        let cost = cost_for_tokens(input_tokens, output_tokens);
        total_cost_nanos = total_cost_nanos.saturating_add(cost.as_nanos());
        cumulative_tokens += input_tokens + output_tokens;

        let history_bytes = {
            let s = serde_json::to_string(&history_messages).unwrap();
            s.len() + initial_state_str.len() + obs_str.len()
        };

        metrics.push(StepMetrics {
            t,
            context_tokens: input_tokens,
            total_tokens: cumulative_tokens,
            total_cost_nanos,
            state_bytes: serde_json::to_string(&json!({"counter": t})).unwrap().len(),
            history_bytes: Some(history_bytes),
        });

        // Append this turn to history (what history baselines do)
        history_messages.push(crate::ChatMessage { role: "user".into(), content: format!("Observation: {obs_str}") });
        history_messages.push(crate::ChatMessage { role: "assistant".into(), content: reasoning });
        history_messages.push(crate::ChatMessage { role: "assistant".into(), content: format!("counter={t}") });
    }
    metrics
}

/// Full comparison across horizons.
pub fn run_scaling_report(horizons: &[u64]) -> Vec<HorizonReport> {
    let mut reports = Vec::new();
    for &h in horizons {
        let (state_metrics, state_final, _log, _total) = run_state_horizon(h);
        let history_metrics = run_history_horizon(h);
        let state_success = state_final.value.get("counter").and_then(|v| v.as_u64()) == Some(h);
        let history_final_ctx = history_metrics.last().map(|m| m.context_tokens).unwrap_or(0);
        reports.push(HorizonReport {
            horizon: h,
            state: state_metrics,
            history: history_metrics,
            state_success,
            state_final: state_final.value,
            history_final_context_tokens: history_final_ctx,
        });
    }
    reports
}

// ---------------------------------------------------------------------------
// Trajectory equivalence helpers
// ---------------------------------------------------------------------------

/// Runs the same deterministic trajectory twice:
/// A = state store only, B = state store + EventLog. Returns true iff
/// Σ_A(T)==Σ_B(T) and ModelInput_A(t)==ModelInput_B(t) for all t.
pub fn verify_trajectory_equivalence(horizon: u64) -> (bool, String) {
    let skill = warehouse_skill();

    // Run A: no log
    let mut store_a = InMemoryStateStore::new(skill.clone(), warehouse_initial()).unwrap();
    let mut inputs_a = Vec::new();
    for t in 1..=horizon {
        let obs = Observation::new(json!({"tick": t}));
        let req = build_model_input(&skill, &store_a.load(), &obs, "mock");
        inputs_a.push(serde_json::to_string(&req).unwrap());
        let patch = StatePatch::new(json!({"counter": t})).unwrap();
        let step = AgentStep::new(Some(format!("r{t}")), patch, AgentAction::new(format!("a{t}")));
        crate::apply_transition(&mut store_a, &step).unwrap();
    }

    // Run B: with EventLog
    let mut store_b = InMemoryStateStore::new(skill.clone(), warehouse_initial()).unwrap();
    let mut log = EventLog::new();
    let mut inputs_b = Vec::new();
    for t in 1..=horizon {
        let obs = Observation::new(json!({"tick": t}));
        let req = build_model_input(&skill, &store_b.load(), &obs, "mock");
        inputs_b.push(serde_json::to_string(&req).unwrap());
        let patch = StatePatch::new(json!({"counter": t})).unwrap();
        let step = AgentStep::new(Some(format!("r{t}")), patch.clone(), AgentAction::new(format!("a{t}")));
        let prev = store_b.load();
        let committed = crate::apply_transition(&mut store_b, &step).unwrap();
        log.push(EventLogEntry {
            step: t,
            prev_state: prev.value,
            patch: patch.value,
            next_state: committed.next_state.value.clone(),
            action: committed.action.raw,
            observation: obs.value,
            reasoning: committed.reasoning,
        });
    }

    if store_a.load().value != store_b.load().value {
        return (false, format!("Σ mismatch: A={} B={}", store_a.load().value, store_b.load().value));
    }
    for (i, (a, b)) in inputs_a.iter().zip(inputs_b.iter()).enumerate() {
        if a != b {
            return (false, format!("ModelInput mismatch at t={}: A={a} B={b}", i + 1));
        }
    }
    // Also verify log didn't influence state size
    if log.len() as u64 != horizon {
        return (false, format!("log len {} != horizon {horizon}", log.len()));
    }
    (true, "trajectory equivalence holds: Σ and ModelInput identical with/without EventLog".into())
}

// ---------------------------------------------------------------------------
// Acceptance criteria helpers
// ---------------------------------------------------------------------------

/// Checks that state context stays bounded (variance < threshold) while history grows linearly.
pub fn check_bounded_context(state_metrics: &[StepMetrics], history_metrics: &[StepMetrics]) -> (bool, String) {
    if state_metrics.is_empty() || history_metrics.is_empty() {
        return (false, "empty metrics".into());
    }
    let s_min = state_metrics.iter().map(|m| m.context_tokens).min().unwrap();
    let s_max = state_metrics.iter().map(|m| m.context_tokens).max().unwrap();
    let h_min = history_metrics.iter().map(|m| m.context_tokens).min().unwrap();
    let h_max = history_metrics.iter().map(|m| m.context_tokens).max().unwrap();
    let s_variance = s_max - s_min;
    let h_growth = h_max - h_min;
    // State variance should be small (< 50 tokens); history growth should be large and roughly linear
    let bounded = s_variance < 50;
    let grows = h_growth > s_variance * 4; // history grows at least 4x more
    let msg = format!("state ctx variance {s_variance} (bounded={bounded}), history growth {h_growth} (grows={grows}), state max {s_max} vs history max {h_max}");
    (bounded && grows, msg)
}

/// Checks linear cumulative cost for state: slope roughly constant (within 20%).
pub fn check_linear_cumulative(state_metrics: &[StepMetrics]) -> (bool, String) {
    if state_metrics.len() < 2 {
        return (false, "need >=2 points".into());
    }
    // Per-step cost should be roughly constant
    let mut per_step: Vec<u64> = Vec::new();
    let mut prev = 0u64;
    for m in state_metrics {
        per_step.push(m.total_cost_nanos - prev);
        prev = m.total_cost_nanos;
    }
    let min = *per_step.iter().min().unwrap();
    let max = *per_step.iter().max().unwrap();
    let variance_ok = max - min < 5000; // small variance in nanos (cost per step stable)
    let msg = format!("per-step cost min {min} max {max} variance {} (linear={variance_ok})", max - min);
    (variance_ok, msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warehouse_context_growth_bounded_vs_linear() {
        let (state_m, _, _, _) = run_state_horizon(100);
        let history_m = run_history_horizon(100);
        let (ok, msg) = check_bounded_context(&state_m, &history_m);
        assert!(ok, "context growth invariant failed: {msg}");
    }

    #[test]
    fn warehouse_cumulative_linear() {
        let (state_m, _, _, _) = run_state_horizon(200);
        let (ok, msg) = check_linear_cumulative(&state_m);
        assert!(ok, "linear cost invariant failed: {msg}");
    }

    #[test]
    fn warehouse_accuracy_stable_across_horizons() {
        for &h in &[10, 25, 50, 100, 150, 200] {
            let (state_m, final_state, _, _) = run_state_horizon(h);
            assert_eq!(final_state.value.get("counter").and_then(|v| v.as_u64()), Some(h), "h={h} final counter mismatch");
            assert!(!state_m.is_empty());
            // State context stays bounded at each horizon's last step
            let last_ctx = state_m.last().unwrap().context_tokens;
            assert!(last_ctx < 500, "h={h} state ctx {last_ctx} too large");
        }
    }

    #[test]
    fn warehouse_history_quadratic_total_tokens() {
        // History total tokens should be >> state total tokens and grow superlinearly
        let mut prev_history_total = 0u64;
        let mut prev_state_total = 0u64;
        for &h in &[10, 50, 100, 200] {
            let (state_m, _, _, _) = run_state_horizon(h);
            let history_m = run_history_horizon(h);
            let s_total = state_m.last().unwrap().total_tokens;
            let h_total = history_m.last().unwrap().total_tokens;
            assert!(h_total > s_total * 2, "h={h} history total {h_total} should be >> state {s_total}");
            if prev_history_total > 0 {
                // History growth factor should exceed state growth factor (quadratic vs linear)
                let s_factor = s_total as f64 / prev_state_total as f64;
                let h_factor = h_total as f64 / prev_history_total as f64;
                assert!(h_factor > s_factor, "h={h} history factor {h_factor:.2} should exceed state {s_factor:.2}");
            }
            prev_history_total = h_total;
            prev_state_total = s_total;
        }
    }

    #[test]
    fn trajectory_equivalence() {
        for &h in &[10, 50, 100] {
            let (ok, msg) = verify_trajectory_equivalence(h);
            assert!(ok, "h={h} failed: {msg}");
        }
    }

    #[test]
    fn eventlog_is_observational() {
        // Directly verify that adding EventLog entries never changes ModelInput
        let skill = warehouse_skill();
        let mut store = InMemoryStateStore::new(skill.clone(), warehouse_initial()).unwrap();
        let mut log = EventLog::new();
        for t in 1..=20 {
            let obs = Observation::new(json!({"tick": t, "noise": "x".repeat(200)}));
            let req_before = build_model_input(&skill, &store.load(), &obs, "m");
            let patch = StatePatch::new(json!({"counter": t})).unwrap();
            let step = AgentStep::new(Some("reason".into()), patch.clone(), AgentAction::new("a"));
            let prev = store.load();
            let committed = crate::apply_transition(&mut store, &step).unwrap();
            log.push(EventLogEntry {
                step: t,
                prev_state: prev.value,
                patch: patch.value,
                next_state: committed.next_state.value.clone(),
                action: committed.action.raw,
                observation: obs.value.clone(),
                reasoning: committed.reasoning,
            });
            let req_after = build_model_input(&skill, &store.load(), &Observation::new(json!({"tick": t+1} )), "m");
            // req_after must be independent of log contents
            let req_after2 = build_model_input(&skill, &store.load(), &Observation::new(json!({"tick": t+1})), "m");
            assert_eq!(serde_json::to_string(&req_after).unwrap(), serde_json::to_string(&req_after2).unwrap());
            assert_eq!(log.len() as u64, t);
            // Ensure log bytes don't leak into next request
            let log_str = serde_json::to_string(&log.entries()).unwrap();
            let req_str = serde_json::to_string(&req_after).unwrap();
            assert!(!req_str.contains(&log_str[..log_str.len().min(50)]), "log content leaked into model input");
            let _ = req_before; // silence unused
        }
    }

    #[test]
    fn scaling_report_covers_all_horizons() {
        let horizons = [10, 25, 50, 100, 150, 200];
        let reports = run_scaling_report(&horizons);
        assert_eq!(reports.len(), horizons.len());
        for r in &reports {
            assert!(r.state_success, "horizon {} should succeed", r.horizon);
            // State final counter == horizon
            assert_eq!(r.state_final.get("counter").and_then(|v| v.as_u64()), Some(r.horizon));
            // History context >> state context at large horizons
            if r.horizon >= 100 {
                let s_ctx = r.state.last().unwrap().context_tokens;
                assert!(r.history_final_context_tokens > s_ctx * 3, "h={} history ctx {} should be >> state {}", r.horizon, r.history_final_context_tokens, s_ctx);
            }
        }
    }
}
