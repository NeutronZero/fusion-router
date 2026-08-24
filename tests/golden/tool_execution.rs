use fusion_router::tools::builtin::CalculatorTool;
use fusion_router::tools::ToolRegistry;
use std::sync::Arc;

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
