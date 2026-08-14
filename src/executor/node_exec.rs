use async_trait::async_trait;
use fusion_scheduler::Executor as _;
use std::collections::HashMap;
use std::sync::Arc;

#[cfg(feature = "semantic-cache")]
use crate::cache::SemanticCache;
use crate::executor::fusion_bridge::FusionChatProvider;
use crate::executor::Executor;
use crate::providers::ChatProvider;
use crate::strategies::Strategy;
use crate::tools::ToolRegistry;
use crate::types::{ChatCompletionRequest, ChatMessage, ExecutionNode, ExecutionNodeKind, ExecutionSubgraph, NodeExecContext, NodeExecutionResult, StrategyKind};

pub struct DefaultExecutor {
    pub provider: Arc<dyn ChatProvider + Send + Sync>,
    pub strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>>,
    #[cfg(feature = "semantic-cache")]
    pub cache: Option<Arc<SemanticCache>>,
    pub tool_registry: Option<Arc<ToolRegistry>>,
    pub allow_auto_exec: bool,
}

impl DefaultExecutor {
    pub fn new(provider: Arc<dyn ChatProvider + Send + Sync>, strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>>) -> Self {
        Self { provider, strategies, #[cfg(feature = "semantic-cache")] cache: None, tool_registry: None, allow_auto_exec: false }
    }

    #[cfg(feature = "semantic-cache")]
    pub fn with_cache(mut self, cache: Arc<SemanticCache>) -> Self { self.cache = Some(cache); self }
    pub fn with_tool_registry(mut self, registry: Arc<ToolRegistry>) -> Self { self.tool_registry = Some(registry); self }
    pub fn with_allow_auto_exec(mut self, allow: bool) -> Self { self.allow_auto_exec = allow; self }

    pub(crate) fn build_request(&self, node: &ExecutionNode) -> ChatCompletionRequest {
        let mut messages: Vec<ChatMessage> = node.config.get("messages")
            .and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
        if !messages.iter().any(|m| m.role == "system") {
            let prompt = match node.kind {
                ExecutionNodeKind::LLMJudge => Some("You are a judge evaluating the quality and correctness of responses."),
                _ if node.strategy == StrategyKind::Reflection => Some("You are a reflective reviewer. Improve the previous response."),
                _ => None,
            };
            if let Some(content) = prompt { messages.insert(0, ChatMessage { role: "system".into(), content: content.into() }); }
        }
        ChatCompletionRequest {
            model: node.model.clone(), messages, stream: false,
            temperature: node.config.get("temperature").and_then(|v| v.as_f64()).map(|v| v as f32),
            max_tokens: node.config.get("max_tokens").and_then(|v| v.as_u64()).map(|v| v as u32),
            tools: self.request_tool_definitions(node), files: None, execution: None, output: None, strategy: None,
        }
    }

    #[cfg(feature = "semantic-cache")]
    pub(crate) fn cache_key(request: &ChatCompletionRequest) -> String {
        format!("{}:{}", request.model, serde_json::to_string(&request.messages).unwrap_or_default())
    }

    /// All graph semantics are executed by fusion-runtime. This adapter only
    /// translates provider ABI values at the application boundary.
    pub(crate) fn delegate_to_crates(&self, _node: &ExecutionNode) -> bool { true }

    async fn execute_runtime(&self, node: &ExecutionNode, ctx: &NodeExecContext) -> NodeExecutionResult {
        #[cfg(feature = "semantic-cache")]
        let provider: Arc<dyn fusion_runtime::ChatProvider> = Arc::new(FusionChatProvider::new(self.provider.clone()).with_cache(self.cache.clone()));
        #[cfg(not(feature = "semantic-cache"))]
        let provider: Arc<dyn fusion_runtime::ChatProvider> = Arc::new(FusionChatProvider::new(self.provider.clone()));
        let result = fusion_runtime::ProviderExecutor::new(provider).execute_node(node, ctx).await;
        if matches!(node.kind, ExecutionNodeKind::LLMGenerate | ExecutionNodeKind::LLMReview | ExecutionNodeKind::LLMJudge) {
            let mut result = result;
            if let Some(content) = result.output.as_ref().and_then(|v| v.get("content")).and_then(|v| v.as_str()) {
                result.output = Some(serde_json::Value::String(content.to_string()));
            }
            result
        } else { result }
    }
}

#[async_trait]
impl Executor for DefaultExecutor {
    async fn execute_node(&self, node: &ExecutionNode, ctx: &NodeExecContext) -> NodeExecutionResult {
        self.execute_runtime(node, ctx).await
    }
}

pub(crate) fn propagate_parent_messages(node: &ExecutionNode, subgraph: &mut ExecutionSubgraph) {
    let Some(messages) = node.config.get("messages").filter(|v| v.as_array().map(|a| !a.is_empty()).unwrap_or(false)).cloned() else { return; };
    for child in &mut subgraph.nodes {
        if matches!(child.kind, ExecutionNodeKind::LLMGenerate | ExecutionNodeKind::LLMReview | ExecutionNodeKind::LLMJudge)
            && child.config.get("messages").and_then(|v| v.as_array()).map(|a| a.is_empty()).unwrap_or(true) {
            child.config.insert("messages".into(), messages.clone());
        }
    }
}
