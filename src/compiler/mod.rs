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
        let mut current = ir;

        for pass in &self.passes {
            tracing::debug!(pass = %pass.name(), "running compiler pass");
            current = pass.apply(current).await?;
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
        });
    }

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
    })
}
