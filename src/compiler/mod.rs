pub mod context;
pub mod diagnostics;
pub mod ir;
pub mod passes;
pub mod registry;
pub mod optimization;
pub mod pipeline;

use async_trait::async_trait;
use crate::types::{CompilerError, ExecutionGraph, WorkflowIR};
pub use passes::CompilerPass;

#[async_trait]
pub trait Compiler: Send + Sync {
    async fn compile(&self, ir: WorkflowIR) -> Result<ExecutionGraph, CompilerError>;
}

pub struct DefaultCompiler {
    pub passes: Vec<Box<dyn CompilerPass + Send + Sync>>,
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
        });
    }

    for ir_edge in &ir.edges {
        exec_edges.push(crate::types::ExecutionEdge {
            from: ir_edge.from,
            to: ir_edge.to,
            condition: ir_edge.condition.clone(),
        });
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
}
