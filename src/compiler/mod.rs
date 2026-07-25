pub mod passes;

use async_trait::async_trait;
use crate::types::{CompilerError, ExecutionGraph, WorkflowIR};

#[async_trait]
pub trait Compiler: Send + Sync {
    async fn compile(&self, ir: WorkflowIR) -> Result<ExecutionGraph, CompilerError>;
}

pub struct DefaultCompiler {
    pub passes: Vec<Box<dyn CompilerPass + Send + Sync>>,
}

#[async_trait]
pub trait CompilerPass: Send + Sync {
    fn name(&self) -> &str;
    async fn apply(&self, ir: WorkflowIR) -> Result<WorkflowIR, CompilerError>;
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

    #[tokio::test]
    async fn test_lowering_to_execution_graph() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let ir = WorkflowIR {
            plan_id: Uuid::new_v4(),
            nodes: vec![
                IRNode {
                    id: id1,
                    kind: IRNodeKind::Generate,
                    strategy: StrategyKind::Single,
                    model: Some("gpt-4".into()),
                    config: HashMap::new(),
                },
                IRNode {
                    id: id2,
                    kind: IRNodeKind::Review,
                    strategy: StrategyKind::Single,
                    model: None,
                    config: HashMap::new(),
                },
            ],
            edges: vec![IREdge {
                from: id1,
                to: id2,
                condition: None,
            }],
            metadata: IRMetadata {
                policy_applied: vec![],
                estimated_cost: 0.5,
                estimated_tokens: 2000,
            },
        };

        let graph = lower_to_graph(ir).unwrap();
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.edges.len(), 1);
        assert_eq!(graph.nodes[0].kind, ExecutionNodeKind::LLMGenerate);
        assert_eq!(graph.nodes[1].kind, ExecutionNodeKind::LLMReview);
        assert_eq!(graph.nodes[0].model, "gpt-4");
        assert_eq!(graph.nodes[1].model, "");
        assert_eq!(graph.nodes[0].retry_policy.max_retries, 2);
        assert_eq!(graph.nodes[0].retry_policy.backoff_ms, 1000);
        assert_eq!(graph.edges[0].from, id1);
        assert_eq!(graph.edges[0].to, id2);
        assert_eq!(graph.total_tokens, 2000);
        assert_eq!(graph.total_cost, 500);
        assert_eq!(graph.metadata.node_count, 2);
    }
}
