pub mod context;
pub mod diagnostics;
pub mod ir;
pub mod passes;
pub mod registry;
pub mod optimization;
pub mod pipeline;
pub mod strategy_expansion;

use async_trait::async_trait;
use std::sync::Arc;
use crate::types::{CompilerError, ExecutionGraph, WorkflowIR};
pub use passes::CompilerPass;

#[async_trait]
pub trait Compiler: Send + Sync {
    async fn compile(&self, ir: WorkflowIR) -> Result<ExecutionGraph, CompilerError>;
}

pub struct DefaultCompiler {
    pub passes: Vec<Box<dyn CompilerPass + Send + Sync>>,
}

/// Builds the mandatory compiler pass pipeline (ADR-034 / Law 1).
///
/// This is the sole production construction path for `DefaultCompiler`.
/// Every execution endpoint (chat, `/v1/executions`, triggers) must compile
/// through a compiler produced here; an empty pass list is never
/// constructible from this factory. A supplied `PolicyIR` appends the
/// policy pass (deny = compile error, per Law 2).
pub fn build_compiler(
    model_catalog: crate::types::ModelCatalog,
    resource_manager: Arc<dyn crate::resource::ResourceManager>,
    policy_ir: Option<crate::policy::ir::PolicyIR>,
) -> DefaultCompiler {
    let mut passes: Vec<Box<dyn CompilerPass + Send + Sync>> = vec![
        Box::new(passes::ConstraintValidationPass),
        Box::new(passes::ControlFlowValidationPass),
        Box::new(passes::ModelResolutionPass {
            model_catalog,
            model_requirements: None,
        }),
        Box::new(passes::BudgetOptimisationPass { resource_manager }),
    ];
    if let Some(ir) = policy_ir {
        passes.push(Box::new(passes::policy::PolicyCompilerPass::new(ir)));
    }
    DefaultCompiler { passes }
}

#[async_trait]
impl Compiler for DefaultCompiler {
    async fn compile(&self, ir: WorkflowIR) -> Result<ExecutionGraph, CompilerError> {
        let snapshot = ir.clone();
        let mut current = ir;

        for pass in &self.passes {
            tracing::debug!(pass = %pass.name(), "running compiler pass");
            match pass.apply(current.clone()).await {
                Ok(next) => {
                    current = next;
                }
                Err(e) => {
                    tracing::warn!(
                        pass = %pass.name(),
                        error = %e,
                        plan_id = %snapshot.plan_id,
                        "compiler pass failed; transaction rolled back to initial IR snapshot"
                    );
                    return Err(e);
                }
            }
        }

        lower_to_graph(current)
    }
}

pub(crate) fn lower_to_graph(ir: WorkflowIR) -> Result<ExecutionGraph, CompilerError> {
    let mut exec_nodes = Vec::new();
    let mut exec_edges = Vec::new();

    for ir_node in &ir.nodes {
        exec_nodes.push(crate::types::ExecutionNode {
            id: ir_node.id,
            kind: match ir_node.kind {
                crate::types::IRNodeKind::Generate => crate::types::ExecutionNodeKind::LLMGenerate,
                crate::types::IRNodeKind::Review => crate::types::ExecutionNodeKind::LLMReview,
                crate::types::IRNodeKind::Judge => crate::types::ExecutionNodeKind::LLMJudge,
                crate::types::IRNodeKind::Transform => crate::types::ExecutionNodeKind::Transform,
                crate::types::IRNodeKind::Gate => crate::types::ExecutionNodeKind::Gate,
                crate::types::IRNodeKind::Conditional => crate::types::ExecutionNodeKind::Conditional,
                crate::types::IRNodeKind::Loop => crate::types::ExecutionNodeKind::Loop,
                crate::types::IRNodeKind::Split => crate::types::ExecutionNodeKind::Split,
                crate::types::IRNodeKind::Join => crate::types::ExecutionNodeKind::Join,
                crate::types::IRNodeKind::Barrier => crate::types::ExecutionNodeKind::Barrier,
            },
            strategy: ir_node.strategy.clone(),
            model: ir_node.model.clone().unwrap_or_default(),
            retry_policy: crate::types::RetryPolicy {
                max_retries: 2,
                backoff_ms: 1000,
            },
            fallback: None,
            config: ir_node.config.clone(),
            subgraph: None,
        });
    }

    for ir_edge in &ir.edges {
        exec_edges.push(crate::types::ExecutionEdge {
            from: ir_edge.from,
            to: ir_edge.to,
            condition: ir_edge.condition.clone(),
        });
    }

    // Compile-time strategy expansion: lower every non-passthrough strategy
    // node into a prebuilt `ExecutionSubgraph` (pure, deterministic — see
    // `strategy_expansion`). The executor executes these subgraphs directly
    // and no longer lowers strategies on the live path.
    for node in &mut exec_nodes {
        node.subgraph = strategy_expansion::expanded_subgraph(node);
    }

    let total_cost = (ir.metadata.estimated_cost * 1000.0) as u64;
    let total_tokens = ir.metadata.estimated_tokens;

    Ok(ExecutionGraph {
        graph_id: ir.plan_id,
        nodes: exec_nodes,
        edges: exec_edges,
        metadata: crate::types::GraphMetadata {
            estimated_cost: ir.metadata.estimated_cost,
            estimated_tokens: ir.metadata.estimated_tokens,
            max_depth: 1,
            node_count: ir.nodes.len() as u32,
        },
        primitive_graph_hash: 0,
        total_tokens,
        total_cost,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;
    use std::collections::HashMap;
    use uuid::Uuid;

    struct FailPass;

    #[async_trait]
    impl CompilerPass for FailPass {
        fn name(&self) -> &str {
            "fail_pass"
        }

        async fn apply(&self, _ir: WorkflowIR) -> Result<WorkflowIR, CompilerError> {
            Err(CompilerError::PassError {
                pass: "fail_pass".into(),
                message: "intentional rollback failure".into(),
            })
        }
    }

    fn test_ir() -> WorkflowIR {
        WorkflowIR {
            plan_id: Uuid::new_v4(),
            nodes: vec![IRNode {
                id: Uuid::new_v4(),
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: None,
                config: HashMap::new(),
            }],
            edges: vec![],
            metadata: IRMetadata {
                policy_applied: vec![],
                estimated_cost: 0.1,
                estimated_tokens: 500,
            },
        }
    }

    #[tokio::test]
    async fn test_transactional_rollback() {
        let ir = test_ir();
        let compiler = DefaultCompiler {
            passes: vec![Box::new(FailPass)],
        };

        let result = compiler.compile(ir).await;
        match result {
            Err(CompilerError::PassError { pass, .. }) => {
                assert_eq!(pass, "fail_pass");
            }
            _ => panic!("expected PassError"),
        }
    }

    fn permissive_quota() -> crate::types::Quota {
        crate::types::Quota {
            max_daily_cost: 1_000_000.0,
            max_daily_tokens: 1_000_000_000,
            max_concurrent: 100,
            provider_limits: std::collections::HashMap::new(),
        }
    }

    fn test_compiler() -> DefaultCompiler {
        build_compiler(
            crate::types::ModelCatalog::default(),
            Arc::new(crate::resource::DefaultResourceManager::new(permissive_quota())),
            None,
        )
    }

    #[test]
    fn law1_build_compiler_contains_mandatory_passes_in_order() {
        let compiler = test_compiler();
        let names: Vec<String> = compiler.passes.iter().map(|p| p.name().to_string()).collect();
        assert_eq!(
            names,
            vec![
                "constraint_validation",
                "control_flow_validation",
                "model_resolution",
                "budget_optimisation",
            ],
            "mandatory pass pipeline must be present and ordered"
        );
        assert!(!compiler.passes.is_empty(), "Law 1: no execution path may use an empty pass pipeline");
    }

    #[tokio::test]
    async fn law1_compiler_rejects_ir_violating_control_flow_validation() {
        let compiler = test_compiler();
        let mut ir = test_ir();
        // Edge referencing an unknown node must be rejected by the pipeline.
        ir.edges.push(crate::types::IREdge {
            from: uuid::Uuid::new_v4(),
            to: uuid::Uuid::new_v4(),
            condition: None,
        });
        let result = compiler.compile(ir).await;
        assert!(result.is_err(), "dangling edge must fail compilation");
        assert!(matches!(
            result,
            Err(CompilerError::ValidationError { pass, .. }) if pass == "control_flow_validation"
        ));
    }

    #[tokio::test]
    async fn law2_deny_blocks_compilation() {
        let json_raw = r#"{
            "version": "1.0",
            "declarations": [
                {
                    "name": "deny-shell",
                    "priority": 100,
                    "match_target": "shell.exec",
                    "effect": "deny",
                    "conditions": {},
                    "annotations": {}
                }
            ]
        }"#;
        let (ast, _) = crate::policy::ast::PolicyParser::parse_json(json_raw).unwrap();
        let policy_ir = crate::policy::ir::PolicyIR::from_ast(&ast).unwrap();

        let compiler = build_compiler(
            crate::types::ModelCatalog::default(),
            Arc::new(crate::resource::DefaultResourceManager::new(permissive_quota())),
            Some(policy_ir),
        );

        let mut ir = test_ir();
        ir.nodes[0].config.insert(
            "capability".into(),
            serde_json::json!("shell.exec"),
        );
        let result = compiler.compile(ir).await;
        assert!(result.is_err(), "a matched Deny rule must block compilation through the factory");
    }

    #[tokio::test]
    async fn law4_compile_failure_yields_no_graph() {
        let compiler = test_compiler();
        // Empty IR fails ConstraintValidationPass; compile must return Err, not a graph.
        let empty_ir = crate::types::WorkflowIR {
            plan_id: uuid::Uuid::new_v4(),
            nodes: vec![],
            edges: vec![],
            metadata: crate::types::IRMetadata {
                policy_applied: vec![],
                estimated_cost: 0.0,
                estimated_tokens: 0,
            },
        };
        let result = compiler.compile(empty_ir).await;
        assert!(result.is_err(), "ExecutionGraph construction is impossible after compiler failure");
    }
}
