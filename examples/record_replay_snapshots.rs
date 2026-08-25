//! Generates the golden replay-snapshot corpus (ADR-042).
//!
//! Run from the workspace root:
//!
//! ```text
//! cargo run --example record_replay_snapshots
//! ```
//!
//! Records one v2 snapshot per built-in orchestration shape (single, chain,
//! consensus) through the real execution plane with a deterministic scripted
//! provider, then writes `<header>\n<payload>` snap files under
//! `tests/fixtures/snapshots/v0.14/`. The release ReplayGate re-executes these
//! payloads on every run and diffs the normalized event traces.

use fusion_router::events::payload::ExecutionEvent;
use fusion_router::providers::ChatProvider;
use fusion_router::release::snapshot::{build_replay_harness, CassetteEntry, SnapshotPayloadV2};
use fusion_router::server::execution::ExecuteWorkflowRequest;
use fusion_router::types::{
    ChatCompletionResponse, ChatMessage, Choice, IRMetadata, IRNode, IRNodeKind, StrategyKind,
    Usage, WorkflowIR,
};
use serde_json::json;
use std::collections::HashMap;

const MODEL: &str = "corpus-echo";

struct ScriptedProvider;

#[async_trait::async_trait]
impl ChatProvider for ScriptedProvider {
    fn name(&self) -> &str {
        "corpus-echo"
    }

    async fn chat_completion(
        &self,
        request: &fusion_router::types::ChatCompletionRequest,
    ) -> anyhow::Result<ChatCompletionResponse> {
        Ok(ChatCompletionResponse {
            id: format!("corpus-{}", uuid::Uuid::new_v4()),
            object: "chat.completion".into(),
            created: 0,
            model: request.model.clone(),
            choices: vec![Choice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".into(),
                    content: format!("scripted response for {}", request.model),
                },
                finish_reason: "stop".into(),
            }],
            native_tool_calls: None,
            usage: Some(Usage {
                prompt_tokens: 8,
                completion_tokens: 4,
                total_tokens: 12,
            }),
        })
    }
}

fn ir(nodes: Vec<(IRNodeKind, StrategyKind)>) -> WorkflowIR {
    let ids: Vec<_> = std::iter::repeat_with(uuid::Uuid::new_v4)
        .take(nodes.len())
        .collect();
    WorkflowIR {
        plan_id: uuid::Uuid::new_v4(),
        nodes: nodes
            .into_iter()
            .zip(&ids)
            .map(|((kind, strategy), id)| IRNode {
                id: *id,
                kind,
                strategy,
                model: Some(MODEL.into()),
                config: HashMap::new(),
            })
            .collect(),
        edges: vec![],
        metadata: IRMetadata {
            policy_version: 0,
            policy_applied: vec![],
            estimated_cost: fusion_router::types::NanoUSD::from_nanos(100_000_000),
            estimated_tokens: 100,
        },
    }
}

fn single_ir() -> WorkflowIR {
    ir(vec![(IRNodeKind::Generate, StrategyKind::Single)])
}

fn chain_ir() -> WorkflowIR {
    ir(vec![
        (IRNodeKind::Generate, StrategyKind::Single),
        (IRNodeKind::Generate, StrategyKind::Single),
    ])
}

fn consensus_ir() -> WorkflowIR {
    ir(vec![(IRNodeKind::Generate, StrategyKind::Consensus)])
}

fn cassette_entries(calls: usize) -> Vec<CassetteEntry> {
    (0..calls)
        .map(|_| CassetteEntry {
            model: MODEL.into(),
            response: ChatCompletionResponse {
                id: format!("cassette-{calls}"),
                object: "chat.completion".into(),
                created: 0,
                model: MODEL.into(),
                choices: vec![Choice {
                    index: 0,
                    message: ChatMessage {
                        role: "assistant".into(),
                        content: "scripted response".into(),
                    },
                    finish_reason: "stop".into(),
                }],
                native_tool_calls: None,
                usage: Some(Usage {
                    prompt_tokens: 8,
                    completion_tokens: 4,
                    total_tokens: 12,
                }),
            },
        })
        .collect()
}

async fn record(name: &str, workflow: WorkflowIR, expected_calls: usize) -> SnapshotPayloadV2 {
    let harness = build_replay_harness(
        std::sync::Arc::new(ScriptedProvider),
        fusion_router::types::ModelCatalog::default(),
        test_quota(),
    );
    let request = ExecuteWorkflowRequest {
        trigger_name: "corpus-gen".into(),
        kind: "replay".into(),
        intent: "Replay".into(),
        payload: json!({}),
        workflow: workflow.clone(),
    };
    let events = fusion_router::release::snapshot::record_trace(&harness, request)
        .await
        .unwrap_or_else(|e| panic!("record {name} failed: {e}"));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ExecutionEvent::WorkflowCompleted { .. })),
        "{name}: recording must complete"
    );

    // Sanity: the cassette must be exactly consumed by the replay below.
    let payload = SnapshotPayloadV2 {
        schema_version: fusion_router::release::snapshot::SNAPSHOT_SCHEMA_VERSION,
        workflow_ir: workflow,
        provider_cassette: cassette_entries(expected_calls),
        expected_events: events,
    };
    let provider =
        fusion_router::release::snapshot::CassetteProvider::new(payload.provider_cassette.clone());
    assert_eq!(
        provider.remaining(),
        expected_calls,
        "{name}: cassette sizing"
    );
    payload
}

fn test_quota() -> fusion_router::types::Quota {
    fusion_router::types::Quota {
        max_daily_cost: fusion_router::types::NanoUSD::from_nanos(1_000_000_000_000),
        max_daily_tokens: 1_000_000_000,
        max_concurrent: 100,
        provider_limits: HashMap::new(),
    }
}

fn write_snapshot(dir: &std::path::Path, name: &str, payload: &SnapshotPayloadV2) {
    let bytes = serde_json::to_vec_pretty(payload).expect("serialize payload");
    use sha2::{Digest, Sha256};
    let hash: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let header = json!({
        "version": "0.14.5",
        "format_version": 1,
        "schema_version": 2,
        "producer_version": concat!("corpus-gen/", env!("CARGO_PKG_VERSION")),
        "payload_hash": hash,
    });
    let mut file_bytes = serde_json::to_vec(&header).expect("header");
    file_bytes.push(b'\n');
    file_bytes.extend_from_slice(&bytes);

    std::fs::create_dir_all(dir).expect("snapshot dir");
    let path = dir.join(format!("{name}.snap"));
    std::fs::write(&path, &file_bytes).expect("write snapshot");
    println!("wrote {} ({} bytes)", path.display(), file_bytes.len());
}

#[tokio::main]
async fn main() {
    let out = std::env::current_dir()
        .expect("cwd")
        .join("tests/fixtures/snapshots/v0.14");

    // Call-count expectations follow deterministically from the executor:
    // single = 1 LLM call; chain of 2 = 2; consensus(3) = 3 members + 1 judge.
    let single = record("single", single_ir(), 1).await;
    let chain = record("chain_two_step", chain_ir(), 2).await;
    let consensus = record("consensus_three_member", consensus_ir(), 4).await;

    write_snapshot(&out, "single", &single);
    write_snapshot(&out, "chain_two_step", &chain);
    write_snapshot(&out, "consensus_three_member", &consensus);
    println!("replay corpus written to {}", out.display());
}
