//! Stage 1 host adapters — Σ-blind boundary.
//!
//! `fusion-agent-state` owns Σ; this module maps it to FusionRouter's
//! `ChatProvider` / `ToolRegistry` without introducing a reverse dependency.
//! Keep all `RouterLlm` / `RouterTools` code here in `src/agent/`.

use std::sync::Arc;

use fusion_agent_state::{
    AgentAction, AgentStep, ChatRequest as AgentChatRequest,
    Observation, StatePatch,
};

use crate::providers::ChatProvider as RouterChatProvider;
use crate::tools::{Tool, ToolRegistry};

// ---------------------------------------------------------------------------
// JSON parsing — the SKILL.state contract
// ---------------------------------------------------------------------------

/// Extracts `AgentStep` from provider text.
///
/// Contract (from `crates/fusion-agent-state` prompt):
/// ```json
/// { "state_patch": { ... }, "action": "..." }
/// ```
/// fenced as ```json ... ```. Text before the fence is ephemeral `R_t`.
/// Missing/malformed fence or keys → `Err` (no mutation, no tool execution).
pub fn parse_skill_response(text: &str) -> Result<AgentStep, String> {
    // Find first fenced ```json block
    let fence_start = text.find("```json").ok_or_else(|| "missing ```json fence for state_patch/action".to_string())?;
    let after_fence = &text[fence_start + "```json".len()..];
    let fence_end = after_fence.find("```").ok_or_else(|| "unterminated ```json fence".to_string())?;
    let json_str = after_fence[..fence_end].trim();
    let v: serde_json::Value = serde_json::from_str(json_str).map_err(|e| format!("invalid JSON in fence: {e}"))?;

    let patch_val = v.get("state_patch").ok_or_else(|| "missing required key 'state_patch'".to_string())?;
    if !patch_val.is_object() {
        return Err("state_patch must be a JSON object".to_string());
    }
    let action_val = v.get("action").ok_or_else(|| "missing required key 'action'".to_string())?;
    let action_str = action_val.as_str().ok_or_else(|| "action must be a string".to_string())?;

    let reasoning = {
        let r = text[..fence_start].trim();
        if r.is_empty() { None } else { Some(r.to_string()) }
    };

    let patch = StatePatch::new(patch_val.clone()).map_err(|e| e.to_string())?;
    let action = AgentAction::new(action_str.to_string());

    Ok(AgentStep::new(reasoning, patch, action))
}

// ---------------------------------------------------------------------------
// RouterLlm — AgentLlm over RouterChatProvider (OpenAI-compatible path)
// ---------------------------------------------------------------------------

pub struct RouterLlm {
    provider: Arc<dyn RouterChatProvider>,
    model: String,
    max_retries: u32,
}

impl RouterLlm {
    pub fn new(provider: Arc<dyn RouterChatProvider>, model: impl Into<String>) -> Self {
        Self { provider, model: model.into(), max_retries: 2 }
    }

    pub fn with_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    fn agent_to_router_request(&self, req: &AgentChatRequest) -> crate::types::ChatCompletionRequest {
        crate::types::ChatCompletionRequest {
            model: self.model.clone(),
            messages: req.messages.iter().map(|m| crate::types::ChatMessage { role: m.role.clone(), content: m.content.clone() }).collect(),
            stream: false,
            temperature: Some(0.0),
            max_tokens: Some(1024),
            tools: None,
            files: None,
            execution: None,
            output: None,
            strategy: None,
        }
    }
}

#[async_trait::async_trait]
impl fusion_agent_state::runner::AgentLlm for RouterLlm {
    async fn complete(&self, request: AgentChatRequest) -> Result<AgentStep, String> {
        // Cancellation is checked by AgentRunner before calling complete;
        // here we handle provider retry idempotently: only ONE AgentStep
        // is ever returned per logical step, even if provider retries fire.
        let router_req = self.agent_to_router_request(&request);
        let mut last_err = String::new();
        for attempt in 0..=self.max_retries {
            match self.provider.chat_completion(&router_req).await {
                Ok(resp) => {
                    let text = resp.choices.first().map(|c| c.message.content.clone()).unwrap_or_default();
                    // Record usage into telemetry if present (caller may also record via BudgetEnvelope)
                    // Parsing is the commit gate — malformed JSON never reaches StateStore
                    match parse_skill_response(&text) {
                        Ok(step) => return Ok(step),
                        Err(e) => return Err(format!("malformed model output: {e} | raw: {}", &text[..text.len().min(300)])),
                    }
                }
                Err(e) => {
                    last_err = e.to_string();
                    if attempt == self.max_retries {
                        break;
                    }
                    // brief backoff before retry (no state committed yet)
                    tokio::time::sleep(std::time::Duration::from_millis(50 * (attempt as u64 + 1))).await;
                }
            }
        }
        Err(format!("provider failed after {} retries: {last_err}", self.max_retries + 1))
    }
}

// ---------------------------------------------------------------------------
// RouterTools — AgentToolExecutor over ToolRegistry + ApprovalGate + ResourceManager
// Ordered: allowlist → ApprovalGate → ResourceManager → registry → execute
// Policy remains authoritative before execution; registry lookup does NOT
// bypass approval/budget and must not create side effects on deny.
// ---------------------------------------------------------------------------

/// Read-only allowlist for Stage 1. Only these tools may be executed even if
/// the model requests others. Stage 2 keeps this as the first gate.
pub const READ_ONLY_ALLOWLIST: &[&str] = &["file_read", "calculator", "search"];

pub struct RouterTools {
    registry: Arc<ToolRegistry>,
    allowlist: Vec<String>,
    /// Optional approval authority. When Some, denied → no execution (fail-closed).
    approval: Option<Arc<dyn ApprovalGate>>,
    /// Optional budget guard. When Some, try_reserve/can_afford deny → no execution
    /// and failed reservation does NOT consume budget.
    resource_manager: Option<Arc<dyn ResourceManager>>,
}

/// Minimal ApprovalGate trait — mirrors `fusion_runtime::ApprovalGate` but kept
/// local to avoid hard dep; adapter in `src/` can wrap the runtime gate.
#[async_trait::async_trait]
pub trait ApprovalGate: Send + Sync {
    async fn is_approved(&self, tool_name: &str) -> bool;
}

/// Minimal ResourceManager for Stage 2 — check-only gate, no side effect on deny.
#[async_trait::async_trait]
pub trait ResourceManager: Send + Sync {
    async fn can_afford(&self, tool_name: &str) -> bool;
    fn reserve_calls(&self) -> usize { 0 }
}

impl RouterTools {
    pub fn new(registry: Arc<ToolRegistry>) -> Self {
        Self { registry, allowlist: READ_ONLY_ALLOWLIST.iter().map(|s| s.to_string()).collect(), approval: None, resource_manager: None }
    }

    pub fn with_allowlist(mut self, allowlist: Vec<String>) -> Self {
        self.allowlist = allowlist;
        self
    }

    pub fn with_approval_gate(mut self, gate: Arc<dyn ApprovalGate>) -> Self {
        self.approval = Some(gate);
        self
    }

    pub fn with_resource_manager(mut self, rm: Arc<dyn ResourceManager>) -> Self {
        self.resource_manager = Some(rm);
        self
    }

    fn is_allowed(&self, name: &str) -> bool {
        self.allowlist.contains(&name.to_string())
    }
}

#[async_trait::async_trait]
impl fusion_agent_state::runner::AgentToolExecutor for RouterTools {
    async fn execute(&self, action: &AgentAction) -> Result<Observation, String> {
        let tool_name = action.tool.clone().unwrap_or_else(|| action.raw.clone());

        // 1. Allowlist — fail closed
        if !self.is_allowed(&tool_name) {
            return Ok(Observation::new(serde_json::json!({
                "tool": tool_name,
                "executed": false,
                "reason": format!("tool '{tool_name}' not in allowlist {:?}", self.allowlist),
                "action_raw": action.raw,
            })));
        }

        // 2. ApprovalGate — authoritative before any registry lookup side effect
        if let Some(gate) = &self.approval {
            if !gate.is_approved(&tool_name).await {
                return Ok(Observation::new(serde_json::json!({
                    "tool": tool_name,
                    "executed": false,
                    "reason": "approval denied",
                })));
            }
        }

        // 3. ResourceManager — check-only gate; failed check does NOT consume budget
        if let Some(rm) = &self.resource_manager {
            if !rm.can_afford(&tool_name).await {
                return Ok(Observation::new(serde_json::json!({
                    "tool": tool_name,
                    "executed": false,
                    "reason": "resource/budget unavailable",
                    "reserve_calls": rm.reserve_calls(),
                })));
            }
        }

        // 4. Registry lookup — only after policy gates
        let tool = match self.registry.get(&tool_name) {
            Some(t) => t,
            None => return Ok(Observation::new(serde_json::json!({
                "tool": tool_name,
                "executed": false,
                "reason": "tool not registered (fail closed)",
            }))),
        };
        let args = if action.arguments.is_null() { serde_json::json!({}) } else { action.arguments.clone() };
        let result = tool.execute(args).await.map_err(|e| format!("tool '{tool_name}' failed: {e}"))?;
        Ok(Observation::new(serde_json::json!({
            "tool": tool_name,
            "executed": true,
            "result": result,
        })))
    }
}

/// Helper to build a read-only registry for Stage 1 (no shell/http write tools).
pub fn stage1_registry(allowed_dir: Option<String>) -> Arc<ToolRegistry> {
    let mut reg = ToolRegistry::new();
    reg.register(Arc::new(crate::tools::builtin::CalculatorTool));
    reg.register(Arc::new(crate::tools::builtin::SearchTool));
    if let Some(dir) = allowed_dir {
        reg.register(Arc::new(crate::tools::builtin::FileReadTool::new(dir)));
    }
    Arc::new(reg)
}

// ---------------------------------------------------------------------------
// Usage accounting helper — bridges provider Usage → BudgetEnvelope
// ---------------------------------------------------------------------------

pub fn record_usage(
    envelope: Option<&crate::resource::BudgetEnvelope>,
    usage: Option<&crate::types::Usage>,
    model: &str,
) {
    if let (Some(env), Some(u)) = (envelope, usage) {
        // Map to NanoUSD via a minimal pricing stub; real pricing comes from provider metadata
        let cost = crate::types::NanoUSD::from_nanos(
            (u.prompt_tokens as u64).saturating_mul(500).saturating_add((u.completion_tokens as u64).saturating_mul(1500)),
        );
        let _ = env.record_and_check(cost, u.total_tokens as u64);
        let _ = model; // silence unused in stub pricing path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fusion_agent_state::ChatRequest;

    #[test]
    fn parse_valid_fence() {
        let text = r#"I will increment counter.
```json
{"state_patch": {"counter": 1}, "action": "file_read"}
```"#;
        let step = parse_skill_response(text).unwrap();
        assert_eq!(step.reasoning.as_deref(), Some("I will increment counter."));
        assert_eq!(step.patch.value, serde_json::json!({"counter": 1}));
        assert_eq!(step.action.raw, "file_read");
    }

    #[test]
    fn parse_missing_fence_fails() {
        assert!(parse_skill_response("no fence here").is_err());
    }

    #[test]
    fn parse_missing_keys_fails() {
        assert!(parse_skill_response("```json\n{\"state_patch\": {}}\n```").is_err());
        assert!(parse_skill_response("```json\n{\"action\": \"x\"}\n```").is_err());
        assert!(parse_skill_response("```json\n{\"state_patch\": \"not object\", \"action\": \"x\"}\n```").is_err());
    }

    #[test]
    fn parse_action_must_be_string() {
        assert!(parse_skill_response("```json\n{\"state_patch\": {}, \"action\": 123}\n```").is_err());
    }
}
