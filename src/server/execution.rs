//! Execution plane: compiles a submitted workflow, runs it through the
//! executor, and streams lifecycle events onto the event bus.
//!
//! This is the production wiring that makes the trigger/session/events
//! subsystems reachable from the server binary (`src/main.rs`).

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::compiler::Compiler;
use crate::events::payload::ExecutionEvent;
use crate::events::{BroadcastEventBus, EventBus, ExecutionEventEnvelope};
use crate::executor::Executor;
use crate::lifecycle::LifecycleManager;
use crate::scheduler::{default::DefaultScheduler, Scheduler};
use crate::session::store::InMemorySessionStore;
use crate::types::{ExecutionGraph, NodeState, ReservationId, WorkflowIR};

/// HTTP request body for `POST /v1/executions`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteWorkflowRequest {
    pub trigger_name: String,
    pub kind: String,
    pub intent: String,
    pub payload: Value,
    pub workflow: WorkflowIR,
}

/// Orchestrates a single workflow execution end-to-end.
pub struct ExecutionPlane {
    bus: Arc<BroadcastEventBus>,
    compiler: Arc<dyn Compiler>,
    scheduler: Arc<dyn Scheduler>,
    executor: Arc<dyn Executor>,
    lifecycle: Arc<LifecycleManager>,
}

impl ExecutionPlane {
    pub fn new(
        bus: Arc<BroadcastEventBus>,
        compiler: Arc<dyn Compiler>,
        scheduler: Arc<dyn Scheduler>,
        executor: Arc<dyn Executor>,
        lifecycle: Arc<LifecycleManager>,
    ) -> Self {
        Self {
            bus,
            compiler,
            scheduler,
            executor,
            lifecycle,
        }
    }

    async fn emit(
        &self,
        seq: &mut u64,
        workflow_id: &str,
        execution_id: &str,
        event: ExecutionEvent,
    ) {
        *seq += 1;
        let envelope =
            ExecutionEventEnvelope::new(workflow_id, execution_id, None, *seq, None, event);
        if let Err(e) = self.bus.publish(envelope).await {
            tracing::warn!(error = %e, "event bus publish failed; event lost");
        }
    }

    fn topological_order(graph: &ExecutionGraph) -> Vec<usize> {
        let mut in_degree = vec![0usize; graph.nodes.len()];
        let mut adjacency: HashMap<usize, Vec<usize>> = HashMap::new();
        let index_of: HashMap<uuid::Uuid, usize> = graph
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id, i))
            .collect();
        for edge in &graph.edges {
            if let (Some(&from), Some(&to)) = (index_of.get(&edge.from), index_of.get(&edge.to)) {
                in_degree[to] += 1;
                adjacency.entry(from).or_default().push(to);
            }
        }

        let mut ready: Vec<usize> = (0..graph.nodes.len())
            .filter(|&i| in_degree[i] == 0)
            .collect();
        let mut order = Vec::with_capacity(graph.nodes.len());
        while let Some(node) = ready.pop() {
            order.push(node);
            if let Some(nexts) = adjacency.get(&node) {
                for &next in nexts {
                    in_degree[next] -= 1;
                    if in_degree[next] == 0 {
                        ready.push(next);
                    }
                }
            }
        }
        if order.len() == graph.nodes.len() {
            order
        } else {
            (0..graph.nodes.len()).collect()
        }
    }

    /// Runs the submitted workflow and streams events onto the bus.
    pub async fn execute(&self, request: ExecuteWorkflowRequest) -> Result<Value, String> {
        let started_at = std::time::Instant::now();
        let execution_id = uuid::Uuid::new_v4().to_string();
        let workflow_id = request.workflow.plan_id.to_string();
        let session = self
            .lifecycle
            .create_session(&request.trigger_name, request.workflow.plan_id)
            .await
            .map_err(|e| format!("session creation failed: {e}"))?;
        let session_id = session.session_id.to_string();

        let mut seq: u64 = 0;
        self.emit(
            &mut seq,
            &workflow_id,
            &execution_id,
            ExecutionEvent::WorkflowStarted {
                intent: request.intent.clone(),
                input_tokens: 0,
            },
        )
        .await;

        let graph = self
            .compiler
            .compile(request.workflow.clone())
            .await
            .map_err(|e| format!("compilation failed: {e}"))?;

        self.emit(
            &mut seq,
            &workflow_id,
            &execution_id,
            ExecutionEvent::WorkflowCompiled {
                node_count: graph.nodes.len(),
                edge_count: graph.edges.len(),
                primitive_graph_hash: graph.primitive_graph_hash,
            },
        )
        .await;

        // Phase 6.4 (swap 8): the hand-rolled topological loop is gone. The
        // graph runs through the src Scheduler (which delegates to
        // `fusion_scheduler`), so control flow, retry/fallback and parallelism
        // behave exactly like the chat path. Event emission replays the
        // outcome in deterministic topological order afterwards; per-node
        // latency/token telemetry is not recoverable from `ExecutionOutcome`,
        // so `NodeFinished` carries zeros (6.6 debt note).
        let mut instance = self
            .scheduler
            .schedule(graph.clone(), ReservationId(uuid::Uuid::new_v4()));
        let result = self
            .scheduler
            .run_with_cancellation(
                &mut instance,
                self.executor.as_ref(),
                &tokio_util::sync::CancellationToken::new(),
            )
            .await
            .map_err(|e| format!("scheduling failed: {e}"))?;

        let order = Self::topological_order(&graph);
        let mut outputs: HashMap<String, Value> = HashMap::new();

        if !result.success {
            let failed = order.iter().find_map(|&idx| {
                let node = &graph.nodes[idx];
                match instance.node_states.get(&node.id) {
                    Some(NodeState::Failed(message)) => {
                        Some((node.id.to_string(), message.clone()))
                    }
                    _ => None,
                }
            });
            let (node_id, error) = failed.unwrap_or_else(|| {
                (
                    "unknown".to_string(),
                    "execution failed without a failed node".to_string(),
                )
            });
            self.emit(
                &mut seq,
                &workflow_id,
                &execution_id,
                ExecutionEvent::NodeFailed {
                    node_id: node_id.clone(),
                    error: error.clone(),
                    attempt: 0,
                },
            )
            .await;
            self.emit(
                &mut seq,
                &workflow_id,
                &execution_id,
                ExecutionEvent::WorkflowFailed {
                    error: error.clone(),
                    failed_node_id: Some(node_id.clone()),
                },
            )
            .await;
            return Err(format!("node {} failed: {}", node_id, error));
        }

        for &idx in &order {
            let node = &graph.nodes[idx];
            let node_id = node.id.to_string();

            let dependencies: Vec<String> = graph
                .edges
                .iter()
                .filter(|e| e.to == node.id)
                .map(|e| e.from.to_string())
                .collect();
            self.emit(
                &mut seq,
                &workflow_id,
                &execution_id,
                ExecutionEvent::NodeScheduled {
                    node_id: node_id.clone(),
                    node_kind: format!("{:?}", node.kind),
                    dependencies,
                },
            )
            .await;

            self.emit(
                &mut seq,
                &workflow_id,
                &execution_id,
                ExecutionEvent::NodeStarted {
                    node_id: node_id.clone(),
                    target_model: if node.model.is_empty() {
                        None
                    } else {
                        Some(node.model.clone())
                    },
                },
            )
            .await;

            match instance.node_states.get(&node.id) {
                Some(NodeState::Succeeded { .. }) => {
                    let output = instance.outputs.get(&node.id).cloned().unwrap_or(json!({}));
                    self.emit(
                        &mut seq,
                        &workflow_id,
                        &execution_id,
                        ExecutionEvent::NodeFinished {
                            node_id: node_id.clone(),
                            duration_ms: 0,
                            prompt_tokens: 0,
                            completion_tokens: 0,
                        },
                    )
                    .await;
                    outputs.insert(node_id, output);
                }
                other => {
                    let error = format!("node {} ended in unexpected state {other:?}", node_id);
                    self.emit(
                        &mut seq,
                        &workflow_id,
                        &execution_id,
                        ExecutionEvent::WorkflowFailed {
                            error: error.clone(),
                            failed_node_id: Some(node_id.clone()),
                        },
                    )
                    .await;
                    return Err(error);
                }
            }
        }

        self.emit(
            &mut seq,
            &workflow_id,
            &execution_id,
            ExecutionEvent::WorkflowCompleted {
                total_duration_ms: started_at.elapsed().as_millis() as u64,
                total_cost: result.total_cost,
            },
        )
        .await;

        Ok(json!({
            "execution_id": execution_id,
            "session_id": session_id,
            "workflow_id": workflow_id,
            "status": "completed",
            "events_emitted": seq,
            "node_outputs": outputs,
        }))
    }
}

/// Returns the production execution plane with an in-memory session store.
///
/// Law 5 / ADR-034: the plane must receive a compiler built by
/// `build_compiler` — the same mandatory pass pipeline as every other
/// execution endpoint. An empty pass list is never accepted here.
pub fn build_execution_plane(
    bus: Arc<BroadcastEventBus>,
    executor: Arc<dyn Executor>,
    compiler: Arc<dyn Compiler>,
) -> Arc<ExecutionPlane> {
    let lifecycle = Arc::new(LifecycleManager::new(Arc::new(InMemorySessionStore::new())));
    Arc::new(ExecutionPlane::new(
        bus,
        compiler,
        Arc::new(DefaultScheduler::new(4)),
        executor,
        lifecycle,
    ))
}

/// HTTP handler for `POST /v1/executions`.
pub async fn execute_workflow_handler(
    axum::extract::State(plane): axum::extract::State<Arc<ExecutionPlane>>,
    axum::Json(request): axum::Json<ExecuteWorkflowRequest>,
) -> Result<axum::Json<Value>, (axum::http::StatusCode, axum::Json<Value>)> {
    match plane.execute(request).await {
        Ok(result) => Ok(axum::Json(result)),
        Err(error) => Err((
            axum::http::StatusCode::BAD_REQUEST,
            axum::Json(json!({ "error": error })),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        ChatCompletionResponse, ChatMessage, ExecutionEdge, ExecutionNode, ExecutionNodeKind,
        IRMetadata, IRNode, IRNodeKind, StrategyKind,
    };
    use async_trait::async_trait;

    struct EchoProvider;

    #[async_trait]
    impl crate::providers::ChatProvider for EchoProvider {
        fn name(&self) -> &str {
            "echo"
        }

        async fn chat_completion(
            &self,
            _request: &crate::types::ChatCompletionRequest,
        ) -> anyhow::Result<ChatCompletionResponse> {
            Ok(ChatCompletionResponse {
                id: "echo-1".into(),
                object: "chat.completion".into(),
                created: 0,
                model: "echo".into(),
                choices: vec![crate::types::Choice {
                    index: 0,
                    message: ChatMessage {
                        role: "assistant".into(),
                        content: "hello from echo".into(),
                    },
                    finish_reason: "stop".into(),
                }],
                native_tool_calls: None,
                usage: Some(crate::types::Usage {
                    prompt_tokens: 10,
                    completion_tokens: 5,
                    total_tokens: 15,
                }),
            })
        }
    }

    struct FailingProvider;

    #[async_trait]
    impl crate::providers::ChatProvider for FailingProvider {
        fn name(&self) -> &str {
            "failing"
        }

        async fn chat_completion(
            &self,
            _request: &crate::types::ChatCompletionRequest,
        ) -> anyhow::Result<ChatCompletionResponse> {
            anyhow::bail!("provider exploded")
        }
    }

    fn test_workflow() -> WorkflowIR {
        WorkflowIR {
            plan_id: uuid::Uuid::new_v4(),
            nodes: vec![IRNode {
                id: uuid::Uuid::new_v4(),
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: Some("echo".into()),
                config: HashMap::new(),
            }],
            edges: vec![],
            metadata: IRMetadata {
                policy_version: 0,
                policy_applied: vec![],
                estimated_cost: crate::types::NanoUSD::from_nanos(100_000_000),
                estimated_tokens: 100,
            },
        }
    }

    /// Law 5: test planes compile through the same `build_compiler` factory.
    fn test_plane_compiler() -> Arc<dyn Compiler> {
        Arc::new(crate::compiler::build_compiler(
            crate::types::ModelCatalog::default(),
            Arc::new(crate::resource::DefaultResourceManager::new(
                crate::types::Quota {
                    max_daily_cost: crate::types::NanoUSD::from_nanos(1_000_000_000_000),
                    max_daily_tokens: 1_000_000_000,
                    max_concurrent: 100,
                    provider_limits: std::collections::HashMap::new(),
                },
            )),
            None,
        ))
    }

    #[tokio::test]
    async fn test_execute_workflow_success_emits_events_and_outputs() {
        let bus = Arc::new(BroadcastEventBus::new(64));
        let executor = Arc::new(crate::executor::DefaultExecutor::new(
            Arc::new(EchoProvider),
            HashMap::new(),
        ));
        let plane = build_execution_plane(bus.clone(), executor, test_plane_compiler());
        let mut bus_rx = bus.subscribe();

        let request = ExecuteWorkflowRequest {
            trigger_name: "api-test".into(),
            kind: "event_bus".into(),
            intent: "Quality".into(),
            payload: json!({}),
            workflow: test_workflow(),
        };

        let result = plane.execute(request).await.unwrap();
        assert_eq!(result["status"], "completed");
        assert!(result["session_id"].is_string());
        assert!(result["events_emitted"].as_u64().unwrap() >= 4);

        let first = tokio::time::timeout(std::time::Duration::from_secs(2), bus_rx.recv())
            .await
            .expect("first event must be published")
            .unwrap();
        match first.payload {
            ExecutionEvent::WorkflowStarted { intent, .. } => assert_eq!(intent, "Quality"),
            other => panic!("expected WorkflowStarted, got {:?}", other),
        }

        let mut saw_completed = false;
        for _ in 0..10 {
            match tokio::time::timeout(std::time::Duration::from_secs(2), bus_rx.recv()).await {
                Ok(Ok(envelope)) => {
                    if matches!(envelope.payload, ExecutionEvent::WorkflowCompleted { .. }) {
                        saw_completed = true;
                        break;
                    }
                }
                _ => break,
            }
        }
        assert!(
            saw_completed,
            "WorkflowCompleted must be emitted on success"
        );
    }

    #[tokio::test]
    async fn test_execute_workflow_provider_failure_fails_execution() {
        let bus = Arc::new(BroadcastEventBus::new(64));
        let executor = Arc::new(crate::executor::DefaultExecutor::new(
            Arc::new(FailingProvider),
            HashMap::new(),
        ));
        let plane = build_execution_plane(bus, executor, test_plane_compiler());

        let request = ExecuteWorkflowRequest {
            trigger_name: "api-test".into(),
            kind: "event_bus".into(),
            intent: "Quality".into(),
            payload: json!({}),
            workflow: test_workflow(),
        };

        let result = plane.execute(request).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("provider exploded"));
    }

    #[tokio::test]
    async fn test_execute_workflow_route_returns_200() {
        use axum::routing::post;

        let bus = Arc::new(BroadcastEventBus::new(64));
        let executor = Arc::new(crate::executor::DefaultExecutor::new(
            Arc::new(EchoProvider),
            HashMap::new(),
        ));
        let plane = build_execution_plane(bus, executor, test_plane_compiler());
        let app = axum::Router::new()
            .route("/v1/executions", post(execute_workflow_handler))
            .with_state(plane);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let body = json!({
            "trigger_name": "api-test",
            "kind": "webhook",
            "intent": "Quality",
            "payload": {},
            "workflow": {
                "plan_id": uuid::Uuid::new_v4().to_string(),
                "nodes": [{
                    "id": uuid::Uuid::new_v4().to_string(),
                    "kind": "Generate",
                    "strategy": "Single",
                    "model": "echo",
                    "config": {}
                }],
                "edges": [],
                "metadata": {
                    "policy_applied": [],
                    "estimated_cost": 100000000,
                    "estimated_tokens": 100
                }
            }
        });

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("http://{}/v1/executions", addr))
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200);
        let result: Value = resp.json().await.unwrap();
        assert_eq!(result["status"], "completed");
        assert!(result["session_id"].is_string());
    }

    #[test]
    fn test_topological_order_respects_edges() {
        let first_id = uuid::Uuid::new_v4();
        let second_id = uuid::Uuid::new_v4();
        let graph = ExecutionGraph {
            graph_id: uuid::Uuid::new_v4(),
            nodes: vec![
                ExecutionNode {
                    id: first_id,
                    kind: ExecutionNodeKind::LLMGenerate,
                    strategy: StrategyKind::Single,
                    model: "m".into(),
                    retry_policy: crate::types::RetryPolicy {
                        max_retries: 0,
                        backoff_ms: 0,
                    },
                    fallback: None,
                    config: HashMap::new(),
                    subgraph: None,
                },
                ExecutionNode {
                    id: second_id,
                    kind: ExecutionNodeKind::LLMJudge,
                    strategy: StrategyKind::Single,
                    model: "m".into(),
                    retry_policy: crate::types::RetryPolicy {
                        max_retries: 0,
                        backoff_ms: 0,
                    },
                    fallback: None,
                    config: HashMap::new(),
                    subgraph: None,
                },
            ],
            edges: vec![ExecutionEdge {
                from: first_id,
                to: second_id,
                condition: None,
            }],
            metadata: crate::types::GraphMetadata {
                policy_version: 0,
                estimated_cost: crate::types::NanoUSD::ZERO,
                estimated_tokens: 0,
                max_depth: 1,
                node_count: 2,
            },
            primitive_graph_hash: 0,
            total_tokens: 0,
            total_cost: crate::types::NanoUSD::ZERO,
        };

        let order = ExecutionPlane::topological_order(&graph);
        assert_eq!(
            order,
            vec![0, 1],
            "producer must be scheduled before consumer"
        );
    }
}
