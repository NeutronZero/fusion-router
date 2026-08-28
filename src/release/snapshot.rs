//! Snapshot Payload v2 + cassette replay verification (ADR-042).
//!
//! A v2 snapshot records the exact inputs (`WorkflowIR`), a scripted provider
//! cassette, and the event trace the pipeline produced when it was recorded.
//! Verification recompiles and re-executes against the cassette â€” no network â€”
//! then diffs normalized traces. Volatile fields (timings, token counts, cost)
//! are stripped before comparison; identity fields (event sequence, node ids,
//! models) must match exactly.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use crate::events::payload::ExecutionEvent;
use crate::events::{BroadcastEventBus, EventBus};
use crate::providers::ChatProvider;
use crate::server::execution::{build_execution_plane, ExecuteWorkflowRequest, ExecutionPlane};
use crate::types::{
    ChatCompletionRequest, ChatCompletionResponse, ModelCatalog, Quota, WorkflowIR,
};

pub const SNAPSHOT_SCHEMA_VERSION: u32 = 2;

/// One recorded provider response, matched strictly in call order against the
/// requesting model name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CassetteEntry {
    pub model: String,
    pub response: ChatCompletionResponse,
}

/// Payload for `schema_version: 2` snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotPayloadV2 {
    pub schema_version: u32,
    pub workflow_ir: WorkflowIR,
    pub provider_cassette: Vec<CassetteEntry>,
    pub expected_events: Vec<ExecutionEvent>,
}

/// Provider that replays a recorded response cassette in strict order
/// (ADR-042). A model mismatch or an exhausted cassette is a verification
/// failure â€” both indicate contract drift between record and replay.
pub struct CassetteProvider {
    cassette: Mutex<VecDeque<CassetteEntry>>,
}

impl CassetteProvider {
    pub fn new(entries: Vec<CassetteEntry>) -> Self {
        Self {
            cassette: Mutex::new(entries.into()),
        }
    }

    pub fn remaining(&self) -> usize {
        self.cassette
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }
}

#[async_trait::async_trait]
impl ChatProvider for CassetteProvider {
    fn name(&self) -> &str {
        "cassette"
    }

    async fn chat_completion(
        &self,
        request: &ChatCompletionRequest,
    ) -> anyhow::Result<ChatCompletionResponse> {
        let mut queue = self.cassette.lock().unwrap_or_else(|e| e.into_inner());
        let entry = queue
            .pop_front()
            .ok_or_else(|| anyhow::anyhow!("provider cassette exhausted"))?;
        if entry.model != request.model {
            return Err(anyhow::anyhow!(
                "cassette order drift: recorded response for '{}', request was for '{}'",
                entry.model,
                request.model
            ));
        }
        Ok(entry.response)
    }
}

/// Fields that legitimately differ between the recording run and the replay
/// run. Everything else must match exactly.
const VOLATILE_FIELDS: &[&str] = &[
    "duration_ms",
    "total_duration_ms",
    "cost",
    "total_cost",
    "prompt_tokens",
    "completion_tokens",
    "input_tokens",
    "prompt_bytes",
];

fn normalize_event(event: &ExecutionEvent) -> Value {
    let mut value = serde_json::to_value(event).unwrap_or(Value::Null);
    // The payload enum serializes as {"type": ..., "data": {...}}.
    if let Some(Value::Object(map)) = value.get_mut("data") {
        for key in VOLATILE_FIELDS {
            map.remove(*key);
        }
    }
    value
}

/// Compares two traces after volatile-field normalization. Returns Err with
/// the first divergence index on mismatch.
pub fn diff_traces(expected: &[ExecutionEvent], actual: &[ExecutionEvent]) -> Result<(), String> {
    if expected.len() != actual.len() {
        return Err(format!(
            "event count diverges: expected {}, actual {}",
            expected.len(),
            actual.len()
        ));
    }
    for (i, (e, a)) in expected.iter().zip(actual.iter()).enumerate() {
        let ne = normalize_event(e);
        let na = normalize_event(a);
        if ne != na {
            return Err(format!(
                "event {i} diverges:\n  expected: {ne}\n  actual:   {na}"
            ));
        }
    }
    Ok(())
}

/// Bus + plane pair used by both the recorder and the verifier.
pub struct ReplayHarness {
    pub bus: Arc<BroadcastEventBus>,
    pub plane: Arc<ExecutionPlane>,
}

pub fn build_replay_harness(
    provider: Arc<dyn ChatProvider + Send + Sync>,
    model_catalog: ModelCatalog,
    quota: Quota,
) -> ReplayHarness {
    let bus = Arc::new(BroadcastEventBus::new(1024));
    let executor = Arc::new(crate::executor::DefaultExecutor::new(
        provider,
        HashMap::new(),
    ));
    let plane = build_execution_plane(
        bus.clone(),
        executor,
        model_catalog,
        Arc::new(crate::resource::DefaultResourceManager::new(quota)),
        Arc::new(crate::policy::PolicyRegistry::offline_default()),
    );
    ReplayHarness { bus, plane }
}

fn replay_request(workflow_ir: WorkflowIR) -> ExecuteWorkflowRequest {
    ExecuteWorkflowRequest {
        trigger_name: "replay".into(),
        kind: "replay".into(),
        intent: "Replay".into(),
        payload: serde_json::json!({}),
        workflow: workflow_ir,
    }
}

/// Drains the bus until a terminal workflow event arrives (or timeout).
async fn drain_events(
    mut rx: tokio::sync::broadcast::Receiver<crate::events::ExecutionEventEnvelope>,
) -> Vec<ExecutionEvent> {
    let mut events = Vec::new();
    while let Ok(Ok(envelope)) = tokio::time::timeout(std::time::Duration::from_secs(30), rx.recv()).await {
        let terminal = matches!(
            envelope.payload,
            ExecutionEvent::WorkflowCompleted { .. }
                | ExecutionEvent::WorkflowFailed { .. }
        );
        events.push(envelope.payload);
        if terminal {
            break;
        }
    }
    events
}

/// Records a snapshot's event trace by running `request` through the harness
/// with a live provider. Used by the producer; output feeds
/// [`SnapshotPayloadV2::expected_events`].
pub async fn record_trace(
    harness: &ReplayHarness,
    request: ExecuteWorkflowRequest,
) -> Result<Vec<ExecutionEvent>, String> {
    let rx = harness.bus.subscribe();
    let execute_result = harness.plane.execute(request).await;
    let mut events = drain_events(rx).await;
    // The bus drain can race a fast completion on broadcast lag; guarantee the
    // trace always contains its terminal event.
    if !events.iter().any(|e| {
        matches!(
            e,
            ExecutionEvent::WorkflowCompleted { .. } | ExecutionEvent::WorkflowFailed { .. }
        )
    }) {
        events.push(match &execute_result {
            Ok(_) => ExecutionEvent::WorkflowCompleted {
                total_duration_ms: 0,
                total_cost: crate::types::NanoUSD::ZERO,
            },
            Err(e) => ExecutionEvent::WorkflowFailed {
                error: e.clone(),
                failed_node_id: None,
            },
        });
    }
    execute_result.map(|_| events)
}

/// Verifies one schema_version-2 snapshot payload by recompiling its IR and
/// re-executing it against the recorded cassette, then diffing traces.
pub async fn verify_payload_v2(payload_bytes: &[u8]) -> Result<(), String> {
    let payload: SnapshotPayloadV2 = serde_json::from_slice(payload_bytes)
        .map_err(|e| format!("v2 payload does not deserialize: {e}"))?;
    if payload.schema_version != SNAPSHOT_SCHEMA_VERSION {
        return Err(format!(
            "expected schema_version {SNAPSHOT_SCHEMA_VERSION}, found {}",
            payload.schema_version
        ));
    }

    let provider = Arc::new(CassetteProvider::new(payload.provider_cassette.clone()));
    let harness = build_replay_harness(
        provider,
        ModelCatalog::default(),
        Quota {
            max_daily_cost: crate::types::NanoUSD::from_nanos(u64::MAX),
            max_daily_tokens: u64::MAX,
            max_concurrent: 16,
            provider_limits: HashMap::new(),
        },
    );
    let rx = harness.bus.subscribe();
    let request = replay_request(payload.workflow_ir.clone());
    let execute_result = harness.plane.execute(request).await;
    let actual = drain_events(rx).await;

    if let Err(e) = execute_result {
        // A failed replay is only acceptable if the recording itself failed.
        let recorded_failure = payload
            .expected_events
            .iter()
            .any(|e| matches!(e, ExecutionEvent::WorkflowFailed { .. }));
        if !recorded_failure {
            return Err(format!(
                "replay execution failed where recording succeeded: {e}"
            ));
        }
    }

    diff_traces(&payload.expected_events, &actual)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_strips_volatile_fields() {
        let event = ExecutionEvent::NodeFinished {
            node_id: "n1".into(),
            duration_ms: 9999,
            prompt_tokens: 42,
            completion_tokens: 7,
        };
        let v = normalize_event(&event);
        let data = v.get("data").expect("tagged payload");
        assert!(data.get("duration_ms").is_none());
        assert!(data.get("prompt_tokens").is_none());
        assert!(data.get("completion_tokens").is_none());
        assert_eq!(data.get("node_id").and_then(|v| v.as_str()), Some("n1"));
    }

    #[test]
    fn test_diff_accepts_identical_traces_with_different_timings() {
        let recorded = vec![
            ExecutionEvent::NodeStarted {
                node_id: "a".into(),
                target_model: Some("m".into()),
            },
            ExecutionEvent::NodeFinished {
                node_id: "a".into(),
                duration_ms: 10,
                prompt_tokens: 1,
                completion_tokens: 2,
            },
        ];
        let replayed = vec![
            ExecutionEvent::NodeStarted {
                node_id: "a".into(),
                target_model: Some("m".into()),
            },
            ExecutionEvent::NodeFinished {
                node_id: "a".into(),
                duration_ms: 500,
                prompt_tokens: 99,
                completion_tokens: 100,
            },
        ];
        assert!(diff_traces(&recorded, &replayed).is_ok());
    }

    #[test]
    fn test_diff_reports_first_divergence_index() {
        let expected = vec![ExecutionEvent::WorkflowStarted {
            intent: "x".into(),
            input_tokens: 1,
        }];
        let actual = vec![ExecutionEvent::WorkflowFailed {
            error: "boom".into(),
            failed_node_id: None,
        }];
        let err = diff_traces(&expected, &actual).unwrap_err();
        assert!(err.contains("event 0 diverges"), "{err}");
    }

    #[test]
    fn test_diff_rejects_length_mismatch() {
        let e = vec![ExecutionEvent::WorkflowStarted {
            intent: "x".into(),
            input_tokens: 0,
        }];
        let a: Vec<ExecutionEvent> = vec![];
        assert!(diff_traces(&e, &a).unwrap_err().contains("count diverges"));
    }
}
