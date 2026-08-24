use crate::types::NanoUSD;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ExecutionEvent {
    // Workflow Lifecycle
    WorkflowStarted {
        intent: String,
        input_tokens: usize,
    },
    WorkflowCompleted {
        total_duration_ms: u64,
        total_cost: NanoUSD,
    },
    WorkflowFailed {
        error: String,
        failed_node_id: Option<String>,
    },

    // Compilation & Scheduling
    WorkflowCompiled {
        node_count: usize,
        edge_count: usize,
        primitive_graph_hash: u64,
    },
    NodeScheduled {
        node_id: String,
        node_kind: String,
        dependencies: Vec<String>,
    },

    // Node Execution Loop
    NodeStarted {
        node_id: String,
        target_model: Option<String>,
    },
    NodeFinished {
        node_id: String,
        duration_ms: u64,
        prompt_tokens: u32,
        completion_tokens: u32,
    },
    NodeFailed {
        node_id: String,
        error: String,
        attempt: u32,
    },

    // Resilience & Retry
    RetryStarted {
        node_id: String,
        attempt: u32,
        backoff_ms: u64,
    },
    RetrySucceeded {
        node_id: String,
        attempt: u32,
    },

    // Transport, Provider & Tool Activity
    ProviderCalled {
        provider: String,
        model: String,
        prompt_bytes: usize,
    },
    ProviderResponded {
        provider: String,
        model: String,
        duration_ms: u64,
        cost: NanoUSD,
    },
    ToolInvoked {
        tool_name: String,
        node_id: String,
    },
    ToolCompleted {
        tool_name: String,
        node_id: String,
        duration_ms: u64,
        success: bool,
    },

    // Context & Resource Lifecycle
    ContextMaterialized {
        node_id: String,
        context_size_bytes: usize,
    },
    ResourceAllocated {
        resource_type: String,
        amount: f64,
    },
    ResourceReleased {
        resource_type: String,
        amount: f64,
    },
    SemaphoreAcquired {
        resource_name: String,
        permits: u32,
    },
    SemaphoreReleased {
        resource_name: String,
        permits: u32,
    },
    BudgetExceeded {
        resource_type: String,
        limit: f64,
        actual: f64,
    },
}
