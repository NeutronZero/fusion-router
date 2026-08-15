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
use crate::types::{ExecutionNode, ExecutionNodeKind, NodeExecContext, NodeExecutionResult, StrategyKind};

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

    /// All graph semantics are executed by fusion-runtime. This adapter only
    /// translates provider ABI values at the application boundary.
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
