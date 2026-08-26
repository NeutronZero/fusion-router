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
            // Running watermark of events lost to broadcast lag; included in
            // every lag warning so operators can see cumulative loss.
            let mut lag_total: u64 = 0;
            loop {
                match rx.recv().await {
                    Ok(envelope) => {
                        // Sequential in-task dispatch: per-envelope spawns
                        // reordered delivery (each spawn raced to grab the
                        // projection mutex). The mutex already serializes
                        // handlers, so dispatch inline to preserve order.
                        for proj in &projections {
                            let mut guard = proj.lock().await;
                            if let Err(e) = guard.handle_event(&envelope).await {
                                tracing::error!(error = %e, "event projection handler failed");
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        lag_total = lag_total.saturating_add(skipped);
                        tracing::warn!(
                            skipped = %skipped,
                            lag_total = %lag_total,
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

    struct SeqRecorderProjection {
        seqs: Arc<tokio::sync::Mutex<Vec<u64>>>,
    }

    #[async_trait]
    impl EventProjection for SeqRecorderProjection {
        fn name(&self) -> &'static str {
            "SeqRecorder"
        }

        async fn handle_event(
            &mut self,
            envelope: &ExecutionEventEnvelope,
        ) -> Result<(), GateError> {
            self.seqs.lock().await.push(envelope.sequence_number);
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_projection_delivers_events_in_publish_order() {
        use crate::events::{EventBus, ExecutionEventEnvelope};
        let (tx, _rx) = broadcast::channel(64);
        let bus = RawBroadcastBus { tx: tx.clone() };

        let seqs = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let mut dispatcher = ProjectionDispatcher::new();
        dispatcher.register(SeqRecorderProjection { seqs: seqs.clone() });
        let handle = dispatcher.spawn_listener(&bus);

        // Publish N envelopes with distinct sequence numbers as fast as
        // possible; delivery must preserve publish order (no per-envelope
        // task spawning racing for the mutex).
        const N: u64 = 20;
        for i in 1..=N {
            let env = ExecutionEventEnvelope::new(
                "wf-ord",
                "exec-ord",
                None,
                i,
                None,
                ExecutionEvent::WorkflowStarted {
                    intent: "Quality".into(),
                    input_tokens: 10,
                },
            );
            bus.publish(env).await.unwrap();
        }

        // Wait until all events are recorded (bounded).
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if seqs.lock().await.len() >= N as usize {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for projections; recorded={:?}",
                seqs.lock().await
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        let recorded = seqs.lock().await.clone();
        let expected: Vec<u64> = (1..=N).collect();
        assert_eq!(
            recorded, expected,
            "projection must receive events in publish order"
        );

        handle.abort();
    }
}
