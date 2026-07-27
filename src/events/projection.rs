use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::events::{EventBus, ExecutionEventEnvelope};
use crate::release::gate::GateError;

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
        Self { projections: Vec::new() }
    }

    pub fn register(&mut self, projection: impl EventProjection + 'static) {
        self.projections.push(Arc::new(Mutex::new(Box::new(projection))));
    }

    pub fn spawn_listener(self, bus: &dyn EventBus) -> tokio::task::JoinHandle<()> {
        let mut rx = bus.subscribe();
        let projections = self.projections;

        tokio::spawn(async move {
            while let Ok(envelope) = rx.recv().await {
                for proj in &projections {
                    let proj = Arc::clone(proj);
                    let env = envelope.clone();
                    // Isolated background dispatch to prevent panic/delay leakage
                    tokio::spawn(async move {
                        let mut guard = proj.lock().await;
                        let _ = guard.handle_event(&env).await;
                    });
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::bus::BroadcastEventBus;
    use crate::events::payload::ExecutionEvent;

    struct TestCounterProjection {
        count: usize,
    }

    #[async_trait]
    impl EventProjection for TestCounterProjection {
        fn name(&self) -> &'static str {
            "TestCounter"
        }

        async fn handle_event(&mut self, _envelope: &ExecutionEventEnvelope) -> Result<(), GateError> {
            self.count += 1;
            Ok(())
        }
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
