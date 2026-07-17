use std::sync::Arc;

use fusion_router::compiler::CompilerPass;
use fusion_router::plugin::{PluginManager, PluginManifest};
use fusion_router::providers::ChatProvider;
use fusion_router::strategies::Strategy;
use fusion_router::types::{ChatCompletionRequest, StrategyKind};

struct TestProvider;

#[async_trait::async_trait]
impl ChatProvider for TestProvider {
    fn name(&self) -> &str { "test-plugin" }

    async fn chat_completion(
        &self,
        _request: &ChatCompletionRequest,
    ) -> anyhow::Result<fusion_router::types::ChatCompletionResponse> {
        Ok(fusion_router::types::ChatCompletionResponse {
            id: "test".into(), object: "chat.completion".into(),
            created: 0, model: "test".into(), choices: vec![],
            usage: None,
        })
    }
}

#[test]
fn test_plugin_registry_register_provider() {
    let mut mgr = PluginManager::new();
    let provider: Arc<dyn ChatProvider + Send + Sync> = Arc::new(TestProvider);
    mgr.register_provider("test-provider", provider);

    let registry = mgr.registry();
    assert!(registry.providers.contains_key("test-provider"));
    assert_eq!(registry.providers["test-provider"].name(), "test-plugin");
}

#[test]
fn test_plugin_manifest_discover_nonexistent_dir() {
    let manifests = PluginManifest::discover("/nonexistent/plugins/");
    assert!(manifests.is_empty(), "Non-existent dir should yield empty manifests");
}

#[cfg(target_os = "linux")]
#[test]
fn test_plugin_manifest_load_valid() {
    let manifest_path = "plugins/example-provider/example-provider.toml";
    if std::path::Path::new(manifest_path).exists() {
        let manifest = PluginManifest::load(manifest_path).expect("should load manifest");
        assert_eq!(manifest.plugin.name, "example-provider");
        assert!(manifest.provider.is_some());
        assert_eq!(manifest.provider.unwrap().name, "example");
    }
}

struct TestStrategy;

impl Strategy for TestStrategy {
    fn apply(&self, node: &fusion_router::types::ExecutionNode) -> fusion_router::types::ExecutionSubgraph {
        fusion_router::types::ExecutionSubgraph {
            nodes: vec![node.clone()],
            edges: vec![],
            entry_node_id: node.id,
            exit_node_id: node.id,
        }
    }
}

#[test]
fn test_plugin_registry_register_strategy() {
    let mut mgr = PluginManager::new();
    mgr.register_strategy(
        StrategyKind::Chain,
        Box::new(TestStrategy),
    );

    let registry = mgr.registry();
    assert!(registry.strategies.contains_key(&StrategyKind::Chain));
}

struct TestPass;

#[async_trait::async_trait]
impl CompilerPass for TestPass {
    fn name(&self) -> &str { "test-pass" }

    async fn apply(&self, ir: fusion_router::types::WorkflowIR) -> Result<fusion_router::types::WorkflowIR, fusion_router::types::CompilerError> {
        Ok(ir)
    }
}

#[tokio::test]
async fn test_plugin_registry_register_pass() {
    let mut mgr = PluginManager::new();
    mgr.register_pass(Box::new(TestPass));

    let registry = mgr.registry();
    assert_eq!(registry.passes.len(), 1);
    assert_eq!(registry.passes[0].name(), "test-pass");
}

#[test]
fn test_plugin_manager_default_is_empty() {
    let mgr = PluginManager::new();
    assert!(mgr.manifests().is_empty());
    assert!(mgr.registry().providers.is_empty());
    assert!(mgr.registry().strategies.is_empty());
    assert!(mgr.registry().passes.is_empty());
    assert!(mgr.registry().tools.is_empty());
}

#[test]
fn test_plugin_registry_register_tool() {
    use std::sync::Arc;
    use fusion_router::tools::{Tool, builtin::CalculatorTool};

    let mut mgr = PluginManager::new();
    let tool: Arc<dyn Tool + Send + Sync> = Arc::new(CalculatorTool);
    mgr.register_tool(tool);

    let registry = mgr.registry();
    assert_eq!(registry.tools.len(), 1);
    assert!(registry.tools.contains_key("calculator"));
}
