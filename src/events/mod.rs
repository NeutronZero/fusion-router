pub mod bus;
pub mod consumers;
pub mod payload;
pub mod projection;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

pub use bus::{BroadcastEventBus, EventBus};
pub use payload::ExecutionEvent;
#[allow(unused_imports)]
pub use projection::{EventProjection, ProjectionDispatcher};

pub const EVENT_SCHEMA_VERSION: &str = "fusion.router.event.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionEventEnvelope {
    pub schema_version: String,
    pub event_id: String,
    pub workflow_id: String,
    pub execution_id: String,
    pub correlation_id: Option<String>,
    pub sequence_number: u64,
    pub timestamp: DateTime<Utc>,
    pub parent_event_id: Option<String>,
    pub payload: ExecutionEvent,
}

impl ExecutionEventEnvelope {
    pub fn new(
        workflow_id: impl Into<String>,
        execution_id: impl Into<String>,
        correlation_id: Option<String>,
        sequence_number: u64,
        parent_event_id: Option<String>,
        payload: ExecutionEvent,
    ) -> Self {
        let timestamp = Utc::now();
        let wf_id = workflow_id.into();
        let exec_id = execution_id.into();

        let mut hasher = DefaultHasher::new();
        wf_id.hash(&mut hasher);
        exec_id.hash(&mut hasher);
        sequence_number.hash(&mut hasher);
        timestamp.hash(&mut hasher);
        let hash = hasher.finish();
        let event_id = format!("evt-{:012x}", hash);

        Self {
            schema_version: EVENT_SCHEMA_VERSION.to_string(),
            event_id,
            workflow_id: wf_id,
            execution_id: exec_id,
            correlation_id,
            sequence_number,
            timestamp,
            parent_event_id,
            payload,
        }
    }
}
