use std::collections::HashMap;
use uuid::Uuid;

use fusion_router::compiler::Compiler;
use fusion_router::compiler::passes::{ConstraintValidationPass, ModelResolutionPass};
use fusion_router::compiler::CompilerPass;
use fusion_router::types::{
    IRMetadata, IRNode, IRNodeKind, StrategyKind, WorkflowIR,
};

fn create_test_ir() -> WorkflowIR {
    WorkflowIR {
        plan_id: Uuid::nil(),
        nodes: vec![
            IRNode {
                id: Uuid::nil(),
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: None,
                config: HashMap::new(),
            },
        ],
        edges: vec![],
        metadata: IRMetadata {
            policy_applied: vec!["test".to_string()],
            estimated_cost: 0.01,
            estimated_tokens: 500,
        },
    }
}

#[tokio::test]
async fn test_compiler_determinism() {
    let compiler = fusion_router::compiler::DefaultCompiler {
        passes: vec![
            Box::new(ConstraintValidationPass),
            Box::new(ModelResolutionPass),
        ],
    };

    let ir = create_test_ir();
    let graph1 = compiler.compile(ir.clone()).await.unwrap();

    let graph2 = compiler.compile(ir).await.unwrap();

    assert_eq!(graph1.nodes.len(), graph2.nodes.len());
    assert_eq!(graph1.edges.len(), graph2.edges.len());
}

#[tokio::test]
async fn test_constraint_validation_empty_ir() {
    let pass = ConstraintValidationPass;
    let empty_ir = WorkflowIR {
        plan_id: Uuid::new_v4(),
        nodes: vec![],
        edges: vec![],
        metadata: IRMetadata {
            policy_applied: vec![],
            estimated_cost: 0.0,
            estimated_tokens: 0,
        },
    };

    let result = pass.apply(empty_ir).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_model_resolution() {
    let pass = ModelResolutionPass;
    let ir = create_test_ir();

    let result = pass.apply(ir).await.unwrap();
    assert!(result.nodes[0].model.is_some());
}
