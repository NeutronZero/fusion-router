use async_trait::async_trait;

mod fusion_bridge;
mod node_exec;
pub use node_exec::DefaultExecutor;

use crate::types::{ExecutionNode, NodeExecContext, NodeExecutionResult};

#[async_trait]
pub trait Executor: Send + Sync {
    async fn execute_node(
        &self,
        node: &ExecutionNode,
        ctx: &NodeExecContext,
    ) -> NodeExecutionResult;
}

