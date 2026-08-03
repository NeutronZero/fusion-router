use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use crate::events::projection::EventProjection;
use crate::events::ExecutionEventEnvelope;
use crate::release::gate::GateError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CheckpointPolicy {
    EveryNode,
    EveryNthNode(usize),
    Timed(std::time::Duration),
    Manual,
}

pub struct CheckpointProjection {
    policy: CheckpointPolicy,
    storage_dir: PathBuf,
    node_count: usize,
    saved_sequence_numbers: std::collections::HashSet<u64>,
}

impl CheckpointProjection {
    pub fn new(policy: CheckpointPolicy, storage_dir: PathBuf) -> Self {
        Self {
            policy,
            storage_dir,
            node_count: 0,
            saved_sequence_numbers: std::collections::HashSet::new(),
        }
    }
}

#[async_trait]
impl EventProjection for CheckpointProjection {
    fn name(&self) -> &'static str {
        "CheckpointProjection"
    }

    async fn handle_event(&mut self, envelope: &ExecutionEventEnvelope) -> Result<(), GateError> {
        // Enforce idempotency: skip duplicate event sequence numbers
        if self.saved_sequence_numbers.contains(&envelope.sequence_number) {
            return Ok(());
        }

        if matches!(envelope.payload, crate::events::payload::ExecutionEvent::NodeFinished { .. }) {
            self.node_count += 1;
            let should_checkpoint = match self.policy {
                CheckpointPolicy::EveryNode => true,
                CheckpointPolicy::EveryNthNode(n) => self.node_count.is_multiple_of(n),
                CheckpointPolicy::Timed(_) | CheckpointPolicy::Manual => false,
            };

            if should_checkpoint {
                self.saved_sequence_numbers.insert(envelope.sequence_number);
                let path = self.storage_dir.join(format!("{}-seq{}.chk", envelope.execution_id, envelope.sequence_number));
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        GateError::ExecutionFailed(format!("checkpoint dir create error: {e}"))
                    })?;
                }
                let json = serde_json::to_string_pretty(envelope).map_err(|e| GateError::ExecutionFailed(format!("checkpoint serialize error: {e}")))?;
                std::fs::write(&path, json).map_err(|e| {
                    GateError::ExecutionFailed(format!("checkpoint write error: {e}"))
                })?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::payload::ExecutionEvent;

    #[tokio::test]
    async fn test_checkpoint_projection_idempotency() {
        let temp_dir = std::env::temp_dir().join("fusion_chk_test");
        let mut proj = CheckpointProjection::new(CheckpointPolicy::EveryNode, temp_dir.clone());

        let env = ExecutionEventEnvelope::new(
            "wf-1",
            "exec-1",
            None,
            1,
            None,
            ExecutionEvent::NodeFinished {
                node_id: "node_1".into(),
                duration_ms: 50,
                prompt_tokens: 10,
                completion_tokens: 20,
            },
        );

        proj.handle_event(&env).await.unwrap();
        proj.handle_event(&env).await.unwrap(); // Duplicate call

        assert_eq!(proj.saved_sequence_numbers.len(), 1);
        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[tokio::test]
    async fn test_checkpoint_projection_write_failure_propagates() {
        let temp_dir = std::env::temp_dir().join(format!("fusion_chk_fail_{}", std::process::id()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let blocker = temp_dir.join("blocker");
        std::fs::write(&blocker, "not a directory").unwrap();
        // create_dir_all on this path fails: its parent is a regular file
        let storage_dir = blocker.join("sub");

        let mut proj = CheckpointProjection::new(CheckpointPolicy::EveryNode, storage_dir);

        let env = ExecutionEventEnvelope::new(
            "wf-1",
            "exec-1",
            None,
            1,
            None,
            ExecutionEvent::NodeFinished {
                node_id: "node_1".into(),
                duration_ms: 50,
                prompt_tokens: 10,
                completion_tokens: 20,
            },
        );

        let result = proj.handle_event(&env).await;
        assert!(result.is_err(), "checkpoint write failure must not be silent");
        let _ = std::fs::remove_dir_all(temp_dir);
    }
}
