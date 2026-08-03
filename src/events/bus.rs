use async_trait::async_trait;
use tokio::sync::broadcast;
use crate::events::ExecutionEventEnvelope;
use crate::release::gate::GateError;

#[async_trait]
pub trait EventBus: Send + Sync {
    async fn publish(&self, envelope: ExecutionEventEnvelope) -> Result<(), GateError>;
    fn subscribe(&self) -> broadcast::Receiver<ExecutionEventEnvelope>;
}

pub struct BroadcastEventBus {
    sender: broadcast::Sender<ExecutionEventEnvelope>,
}

impl BroadcastEventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }
}

impl Default for BroadcastEventBus {
    fn default() -> Self {
        Self::new(1024)
    }
}

#[async_trait]
impl EventBus for BroadcastEventBus {
    async fn publish(&self, envelope: ExecutionEventEnvelope) -> Result<(), GateError> {
        self.sender
            .send(envelope)
            .map(|_| ())
            .map_err(|_| GateError::ExecutionFailed(
                "event bus publish failed: no subscribers listening (event would be lost)".into(),
            ))
    }

    fn subscribe(&self) -> broadcast::Receiver<ExecutionEventEnvelope> {
        self.sender.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::payload::ExecutionEvent;

    #[tokio::test]
    async fn test_broadcast_event_bus_publish_and_subscribe() {
        let bus = BroadcastEventBus::default();
        let mut rx = bus.subscribe();

        let env = ExecutionEventEnvelope::new(
            "wf-1",
            "exec-1",
            Some("corr-1".into()),
            1,
            None,
            ExecutionEvent::WorkflowStarted {
                intent: "Quality".into(),
                input_tokens: 100,
            },
        );

        bus.publish(env.clone()).await.unwrap();

        let received = rx.recv().await.unwrap();
        assert_eq!(received.event_id, env.event_id);
        assert_eq!(received.sequence_number, 1);
        assert_eq!(received.correlation_id, Some("corr-1".into()));
    }

    #[tokio::test]
    async fn test_broadcast_event_bus_publish_without_subscribers_reports_error() {
        let bus = BroadcastEventBus::default();

        let env = ExecutionEventEnvelope::new(
            "wf-1",
            "exec-1",
            None,
            1,
            None,
            ExecutionEvent::WorkflowStarted {
                intent: "Quality".into(),
                input_tokens: 100,
            },
        );

        let result = bus.publish(env).await;
        assert!(result.is_err(), "publish with no receivers must not be silent");
    }
}
