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
use crate::types::{
    ExecutionNode, ExecutionNodeKind, NodeExecContext, NodeExecutionResult, StrategyKind,
};

pub struct DefaultExecutor {
    pub provider: Arc<dyn ChatProvider + Send + Sync>,
    pub strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>>,
    #[cfg(feature = "semantic-cache")]
    pub cache: Option<Arc<SemanticCache>>,
    pub tool_registry: Option<Arc<ToolRegistry>>,
    pub allow_auto_exec: bool,
    /// Operator-configured tool names a client may reach by declaring them
    /// (review H2). `None` means unrestricted; an empty list permits none.
    pub permitted_tools: Option<Vec<String>>,
    /// Model-aware pricing resolver forwarded to the runtime executor so
    /// recorded usage carries real cost (review H3).
    pub pricing: Option<fusion_scheduler::PricingResolver>,
}

impl DefaultExecutor {
    pub fn new(
        provider: Arc<dyn ChatProvider + Send + Sync>,
        strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>>,
    ) -> Self {
        Self {
            provider,
            strategies,
            #[cfg(feature = "semantic-cache")]
            cache: None,
            tool_registry: None,
            allow_auto_exec: false,
            permitted_tools: None,
            pricing: None,
        }
    }

    #[cfg(feature = "semantic-cache")]
    pub fn with_cache(mut self, cache: Arc<SemanticCache>) -> Self {
        self.cache = Some(cache);
        self
    }
    pub fn with_tool_registry(mut self, registry: Arc<ToolRegistry>) -> Self {
        self.tool_registry = Some(registry);
        self
    }
    pub fn with_allow_auto_exec(mut self, allow: bool) -> Self {
        self.allow_auto_exec = allow;
        self
    }

    /// Restricts client-declarable tools to the operator's permitted set.
    pub fn with_permitted_tools(mut self, permitted: Vec<String>) -> Self {
        self.permitted_tools = Some(permitted);
        self
    }

    /// Installs model-aware pricing on the delegated runtime executor.
    pub fn with_pricing(mut self, pricing: fusion_scheduler::PricingResolver) -> Self {
        self.pricing = Some(pricing);
        self
    }

    /// Clamps a node's `tool_allowlist` to the intersection of the
    /// registered tools and the operator-permitted set (review H2). Returns
    /// a cloned node only when something was actually removed. Client-
    /// submitted workflow config can name ANY registered tool otherwise;
    /// this is the single enforcement chokepoint on every execution path.
    fn sanitize_tool_allowlist(&self, node: &ExecutionNode) -> ExecutionNode {
        let Some(allowlist_value) = node.config.get("tool_allowlist") else {
            return node.clone();
        };
        let Some(names) = allowlist_value.as_array() else {
            return node.clone();
        };
        let filtered: Vec<serde_json::Value> = names
            .iter()
            .filter(|v| match v.as_str() {
                Some(name) => {
                    let registered = self
                        .tool_registry
                        .as_ref()
                        .map(|r| r.get(name).is_some())
                        .unwrap_or(false);
                    match &self.permitted_tools {
                        Some(permitted) => registered && permitted.contains(&name.to_string()),
                        None => registered,
                    }
                }
                None => false,
            })
            .cloned()
            .collect();
        if filtered.len() == names.len() {
            return node.clone();
        }
        let mut sanitized = node.clone();
        sanitized
            .config
            .insert("tool_allowlist".to_string(), serde_json::Value::Array(filtered));
        tracing::warn!(
            node_id = %node.id,
            "tool_allowlist narrowed to operator-permitted, registered tools"
        );
        sanitized
    }

    /// All graph semantics are executed by fusion-runtime. This adapter only
    /// translates provider ABI values at the application boundary.
    async fn execute_runtime(
        &self,
        node: &ExecutionNode,
        ctx: &NodeExecContext,
    ) -> NodeExecutionResult {
        #[cfg(feature = "semantic-cache")]
        let provider: Arc<dyn fusion_runtime::ChatProvider> =
            Arc::new(FusionChatProvider::new(self.provider.clone()).with_cache(self.cache.clone()));
        #[cfg(not(feature = "semantic-cache"))]
        let provider: Arc<dyn fusion_runtime::ChatProvider> =
            Arc::new(FusionChatProvider::new(self.provider.clone()));
        let mut exec = fusion_runtime::ProviderExecutor::new(provider)
            .with_allow_auto_exec(self.allow_auto_exec);
        if let Some(pricing) = &self.pricing {
            exec = exec.with_pricing(pricing.clone());
        }
        if let Some(ref registry) = self.tool_registry {
            let mut rt_registry = fusion_runtime::ToolRegistry::new();
            for name in registry.list() {
                if let Some(tool) = registry.get(name) {
                    rt_registry.register(Arc::new(ToolAdapter(tool.clone())));
                }
            }
            exec = exec.with_tools(rt_registry);
        }
        let result = exec.execute_node(node, ctx).await;
        if matches!(
            node.kind,
            ExecutionNodeKind::LLMGenerate
                | ExecutionNodeKind::LLMReview
                | ExecutionNodeKind::LLMJudge
        ) {
            let mut result = result;
            if let Some(content) = result
                .output
                .as_ref()
                .and_then(|v| v.get("content"))
                .and_then(|v| v.as_str())
            {
                if result
                    .output
                    .as_ref()
                    .and_then(|v| v.get("tool_calls"))
                    .is_some()
                {
                    let mut new_output = serde_json::json!({ "content": content.to_string() });
                    new_output["tool_calls"] = result
                        .output
                        .as_ref()
                        .unwrap()
                        .get("tool_calls")
                        .unwrap()
                        .clone();
                    result.output = Some(new_output);
                } else {
                    result.output = Some(serde_json::Value::String(content.to_string()));
                }
            }
            result
        } else {
            result
        }
    }
}

#[async_trait]
impl Executor for DefaultExecutor {
    async fn execute_node(
        &self,
        node: &ExecutionNode,
        ctx: &NodeExecContext,
    ) -> NodeExecutionResult {
        // Review H2 chokepoint: narrow any client-declared allowlist before
        // the node reaches fusion-runtime.
        let sanitized = self.sanitize_tool_allowlist(node);
        self.execute_runtime(&sanitized, ctx).await
    }
}

/// Adapter bridging `crate::tools::Tool` to `fusion_runtime::Tool`.
struct ToolAdapter(Arc<dyn crate::tools::Tool>);

#[async_trait]
impl fusion_runtime::Tool for ToolAdapter {
    fn name(&self) -> &str {
        self.0.name()
    }
    fn description(&self) -> &str {
        self.0.description()
    }
    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value, String> {
        self.0.execute(args).await
    }
}
