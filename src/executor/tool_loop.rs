use tracing::info;

use crate::executor::DefaultExecutor;
use crate::types::{ExecutionNode, ToolCall, ToolDefinition};

impl DefaultExecutor {
    /// Request-scoped tool allowlist from the node config. An absent or
    /// empty allowlist means NO tool may execute (fail closed).
    pub(crate) fn request_tool_allowlist(node: &ExecutionNode) -> Vec<String> {
        node.config
            .get("tool_allowlist")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Tool definitions are advertised to the provider ONLY when auto
    /// execution is enabled AND the request names an allowlist. Otherwise no
    /// definitions are sent, so the provider cannot emit tool calls at all.
    pub(crate) fn request_tool_definitions(
        &self,
        node: &ExecutionNode,
    ) -> Option<Vec<ToolDefinition>> {
        if !self.allow_auto_exec {
            return None;
        }
        let registry = self.tool_registry.as_ref()?;
        let allowlist = Self::request_tool_allowlist(node);
        if allowlist.is_empty() {
            return None;
        }
        let defs: Vec<ToolDefinition> = allowlist
            .iter()
            .filter_map(|name| {
                let tool = registry.get(name)?;
                Some(ToolDefinition {
                    name: tool.name().to_string(),
                    description: tool.description().to_string(),
                    parameters: Some(tool.schema()),
                })
            })
            .collect();
        if defs.is_empty() {
            None
        } else {
            Some(defs)
        }
    }

    /// Law 7 / ADR-037: executes provider-native tool calls under the
    /// per-request allowlist. Calls that are not allowlisted, or that arrive
    /// while auto-execution is disabled, are returned as text — never run.
    pub(crate) async fn execute_native_tool_calls(
        &self,
        node: &ExecutionNode,
        calls: &[ToolCall],
    ) -> serde_json::Value {
        let allowlist = Self::request_tool_allowlist(node);
        let mut results = Vec::with_capacity(calls.len());
        for call in calls {
            let allowlisted = self.allow_auto_exec
                && !allowlist.is_empty()
                && allowlist.iter().any(|t| t == &call.name);
            let registry_has = self
                .tool_registry
                .as_ref()
                .map(|r| r.contains(&call.name))
                .unwrap_or(false);

            if allowlisted && registry_has {
                let Some(registry) = self.tool_registry.as_ref() else {
                    results.push(serde_json::json!({
                        "id": call.id,
                        "tool": call.name,
                        "arguments": call.arguments,
                        "error": "tool registry unavailable",
                        "executed": false,
                    }));
                    continue;
                };
                let Some(tool) = registry.get(&call.name) else {
                    results.push(serde_json::json!({
                        "id": call.id,
                        "tool": call.name,
                        "arguments": call.arguments,
                        "error": "tool not registered",
                        "executed": false,
                    }));
                    continue;
                };
                match tool.execute(call.arguments.clone()).await {
                    Ok(result) => {
                        info!(tool = %call.name, "Tool executed successfully (native tool call)");
                        results.push(serde_json::json!({
                            "id": call.id,
                            "tool": call.name,
                            "arguments": call.arguments,
                            "result": result,
                            "executed": true,
                        }));
                    }
                    Err(e) => {
                        info!(tool = %call.name, error = %e, "Tool execution failed");
                        results.push(serde_json::json!({
                            "id": call.id,
                            "tool": call.name,
                            "arguments": call.arguments,
                            "error": e,
                            "executed": false,
                        }));
                    }
                }
            } else {
                info!(
                    tool = %call.name,
                    auto_exec = self.allow_auto_exec,
                    "Tool call not executed: outside per-request allowlist"
                );
                results.push(serde_json::json!({
                    "id": call.id,
                    "tool": call.name,
                    "arguments": call.arguments,
                    "executed": false,
                    "reason": "tool not allowed by per-request allowlist",
                }));
            }
        }
        serde_json::json!({ "tool_calls": results })
    }
}
