//! Phase 5D — `ReplayEngine` (`src/session/replay.rs`)
//!
//! Event-driven execution replay — reconstructs state from execution traces.

use crate::types::execution_context::{ExecutionEvent, ExecutionState, ExecutionTrace};

pub struct ReplayEngine;

impl ReplayEngine {
    /// Replays an `ExecutionTrace` without calling connectors or producing side-effects.
    pub fn replay_inspection(trace: &ExecutionTrace) -> ExecutionState {
        let events = trace.events();
        let mut final_state = ExecutionState::Pending;

        for event in events {
            if let ExecutionEvent::ExecutionFinished { final_state: s, .. } = event {
                final_state = s;
            }
        }

        final_state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_replay_inspection_mode() {
        let trace = ExecutionTrace::new(Uuid::new_v4());
        trace.record(ExecutionEvent::ExecutionStarted { timestamp_ms: 10 });
        trace.record(ExecutionEvent::ExecutionFinished {
            final_state: ExecutionState::Succeeded,
            timestamp_ms: 20,
        });

        let state = ReplayEngine::replay_inspection(&trace);
        assert_eq!(state, ExecutionState::Succeeded);
    }
}
