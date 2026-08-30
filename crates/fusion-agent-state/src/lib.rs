//! # fusion-agent-state
//!
//! SKILL.state implementation above `fusion-runtime`.
//!
//! Invariant: FusionRouter remains unaware of `Σ` as an agent-state concept.
//! This crate owns `P` (SkillSpec), `Σ` (ExecutionState), `O` (Observation),
//! `ΔΣ` (StatePatch), validation, and the long-horizon loop. It communicates
//! with `fusion-runtime` only via `ChatRequest`-shaped boundaries; `R_t`
//! (ephemeral reasoning) is never forwarded.
//!
//! Transition protocol (authoritative):
//! ```text
//! O_t → BuildModelInput(P,Σ_t,O_t) → LLM → AgentStep{R_t,ΔΣ_t,a_t}
//!     → PatchValidator → Σ' = Σ_t ⊕ ΔΣ_t → StateStore.commit(Σ') → RuntimeEngine.execute(a_t) → O_{t+1}
//! ```
//!
//! Merge operator `⊕`:
//! - object + object → recursive merge
//! - scalar / array → replacement
//! - null → delete key
//! - missing → unchanged
//! - ambiguous (object patch onto scalar/array leaf) → error

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

pub mod benchmark;
pub mod persistence;
pub mod runner;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// Immutable skill specification P. Schema is a JSON Schema value; instructions
/// are the prompt preamble; version is for audit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillSpec {
    /// JSON Schema (draft-07 subset) describing the shape of Σ. Stored as
    /// `serde_json::Value` to avoid a heavy jsonschema dep; validated locally.
    pub schema: serde_json::Value,
    pub instructions: String,
    pub version: String,
}

impl SkillSpec {
    pub fn new(schema: serde_json::Value, instructions: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            schema,
            instructions: instructions.into(),
            version: version.into(),
        }
    }
}

/// Canonical execution state Σ. Always a JSON object at the top level.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionState {
    pub value: serde_json::Value,
}

impl ExecutionState {
    pub fn new(value: serde_json::Value) -> Result<Self, StateError> {
        if !value.is_object() {
            return Err(StateError::NotAnObject("ExecutionState must be a JSON object".into()));
        }
        Ok(Self { value })
    }

    pub fn empty() -> Self {
        Self {
            value: serde_json::Value::Object(Default::default()),
        }
    }
}

/// Latest environment observation O_t. Arbitrary JSON (object, string, etc.).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Observation {
    pub value: serde_json::Value,
}

impl Observation {
    pub fn new(value: serde_json::Value) -> Self {
        Self { value }
    }
}

/// Structured state patch ΔΣ_t. Top-level must be an object; `null` values
/// signal deletion. All other values are replacements or recursive merges.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatePatch {
    pub value: serde_json::Value,
}

impl StatePatch {
    pub fn new(value: serde_json::Value) -> Result<Self, StateError> {
        if !value.is_object() {
            return Err(StateError::NotAnObject("StatePatch must be a JSON object".into()));
        }
        Ok(Self { value })
    }
}

/// Agent action a_t. Reuses the runtime/tool representation shape rather than
/// inventing a second protocol: `raw` is the exact command string forwarded to
/// `RuntimeEngine`/`ToolRegistry`; structured args are optional.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentAction {
    /// Exact command or tool invocation the runtime will execute.
    pub raw: String,
    /// Optional structured arguments (e.g. tool name + JSON args).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default)]
    pub arguments: serde_json::Value,
}

impl AgentAction {
    pub fn new(raw: impl Into<String>) -> Self {
        Self {
            raw: raw.into(),
            tool: None,
            arguments: serde_json::Value::Null,
        }
    }

    pub fn with_tool(mut self, tool: impl Into<String>, arguments: serde_json::Value) -> Self {
        self.tool = Some(tool.into());
        self.arguments = arguments;
        self
    }
}

/// Single LLM transition proposal. `reasoning` is ephemeral and MUST NOT be
/// forwarded to the next model input.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentStep {
    /// Ephemeral Chain-of-Thought `R_t`. Discarded after validation.
    pub reasoning: Option<String>,
    pub patch: StatePatch,
    pub action: AgentAction,
}

impl AgentStep {
    pub fn new(reasoning: Option<String>, patch: StatePatch, action: AgentAction) -> Self {
        Self { reasoning, patch, action }
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error, Clone, PartialEq)]
pub enum StateError {
    #[error("expected JSON object: {0}")]
    NotAnObject(String),
    #[error("schema violation: {0}")]
    SchemaViolation(String),
    #[error("merge error: {0}")]
    MergeError(String),
    #[error("patch validation failed: {0}")]
    PatchValidation(String),
    #[error("invalid patch JSON: {0}")]
    InvalidPatch(String),
}

// ---------------------------------------------------------------------------
// Deterministic merge operator ⊕
// ---------------------------------------------------------------------------

/// Deterministic JSON merge with null-delete semantics.
///
/// Rules:
/// - `object + object` → recursive merge (keys sorted via BTreeMap for determinism)
/// - `scalar / array` patch → replacement of base
/// - `null` patch value → delete key from base
/// - missing key in patch → unchanged in base
/// - ambiguous: patch is object but base leaf is scalar/array → error
///
/// Returns the merged value without mutating inputs.
pub fn merge_state(base: &serde_json::Value, patch: &serde_json::Value) -> Result<serde_json::Value, StateError> {
    // Patch must be object at top level (validated earlier), but recursive calls
    // may receive non-object patches for leaf replacement.
    match (base, patch) {
        // Patch null → delete signal handled by caller for object keys; at top level this is an error
        (_, serde_json::Value::Null) => Err(StateError::MergeError("top-level null patch is not a valid StatePatch".into())),
        // Object + Object → recursive
        (serde_json::Value::Object(base_map), serde_json::Value::Object(patch_map)) => {
            let mut merged = base_map.clone();
            // Iterate patch keys in sorted order for determinism
            let mut sorted_keys: Vec<&String> = patch_map.keys().collect();
            sorted_keys.sort();
            for key in sorted_keys {
                let patch_val = &patch_map[key];
                if patch_val.is_null() {
                    // null → delete
                    merged.remove(key);
                } else if let Some(base_val) = base_map.get(key) {
                    // Both have key: recurse
                    match (base_val, patch_val) {
                        (serde_json::Value::Object(_), serde_json::Value::Object(_)) => {
                            let rec = merge_state(base_val, patch_val)?;
                            merged.insert(key.clone(), rec);
                        }
                        // Base is scalar/array but patch is object → ambiguous
                        (_, serde_json::Value::Object(_)) if !base_val.is_object() => {
                            return Err(StateError::MergeError(format!(
                                "ambiguous merge: patch key '{key}' is object but base is {}",
                                type_of(base_val)
                            )));
                        }
                        // Otherwise replacement (scalar→scalar, array→array, etc.)
                        _ => {
                            merged.insert(key.clone(), patch_val.clone());
                        }
                    }
                } else {
                    // New key, null already handled, so insert
                    // If patch_val is object pending recursion against missing base, just insert clone
                    merged.insert(key.clone(), patch_val.clone());
                }
            }
            // Ensure deterministic key order by rebuilding via BTreeMap
            let ordered: BTreeMap<String, serde_json::Value> = merged.into_iter().collect();
            Ok(serde_json::Value::Object(ordered.into_iter().collect()))
        }
        // Leaf replacement
        (_, _) => Ok(patch.clone()),
    }
}

fn type_of(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

// ---------------------------------------------------------------------------
// Schema validation (lightweight draft-07 subset)
// ---------------------------------------------------------------------------

/// Minimal JSON Schema validation for Σ and patch-produced Σ'.
///
/// Supports:
/// - `type: "object"` with `properties` and `required`
/// - `type: "string" | "number" | "integer" | "boolean" | "array" | "object"`
/// - `additionalProperties: false`
/// For richer validation, replace with `jsonschema` crate and keep this
/// function's error interface.
pub fn validate_against_schema(value: &serde_json::Value, schema: &serde_json::Value) -> Result<(), StateError> {
    // Empty schema → accept
    if schema.is_null() || (schema.is_object() && schema.as_object().unwrap().is_empty()) {
        return Ok(());
    }
    validate_value_against_schema(value, schema, "$")
}

fn validate_value_against_schema(value: &serde_json::Value, schema: &serde_json::Value, path: &str) -> Result<(), StateError> {
    let Some(schema_obj) = schema.as_object() else {
        return Ok(());
    };

    if let Some(type_val) = schema_obj.get("type").and_then(|v| v.as_str()) {
        match type_val {
            "object" => {
                if !value.is_object() {
                    return Err(StateError::SchemaViolation(format!("{path}: expected object, got {}", type_of(value))));
                }
                let props = schema_obj.get("properties").and_then(|v| v.as_object());
                let required = schema_obj
                    .get("required")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
                    .unwrap_or_default();
                let allow_additional = schema_obj
                    .get("additionalProperties")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);

                let obj = value.as_object().unwrap();
                for req in &required {
                    if !obj.contains_key(*req) {
                        return Err(StateError::SchemaViolation(format!("{path}: missing required property '{req}'")));
                    }
                }
                if let Some(props_map) = props {
                    for (k, v) in obj {
                        if let Some(prop_schema) = props_map.get(k) {
                            validate_value_against_schema(v, prop_schema, &format!("{path}.{k}"))?;
                        } else if !allow_additional {
                            return Err(StateError::SchemaViolation(format!("{path}: additional property '{k}' not allowed")));
                        }
                    }
                }
            }
            "string" => {
                if !value.is_string() {
                    return Err(StateError::SchemaViolation(format!("{path}: expected string, got {}", type_of(value))));
                }
            }
            "number" => {
                if !value.is_number() {
                    return Err(StateError::SchemaViolation(format!("{path}: expected number, got {}", type_of(value))));
                }
            }
            "integer" => {
                if !value.as_i64().is_none() && !value.as_u64().is_none() {
                    // serde_json numbers that are integers pass
                    if value.as_i64().is_none() && value.as_u64().is_none() {
                        return Err(StateError::SchemaViolation(format!("{path}: expected integer, got {}", type_of(value))));
                    }
                }
                // Also reject floats that are not integers
                if let Some(n) = value.as_f64() {
                    if n.fract() != 0.0 {
                        return Err(StateError::SchemaViolation(format!("{path}: expected integer, got float")));
                    }
                } else if !value.is_number() {
                    return Err(StateError::SchemaViolation(format!("{path}: expected integer, got {}", type_of(value))));
                }
            }
            "boolean" => {
                if !value.is_boolean() {
                    return Err(StateError::SchemaViolation(format!("{path}: expected boolean, got {}", type_of(value))));
                }
            }
            "array" => {
                if !value.is_array() {
                    return Err(StateError::SchemaViolation(format!("{path}: expected array, got {}", type_of(value))));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// StateStore trait + InMemory implementation (transactional)
// ---------------------------------------------------------------------------

/// Transactional state store. `commit()` is atomic: validation happens before
/// any mutation; a failing patch never partially mutates canonical state.
pub trait StateStore {
    fn load(&self) -> ExecutionState;
    fn validate_patch(&self, patch: &StatePatch) -> Result<(), StateError>;
    fn commit(&mut self, patch: &StatePatch) -> Result<ExecutionState, StateError>;
}

/// In-memory store with schema-validated, deterministic merges.
#[derive(Debug, Clone)]
pub struct InMemoryStateStore {
    skill: SkillSpec,
    state: ExecutionState,
}

impl InMemoryStateStore {
    pub fn new(skill: SkillSpec, initial: ExecutionState) -> Result<Self, StateError> {
        validate_against_schema(&initial.value, &skill.schema)?;
        Ok(Self { skill, state: initial })
    }

    pub fn skill(&self) -> &SkillSpec {
        &self.skill
    }
}

impl StateStore for InMemoryStateStore {
    fn load(&self) -> ExecutionState {
        self.state.clone()
    }

    fn validate_patch(&self, patch: &StatePatch) -> Result<(), StateError> {
        // 1. Patch is already known to be object (constructed via StatePatch::new)
        // 2. Merge in isolation, then validate result against schema
        let merged = merge_state(&self.state.value, &patch.value)?;
        validate_against_schema(&merged, &self.skill.schema)?;
        Ok(())
    }

    fn commit(&mut self, patch: &StatePatch) -> Result<ExecutionState, StateError> {
        // Transactional: compute Σ' on a copy, validate, then atomically replace.
        let merged = merge_state(&self.state.value, &patch.value)?;
        validate_against_schema(&merged, &self.skill.schema)?;
        let next = ExecutionState { value: merged };
        self.state = next.clone();
        Ok(next)
    }
}

// ---------------------------------------------------------------------------
// Model input builder (P, Σ, O only — R_t never forwarded)
// ---------------------------------------------------------------------------

/// Chat message shape for the `ChatRequest` boundary. This crate defines its
/// own type to avoid a hard dependency on `fusion-runtime`; it maps 1:1 to
/// `fusion_runtime::ChatMessage` or `fusion_types::ChatMessage`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
}

/// Builds the bounded LLM prompt `ChatRequest(P, Σ_t, O_t)`. The `reasoning`
/// from any prior step is intentionally absent.
///
/// Invariant: `messages` length is bounded by `|P|+|Σ|+|O|`, not by `T`.
pub fn build_model_input(
    skill: &SkillSpec,
    state: &ExecutionState,
    observation: &Observation,
    model: &str,
) -> ChatRequest {
    let state_json = serde_json::to_string(&state.value).unwrap_or_else(|_| "{}".into());
    let obs_json = match &observation.value {
        serde_json::Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_else(|_| "{}".into()),
    };

    let system_content = skill.instructions.clone();
    let state_msg = format!("Skill Execution State:\n```json\n{state_json}\n```");
    let obs_msg = format!("Latest Observation:\n{obs_json}");

    ChatRequest {
        model: model.to_string(),
        messages: vec![
            ChatMessage { role: "system".into(), content: system_content },
            ChatMessage { role: "user".into(), content: state_msg },
            ChatMessage { role: "user".into(), content: obs_msg },
        ],
    }
}

// ---------------------------------------------------------------------------
// Agent loop abstraction + EventLog (out-of-band, not conversational memory)
// ---------------------------------------------------------------------------

/// Out-of-band event log entry. Used for audit/debug/replay, NOT as LLM context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventLogEntry {
    pub step: u64,
    /// Previous state hash (for chaining/audit).
    pub prev_state: serde_json::Value,
    pub patch: serde_json::Value,
    pub next_state: serde_json::Value,
    pub action: String,
    pub observation: serde_json::Value,
    /// `R_t` may be retained here for telemetry, but is never injected into the next prompt.
    pub reasoning: Option<String>,
}

/// Simple in-memory event log. Bounded only by caller retention policy; the LLM
/// input window remains `O(1)` regardless of log length.
#[derive(Debug, Clone, Default)]
pub struct EventLog {
    entries: Vec<EventLogEntry>,
}

impl EventLog {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    pub fn push(&mut self, entry: EventLogEntry) {
        self.entries.push(entry);
    }

    pub fn entries(&self) -> &[EventLogEntry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Validated transition result, separating state-commit from action execution.
/// The caller must execute the action via `fusion-runtime` *after* commit.
#[derive(Debug, Clone)]
pub struct CommittedTransition {
    pub prev_state: ExecutionState,
    pub next_state: ExecutionState,
    pub action: AgentAction,
    /// Ephemeral reasoning retained only for `EventLog`, never for next prompt.
    pub reasoning: Option<String>,
}

/// Applies the SKILL.state transition protocol transactionally:
///
/// ```text
/// validate ΔΣ_t → Σ' = Σ_t ⊕ ΔΣ_t (deterministic) → StateStore.commit
/// ```
/// Returns `CommittedTransition` on success; state is unchanged on error.
/// Action execution is the caller's responsibility AFTER commit (to preserve
/// `action_executes_only_after_state_commit`).
pub fn apply_transition<S: StateStore>(
    store: &mut S,
    step: &AgentStep,
) -> Result<CommittedTransition, StateError> {
    let prev = store.load();
    let next = store.commit(&step.patch)?;
    Ok(CommittedTransition {
        prev_state: prev,
        next_state: next,
        action: step.action.clone(),
        reasoning: step.reasoning.clone(),
    })
}

// ---------------------------------------------------------------------------
// Budget helper (linear accumulation; input_tokens ≈ |P|+|Σ|+|O|)
// ---------------------------------------------------------------------------

/// Tracks per-step and cumulative cost using `NanoUSD`. The improvement from
/// SKILL.state is that `input_tokens_t` stays bounded, so `total_cost_T`
/// grows linearly via `Σ step_cost_t` rather than quadratically.
#[derive(Debug, Clone, Default)]
pub struct BudgetTracker {
    total_cost: fusion_core::NanoUSD,
    total_tokens: u64,
    total_steps: u64,
}

impl BudgetTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_step(&mut self, cost: fusion_core::NanoUSD, tokens: u64) {
        self.total_cost = fusion_core::NanoUSD::from_nanos(self.total_cost.as_nanos().saturating_add(cost.as_nanos()));
        self.total_tokens = self.total_tokens.saturating_add(tokens);
        self.total_steps += 1;
    }

    pub fn total_cost(&self) -> fusion_core::NanoUSD {
        self.total_cost
    }

    pub fn total_tokens(&self) -> u64 {
        self.total_tokens
    }

    pub fn total_steps(&self) -> u64 {
        self.total_steps
    }
}

// ---------------------------------------------------------------------------
// Tests — mandatory architectural regressions
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn simple_skill() -> SkillSpec {
        SkillSpec::new(
            json!({
                "type": "object",
                "properties": {
                    "branch": {"type": "string"},
                    "tests": {
                        "type": "object",
                        "properties": {
                            "passing": {"type": "integer"},
                            "failing": {"type": "integer"}
                        },
                        "additionalProperties": false
                    },
                    "notes": {"type": "string"}
                },
                "additionalProperties": true
            }),
            "You are a skilled agent. Follow the skill instructions.",
            "1.0.0",
        )
    }

    fn minimal_skill() -> SkillSpec {
        SkillSpec::new(json!({}), "instructions", "1.0.0")
    }

    // 1. patch_schema_rejection
    #[test]
    fn patch_schema_rejection() {
        let skill = simple_skill();
        let initial = ExecutionState::new(json!({"branch": "main", "tests": {"passing": 42, "failing": 3}})).unwrap();
        let mut store = InMemoryStateStore::new(skill, initial).unwrap();
        // Patch introduces wrong type for tests.failing (string instead of integer)
        let bad_patch = StatePatch::new(json!({"tests": {"failing": "two"}})).unwrap();
        let err = store.commit(&bad_patch).unwrap_err();
        assert!(matches!(err, StateError::SchemaViolation(_)), "expected schema violation, got {err:?}");
        // State unchanged
        assert_eq!(store.load().value, json!({"branch": "main", "tests": {"passing": 42, "failing": 3}}));
    }

    // 2. patch_is_atomic_on_failure
    #[test]
    fn patch_is_atomic_on_failure() {
        let skill = minimal_skill();
        let initial = ExecutionState::new(json!({"a": 1, "b": 2})).unwrap();
        let mut store = InMemoryStateStore::new(skill, initial).unwrap();
        // Ambiguous merge: base `a` is number but patch tries to merge object onto it
        let bad_patch = StatePatch::new(json!({"a": {"nested": 1}})).unwrap();
        let before = store.load().value.clone();
        let err = store.commit(&bad_patch).unwrap_err();
        assert!(matches!(err, StateError::MergeError(_)), "got {err:?}");
        assert_eq!(store.load().value, before, "store must be atomic on failure");
    }

    // 3. null_deletes_field
    #[test]
    fn null_deletes_field() {
        let base = json!({"branch": "main", "tests": {"passing": 42, "failing": 3}, "temporary_note": "x"});
        let patch = json!({"tests": {"failing": 2}, "temporary_note": null});
        let merged = merge_state(&base, &patch).unwrap();
        assert_eq!(merged, json!({"branch": "main", "tests": {"passing": 42, "failing": 2}}));
        assert!(!merged.as_object().unwrap().contains_key("temporary_note"));
    }

    // 4. merge_is_deterministic
    #[test]
    fn merge_is_deterministic() {
        let base = json!({"z": 1, "a": 2, "m": {"b": 1, "a": 2}});
        // Insert patch keys in different orders; result must be identical JSON string
        let patch_a = json!({"z": 10, "new": "x", "a": 20});
        let patch_b = {
            let mut m = serde_json::Map::new();
            m.insert("a".into(), json!(20));
            m.insert("new".into(), json!("x"));
            m.insert("z".into(), json!(10));
            serde_json::Value::Object(m)
        };
        let r1 = merge_state(&base, &patch_a).unwrap();
        let r2 = merge_state(&base, &patch_b).unwrap();
        assert_eq!(r1, r2);
        // Canonical JSON must be byte-identical irrespective of insertion order
        assert_eq!(
            serde_json::to_string(&r1).unwrap(),
            serde_json::to_string(&r2).unwrap()
        );
    }

    // 5. invalid_patch_never_reaches_runtime
    #[test]
    fn invalid_patch_never_reaches_runtime() {
        let skill = minimal_skill();
        let initial = ExecutionState::new(json!({"x": 1})).unwrap();
        let mut store = InMemoryStateStore::new(skill, initial).unwrap();
        // Patch is not an object → construction fails, never reaches commit/merge
        let err = StatePatch::new(json!("not an object")).unwrap_err();
        assert!(matches!(err, StateError::NotAnObject(_)));
        // Also: object patch onto scalar leaf is rejected at commit
        let bad = StatePatch::new(json!({"x": {"oops": 1}})).unwrap();
        assert!(store.commit(&bad).is_err());
        assert_eq!(store.load().value, json!({"x": 1}));
    }

    // 6. reasoning_is_not_forwarded
    #[test]
    fn reasoning_is_not_forwarded() {
        let skill = SkillSpec::new(json!({}), "Do the task", "1");
        let state = ExecutionState::new(json!({"a": 1})).unwrap();
        let obs = Observation::new(json!("hello"));
        let req = build_model_input(&skill, &state, &obs, "test-model");
        let serialized = serde_json::to_string(&req).unwrap();
        // Simulate a step with reasoning
        let step = AgentStep::new(
            Some("secret chain-of-thought that must not leak".into()),
            StatePatch::new(json!({"a": 2})).unwrap(),
            AgentAction::new("do thing"),
        );
        // Next input must not contain reasoning
        let next_state = ExecutionState::new(json!({"a": 2})).unwrap();
        let next_obs = Observation::new(json!("world"));
        let next_req = build_model_input(&skill, &next_state, &next_obs, "test-model");
        let next_serialized = serde_json::to_string(&next_req).unwrap();
        assert!(!next_serialized.contains("secret chain-of-thought"));
        assert!(!serialized.contains("secret chain-of-thought"));
        // Also ensure apply_transition retains reasoning only in CommittedTransition/EventLog, not in state
        let mut store = InMemoryStateStore::new(skill, state).unwrap();
        let committed = apply_transition(&mut store, &step).unwrap();
        assert_eq!(committed.reasoning, Some("secret chain-of-thought that must not leak".into()));
        assert!(!serde_json::to_string(&store.load().value).unwrap().contains("secret"));
    }

    // 7. state_is_the_only_persistent_agent_context
    #[test]
    fn state_is_the_only_persistent_agent_context() {
        let skill = SkillSpec::new(json!({}), "instr", "1");
        let s0 = ExecutionState::new(json!({"step": 0})).unwrap();
        let mut store = InMemoryStateStore::new(skill.clone(), s0).unwrap();
        // Run 50 steps, each producing a patch and observation; prompt size stays bounded
        let mut sizes = Vec::new();
        for i in 1..=50 {
            let state = store.load();
            let obs = Observation::new(json!({"tick": i, "noise": "irrelevant"}));
            let req = build_model_input(&skill, &state, &obs, "m");
            sizes.push(serde_json::to_string(&req).unwrap().len());
            let patch = StatePatch::new(json!({"step": i})).unwrap();
            let step = AgentStep::new(None, patch, AgentAction::new(format!("action {i}")));
            apply_transition(&mut store, &step).unwrap();
        }
        // All prompts should be roughly same size (no history accumulation)
        let min = *sizes.iter().min().unwrap();
        let max = *sizes.iter().max().unwrap();
        // Allow small variance from integer width (1 vs 2 digits) but not linear growth
        assert!(max - min < 50, "prompt size grew with history: min {min} max {max}");
        assert_eq!(store.load().value, json!({"step": 50}));
    }

    // 8. action_executes_only_after_state_commit
    #[test]
    fn action_executes_only_after_state_commit() {
        let skill = minimal_skill();
        let initial = ExecutionState::new(json!({"x": 1})).unwrap();
        let mut store = InMemoryStateStore::new(skill, initial).unwrap();
        let step = AgentStep::new(
            None,
            StatePatch::new(json!({"x": 2})).unwrap(),
            AgentAction::new("rm -rf /important"),
        );
        // Commit first
        let committed = apply_transition(&mut store, &step).unwrap();
        assert_eq!(store.load().value, json!({"x": 2}));
        // Action is available ONLY after commit; caller would now call RuntimeEngine.execute(committed.action)
        assert_eq!(committed.action.raw, "rm -rf /important");
        assert_eq!(committed.prev_state.value, json!({"x": 1}));
        assert_eq!(committed.next_state.value, json!({"x": 2}));

        // Failed patch must NOT yield an action to execute
        let bad_step = AgentStep::new(
            None,
            StatePatch::new(json!({"x": {"bad": 1}})).unwrap(),
            AgentAction::new("should never run"),
        );
        let before = store.load().value.clone();
        let err = apply_transition(&mut store, &bad_step);
        assert!(err.is_err());
        assert_eq!(store.load().value, before);
        // Caller must not execute bad_step.action if commit failed
    }

    // 9. runtime_policy_gates_remain_authoritative
    #[test]
    fn runtime_policy_gates_remain_authoritative() {
        // This crate never bypasses policy; it only produces AgentAction for the
        // runtime to gate. We verify that StateStore does NOT execute actions or
        // approve gates — it only commits state.
        let skill = minimal_skill();
        let initial = ExecutionState::new(json!({})).unwrap();
        let mut store = InMemoryStateStore::new(skill, initial).unwrap();
        let step = AgentStep::new(None, StatePatch::new(json!({"approved": true})).unwrap(), AgentAction::new("privileged"));
        let committed = apply_transition(&mut store, &step).unwrap();
        // State says approved, but runtime gate is still authoritative — this crate
        // has no ApprovalGate logic, so callers must still consult fusion-runtime
        assert_eq!(committed.next_state.value, json!({"approved": true}));
        // No in-crate shortcut exists to mark an ApprovalGate as passed.
    }

    // 10. NanoUSD_accumulates_linearly_across_steps
    #[test]
    fn nanousd_accumulates_linearly_across_steps() {
        let mut tracker = BudgetTracker::new();
        // Simulate 100 steps each costing ~ $0.0001 with bounded tokens
        for _ in 0..100 {
            tracker.record_step(fusion_core::NanoUSD::from_nanos(100_000), 50);
        }
        assert_eq!(tracker.total_steps(), 100);
        assert_eq!(tracker.total_tokens(), 5000);
        assert_eq!(tracker.total_cost().as_nanos(), 10_000_000); // 100 * 100k
        // Linear: double steps = double cost
        let mut tracker2 = BudgetTracker::new();
        for _ in 0..200 {
            tracker2.record_step(fusion_core::NanoUSD::from_nanos(100_000), 50);
        }
        assert_eq!(tracker2.total_cost().as_nanos(), tracker.total_cost().as_nanos() * 2);
        assert_eq!(tracker2.total_tokens(), tracker.total_tokens() * 2);
    }

    // 11. state_size_does_not_depend_on_history_length
    #[test]
    fn state_size_does_not_depend_on_history_length() {
        let skill = SkillSpec::new(json!({}), "instr", "1");
        let initial = ExecutionState::new(json!({"counter": 0, "branch": "main"})).unwrap();
        let mut store = InMemoryStateStore::new(skill.clone(), initial).unwrap();
        let mut event_log = EventLog::new();
        for i in 1..=200 {
            let prev = store.load();
            let patch = StatePatch::new(json!({"counter": i})).unwrap();
            let step = AgentStep::new(Some(format!("reasoning {i} not persisted")), patch, AgentAction::new("act"));
            let committed = apply_transition(&mut store, &step).unwrap();
            event_log.push(EventLogEntry {
                step: i,
                prev_state: prev.value,
                patch: committed.next_state.value.clone(),
                next_state: committed.next_state.value.clone(),
                action: committed.action.raw.clone(),
                observation: json!({"i": i}),
                reasoning: committed.reasoning.clone(),
            });
        }
        // State is tiny and constant size; event log grew to 200 but never entered prompt
        let state_str = serde_json::to_string(&store.load().value).unwrap();
        let state_len = state_str.len();
        assert!(state_len < 100, "state should remain bounded, got len {state_len}: {state_str}");
        assert_eq!(event_log.len(), 200);
        // Next prompt still bounded
        let req = build_model_input(&skill, &store.load(), &Observation::new(json!("final obs")), "m");
        assert!(serde_json::to_string(&req).unwrap().len() < 500);
    }

    // 12. observation_noise_does_not_persist_without_patch
    #[test]
    fn observation_noise_does_not_persist_without_patch() {
        let skill = minimal_skill();
        let initial = ExecutionState::new(json!({"important": "keep"})).unwrap();
        let mut store = InMemoryStateStore::new(skill.clone(), initial).unwrap();
        // Noisy observations that are NOT committed via patch must not appear in state
        for i in 0..20 {
            let noise = Observation::new(json!({"telemetry": format!("noise {i}"), "important": "keep"}));
            let req = build_model_input(&skill, &store.load(), &noise, "m");
            let s = serde_json::to_string(&req).unwrap();
            // Observation is in the prompt transiently, but store remains clean
            assert!(s.contains(&format!("noise {i}")));
            assert_eq!(store.load().value, json!({"important": "keep"}));
            // Agent chooses to NOT patch noise into state (empty patch)
            let patch = StatePatch::new(json!({})).unwrap();
            let step = AgentStep::new(None, patch, AgentAction::new("noop"));
            apply_transition(&mut store, &step).unwrap();
            assert_eq!(store.load().value, json!({"important": "keep"}));
        }
        // Explicitly relevant observation DOES persist when patched
        let patch = StatePatch::new(json!({"important": "updated", "new_field": "value"})).unwrap();
        let step = AgentStep::new(None, patch, AgentAction::new("act"));
        apply_transition(&mut store, &step).unwrap();
        assert_eq!(store.load().value, json!({"important": "updated", "new_field": "value"}));
    }

    // Additional: validates the example from the spec comment
    #[test]
    fn spec_example_merge() {
        let base = json!({"branch": "main", "tests": {"passing": 42, "failing": 3}});
        let patch = json!({"tests": {"failing": 2}, "temporary_note": null});
        // base has no temporary_note; patch null-delete is idempotent
        let merged = merge_state(&base, &patch).unwrap();
        assert_eq!(merged, json!({"branch": "main", "tests": {"passing": 42, "failing": 2}}));
        // Now with note present, deletion works
        let base2 = json!({"branch": "main", "tests": {"passing": 42, "failing": 3}, "temporary_note": "x"});
        let merged2 = merge_state(&base2, &patch).unwrap();
        assert_eq!(merged2, json!({"branch": "main", "tests": {"passing": 42, "failing": 2}}));
    }
}
