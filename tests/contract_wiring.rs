//! End-to-end wiring test for the v0.13 contract stack.
//!
//! Verifies the full chain now has real adapters connecting it to the live
//! v0.12 path:
//!
//! `NormalizedIntent` → `WorkflowIR` (fusion_ir) → `types::WorkflowIR`
//! → `build_compiler` → `ExecutionGraph` → `ExecutionAbi` → `LocalEri` → run.
//!
//! Nothing in this test touches the network; the provider is a mock.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

use fusion_router::abi::from_graph::abi_from_graph;
use fusion_router::abi::EXECUTION_ABI_VERSION;
use fusion_router::compiler::{build_compiler, Compiler};
use fusion_router::eri::local_runtime::LocalEri;
use fusion_router::eri::{ExecutionRuntimeInterface, ExecutionState};
use fusion_router::intent::{Budget, Constraints, IntentKind, NormalizedIntent};
use fusion_router::ir::adapter::workflow_to_types;
use fusion_router::providers::ChatProvider;
use fusion_router::strategies::single::SingleStrategy;
use fusion_router::target::ExecutionTarget;
use fusion_router::types::{
    ChatCompletionRequest, ChatCompletionResponse, Choice, ChatMessage, ModelCatalog, Quota, Usage,
};
use uuid::Uuid;

fn canonical_json<T: serde::Serialize>(value: &T) -> String {
    fn normalize(value: serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(map) => {
                let mut ordered = std::collections::BTreeMap::new();
                for (key, value) in map {
                    ordered.insert(key, normalize(value));
                }
                serde_json::Value::Object(ordered.into_iter().collect())
            }
            serde_json::Value::Array(values) => {
                serde_json::Value::Array(values.into_iter().map(normalize).collect())
            }
            value => value,
        }
    }
    serde_json::to_string(&normalize(serde_json::to_value(value).unwrap())).unwrap()
}

struct MockChatProvider;

#[async_trait]
impl ChatProvider for MockChatProvider {
    async fn chat_completion(
        &self,
        _request: &ChatCompletionRequest,
    ) -> anyhow::Result<ChatCompletionResponse> {
        Ok(ChatCompletionResponse {
            id: "mock".into(),
            object: "chat.completion".into(),
            created: 0,
            model: "mock-model".into(),
            choices: vec![Choice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".into(),
                    content: "wired end to end".into(),
                },
                finish_reason: "stop".into(),
            }],
            native_tool_calls: None,
            usage: Some(Usage {
                prompt_tokens: 5,
                completion_tokens: 10,
                total_tokens: 15,
            }),
        })
    }

    fn name(&self) -> &str {
        "mock"
    }
}

fn permissive_quota() -> Quota {
    Quota {
        max_daily_cost: fusion_router::types::NanoUSD::from_nanos(1_000_000_000_000),
        max_daily_tokens: 1_000_000_000,
        max_concurrent: 100,
        provider_limits: HashMap::new(),
    }
}

fn intent() -> NormalizedIntent {
    NormalizedIntent {
        intent_id: Uuid::new_v4(),
        goal: "build a parser".into(),
        kind: IntentKind::Code,
        constraints: Constraints {
            max_latency_ms: Some(60_000),
            max_cost: None,
            max_tokens: Some(10_000),
            min_confidence: None,
        },
        budget: Budget {
            max_cost: Some(fusion_core::NanoUSD::from_nanos(1_000_000_000)),
            max_tokens: Some(10_000),
            max_execution_ms: None,
        },
        session_id: None,
    }
}

#[tokio::test]
async fn intent_to_abi_to_runtime_is_wired() {
    // 1. v0.13 intent → contract WorkflowIR (fusion_ir).
    let workflow = fusion_router::intent::lowering::intent_to_workflow(&intent()).unwrap();
    assert_eq!(workflow.nodes().len(), 2);

    // 2. Contract IR → live v0.12 IR via the adapter.
    let types_ir = workflow_to_types(&workflow).unwrap();
    assert_eq!(types_ir.nodes.len(), 2);

    // 3. Live compile path.
    let compiler = build_compiler(
        ModelCatalog::default(),
        Arc::new(fusion_router::resource::DefaultResourceManager::new(
            permissive_quota(),
        )),
        None,
    );
    let graph = compiler.compile(types_ir).await.unwrap();
    assert_eq!(graph.nodes.len(), 2);

    // 4. ABI generator (contract 3) — provider-free.
    let abi = abi_from_graph(&graph);
    assert_eq!(abi.version, EXECUTION_ABI_VERSION);
    assert!(abi.nodes.iter().all(|n| !n.capability.contains("mock")));
    assert_eq!(abi.nodes.len(), graph.nodes.len());
    assert_eq!(abi.edges.len(), graph.edges.len());

    // 5. ERI (contract 5) executes the ABI against the live engine.
    let eri = LocalEri::new(
        Arc::new(MockChatProvider),
        "mock-model",
        HashMap::from([(
            fusion_router::types::StrategyKind::Single,
            Box::new(SingleStrategy) as Box<dyn fusion_router::strategies::Strategy + Send + Sync>,
        )]),
    );
    let result = eri
        .execute(&abi, &ExecutionTarget::default())
        .await
        .unwrap();
    assert_eq!(result.state, ExecutionState::Succeeded);
    assert_eq!(eri.name(), "fusion-local");
    assert!(result.metrics["total_tokens"] > 0.0);
    assert_eq!(
        eri.state(&result.execution_id).await.unwrap(),
        ExecutionState::Succeeded
    );
}

#[tokio::test]
async fn intent_to_runtime_is_deterministic() {
    // byte-for-byte determinism (canonical IR and ExecutionGraph)
    let workflow = fusion_router::intent::lowering::intent_to_workflow(&intent()).unwrap();
    let types_ir = workflow_to_types(&workflow).unwrap();
    let ir_bytes_a = canonical_json(&types_ir);
    let ir_bytes_b = canonical_json(&types_ir.clone());
    let compiler = build_compiler(
        ModelCatalog::default(),
        Arc::new(fusion_router::resource::DefaultResourceManager::new(
            permissive_quota(),
        )),
        None,
    );
    let graph_a = compiler.compile(types_ir.clone()).await.unwrap();
    let graph_b = compiler.compile(types_ir).await.unwrap();
    let abi_a = abi_from_graph(&graph_a);
    let abi_b = abi_from_graph(&graph_b);
    assert_eq!(ir_bytes_a, ir_bytes_b);
    assert_eq!(canonical_json(&graph_a), canonical_json(&graph_b));
    assert_eq!(canonical_json(&abi_a), canonical_json(&abi_b));
}
