use async_trait::async_trait;
use std::path::PathBuf;
use crate::events::projection::EventProjection;
use crate::events::ExecutionEventEnvelope;
use crate::release::gate::GateError;

pub struct PersistentEventStoreProjection {
    storage_dir: PathBuf,
}

impl PersistentEventStoreProjection {
    pub fn new(storage_dir: PathBuf) -> Self {
        Self { storage_dir }
    }

    pub fn load_events(&self, execution_id: &str) -> Result<Vec<ExecutionEventEnvelope>, GateError> {
        let file_path = self.storage_dir.join(format!("{execution_id}.jsonl"));
        if !file_path.exists() {
            return Ok(vec![]);
        }

        let content = std::fs::read_to_string(&file_path)
            .map_err(|e| GateError::ExecutionFailed(format!("read events file {}: {e}", file_path.display())))?;

        let mut events = Vec::new();
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let env: ExecutionEventEnvelope = serde_json::from_str(line)
                .map_err(|e| GateError::ExecutionFailed(format!("parse event line: {e}")))?;
            events.push(env);
        }

        events.sort_by_key(|e| e.sequence_number);
        Ok(events)
    }
}

#[async_trait]
impl EventProjection for PersistentEventStoreProjection {
    fn name(&self) -> &'static str {
        "PersistentEventStoreProjection"
    }

    async fn handle_event(&mut self, envelope: &ExecutionEventEnvelope) -> Result<(), GateError> {
        let file_path = self.storage_dir.join(format!("{}.jsonl", envelope.execution_id));
        if let Some(parent) = file_path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        let json = serde_json::to_string(envelope)
            .map_err(|e| GateError::ExecutionFailed(format!("serialize event: {e}")))?;

        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .await
            .map_err(|e| GateError::ExecutionFailed(format!("open event log {}: {e}", file_path.display())))?;

        file.write_all(format!("{json}\n").as_bytes())
            .await
            .map_err(|e| GateError::ExecutionFailed(format!("write event log {}: {e}", file_path.display())))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::payload::ExecutionEvent;

    #[tokio::test]
    async fn test_persistent_event_store_append_and_load() {
        let temp_dir = std::env::temp_dir().join(format!("fusion_store_test_{}", uuid::Uuid::new_v4()));
        let mut store = PersistentEventStoreProjection::new(temp_dir.clone());

        let env1 = ExecutionEventEnvelope::new(
            "wf-1",
            "exec-999",
            None,
            1,
            None,
            ExecutionEvent::WorkflowStarted {
                intent: "Quality".into(),
                input_tokens: 10,
            },
        );

        let env2 = ExecutionEventEnvelope::new(
            "wf-1",
            "exec-999",
            None,
            2,
            Some(env1.event_id.clone()),
            ExecutionEvent::WorkflowCompleted {
                total_duration_ms: 100,
                total_cost_usd: 0.001,
            },
        );

        store.handle_event(&env1).await.unwrap();
        store.handle_event(&env2).await.unwrap();

        let loaded = store.load_events("exec-999").unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].sequence_number, 1);
        assert_eq!(loaded[1].sequence_number, 2);

        let _ = std::fs::remove_dir_all(temp_dir);
    }
}
