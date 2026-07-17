use crate::types::{ExecutionNode, ExecutionSubgraph};

pub trait Strategy: Send + Sync {
    fn apply(&self, node: &ExecutionNode) -> ExecutionSubgraph;
}

pub mod single;
pub mod consensus;
pub mod reflection;
pub mod chain;
pub mod react;
pub mod debate;
