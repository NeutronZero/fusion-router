pub mod workflow;

pub use workflow::{
    WorkflowIR, WorkflowNode, WorkflowNodeKind, WorkflowEdge, WorkflowEdgeKind, WorkflowMetadata,
    WORKFLOW_IR_VERSION,
};
