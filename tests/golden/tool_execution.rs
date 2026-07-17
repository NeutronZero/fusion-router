use std::collections::HashMap;
use std::sync::Arc;
use fusion_router::tools::ToolRegistry;
use fusion_router::tools::builtin::CalculatorTool;
use fusion_router::strategies::react::ReActStrategy;
use fusion_router::types::*;
use uuid::Uuid;
use fusion_router::strategies::Strategy;

#[tokio::test]
async fn test_tool_registry_injects_available_tools() {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(CalculatorTool));
    assert!(registry.contains("calculator"));
    assert_eq!(registry.len(), 1);
}

#[tokio::test]
async fn test_tool_registry_list() {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(CalculatorTool));
    let names = registry.list();
    assert!(names.contains(&"calculator"));
}

#[test]
fn test_react_with_tool_registry_injects_available_tools() {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(CalculatorTool));

    let strategy = ReActStrategy::new(10, Some(Arc::new(registry)));

    let node = ExecutionNode {
        id: Uuid::new_v4(),
        kind: ExecutionNodeKind::LLMGenerate,
        strategy: StrategyKind::ReAct,
        model: "test-model".into(),
        retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
        fallback: None,
        config: HashMap::new(),
    };

    let subgraph = strategy.apply(&node);
    assert_eq!(subgraph.nodes.len(), 2);

    // The second node should have available_tools in its config
    let gen_node = &subgraph.nodes[1];
    let tools = gen_node.config.get("available_tools");
    assert!(tools.is_some(), "available_tools should be injected into config");
    assert!(tools.unwrap().as_array().unwrap().contains(&serde_json::json!("calculator")));
}
