use crate::events::{EventBus, ExecutionEventEnvelope};
use crate::release::gate::GateError;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

#[async_trait]
pub trait EventProjection: Send + Sync {
    fn name(&self) -> &'static str;
    async fn handle_event(&mut self, envelope: &ExecutionEventEnvelope) -> Result<(), GateError>;
}

pub struct ProjectionDispatcher {
    projections: Vec<Arc<Mutex<Box<dyn EventProjection>>>>,
}

impl ProjectionDispatcher {
    pub fn new() -> Self {
        Self {
            projections: Vec::new(),
        }
    }

    pub fn register(&mut self, projection: impl EventProjection + 'static) {
        self.projections
            .push(Arc::new(Mutex::new(Box::new(projection))));
    }

    pub fn spawn_listener(self, bus: &dyn EventBus) -> tokio::task::JoinHandle<()> {
        let mut rx = bus.subscribe();
        let projections = self.projections;

        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(envelope) => {
                        for proj in &projections {
                            let proj = Arc::clone(proj);
                            let env = envelope.clone();
                            // Isolated background dispatch to prevent panic/delay leakage
                            tokio::spawn(async move {
                                let mut guard = proj.lock().await;
                                if let Err(e) = guard.handle_event(&env).await {
                                    tracing::error!(error = %e, "event projection handler failed");
                                }
                            });
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(
                            skipped = %skipped,
                            "event bus lagged; skipped events are lost to this projection listener"
                        );
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        tracing::error!("event bus closed; projection listener exiting");
                        break;
                    }
                }
            }
        })
    }
}

impl Default for ProjectionDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

/// Observability projection: logs every event envelope at INFO level.
pub struct LoggingProjection;

#[async_trait]
impl EventProjection for LoggingProjection {
    fn name(&self) -> &'static str {
        "LoggingProjection"
    }

    async fn handle_event(&mut self, envelope: &ExecutionEventEnvelope) -> Result<(), GateError> {
        tracing::info!(
            workflow_id = %envelope.workflow_id,
            execution_id = %envelope.execution_id,
            sequence = %envelope.sequence_number,
            event = ?envelope.payload,
            "execution event"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::bus::BroadcastEventBus;
    use crate::events::payload::ExecutionEvent;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::sync::broadcast;

    struct RawBroadcastBus {
        tx: broadcast::Sender<ExecutionEventEnvelope>,
    }

    #[async_trait]
    impl EventBus for RawBroadcastBus {
        async fn publish(&self, envelope: ExecutionEventEnvelope) -> Result<(), GateError> {
            self.tx
                .send(envelope)
                .map(|_| ())
                .map_err(|_| GateError::ExecutionFailed("no receivers".into()))
        }
        fn subscribe(&self) -> broadcast::Receiver<ExecutionEventEnvelope> {
            self.tx.subscribe()
        }
    }

    struct CountingProjection {
        count: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl EventProjection for CountingProjection {
        fn name(&self) -> &'static str {
            "Counting"
        }

        async fn handle_event(
            &mut self,
            _envelope: &ExecutionEventEnvelope,
        ) -> Result<(), GateError> {
            self.count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    struct TestCounterProjection {
        count: usize,
    }

    #[async_trait]
    impl EventProjection for TestCounterProjection {
        fn name(&self) -> &'static str {
            "TestCounter"
        }

        async fn handle_event(
            &mut self,
            _envelope: &ExecutionEventEnvelope,
        ) -> Result<(), GateError> {
            self.count += 1;
            Ok(())
        }
    }

    fn sample_env() -> ExecutionEventEnvelope {
        ExecutionEventEnvelope::new(
            "wf-1",
            "exec-1",
            None,
            1,
            None,
            ExecutionEvent::WorkflowStarted {
                intent: "Quality".into(),
                input_tokens: 50,
            },
        )
    }

    #[tokio::test]
    async fn test_logging_projection_accepts_all_events() {
        let mut projection = LoggingProjection;
        let env = ExecutionEventEnvelope::new(
            "wf-1",
            "exec-1",
            None,
            1,
            None,
            ExecutionEvent::WorkflowStarted {
                intent: "Quality".into(),
                input_tokens: 50,
            },
        );
        assert!(projection.handle_event(&env).await.is_ok());
        assert_eq!(projection.name(), "LoggingProjection");
    }

    #[tokio::test]
    async fn test_projection_listener_survives_lagged_receive() {
        let (tx, _rx) = broadcast::channel(1);
        let bus = RawBroadcastBus { tx: tx.clone() };
        let count = Arc::new(AtomicUsize::new(0));

        let mut dispatcher = ProjectionDispatcher::new();
        dispatcher.register(CountingProjection {
            count: count.clone(),
        });
        let handle = dispatcher.spawn_listener(&bus);

        tx.send(sample_env()).unwrap();
        tx.send(sample_env()).unwrap();
        tx.send(sample_env()).unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(
            count.load(Ordering::SeqCst),
            1,
            "lagged listener must skip to the latest event and keep running"
        );

        tx.send(sample_env()).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(
            count.load(Ordering::SeqCst),
            2,
            "listener must still be alive after a lag"
        );

        handle.abort();
    }

    #[tokio::test]
    async fn test_projection_listener_exits_when_bus_closed() {
        let (tx, _rx) = broadcast::channel(16);
        let bus = RawBroadcastBus { tx: tx.clone() };
        let count = Arc::new(AtomicUsize::new(0));

        let mut dispatcher = ProjectionDispatcher::new();
        dispatcher.register(CountingProjection {
            count: count.clone(),
        });
        let handle = dispatcher.spawn_listener(&bus);

        tx.send(sample_env()).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(count.load(Ordering::SeqCst), 1);

        drop(bus);
        drop(tx);
        drop(_rx);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(
            handle.is_finished(),
            "listener must exit (not hang) when the event bus is closed"
        );
    }

    #[tokio::test]
    async fn test_projection_dispatcher_isolated_fan_out() {
        let bus = BroadcastEventBus::default();
        let mut dispatcher = ProjectionDispatcher::new();
        dispatcher.register(TestCounterProjection { count: 0 });

        let handle = dispatcher.spawn_listener(&bus);

        let env = ExecutionEventEnvelope::new(
            "wf-1",
            "exec-1",
            None,
            1,
            None,
            ExecutionEvent::WorkflowStarted {
                intent: "Quality".into(),
                input_tokens: 50,
            },
        );

        bus.publish(env).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        handle.abort();
    }
}
