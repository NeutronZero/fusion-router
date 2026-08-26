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

/// Classification of execution-plane failures. `detail` never reaches the
/// client; it is logged server-side only. Clients receive
/// [`ExecutionError::status_code`] + [`ExecutionError::public_message`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionErrorKind {
    /// Malformed/invalid workflow specification (client payload problem).
    Validation,
    /// Live policy configuration rejected the workflow.
    Policy,
    /// Workflow failed to compile.
    Compilation,
    /// Scheduler could not run the graph.
    Scheduling,
    /// An upstream provider/node execution failed.
    Upstream,
    /// Internal infrastructure failure (session store, etc.).
    Internal,
}

#[derive(Debug)]
pub struct ExecutionError {
    kind: ExecutionErrorKind,
    detail: String,
}

impl ExecutionError {
    pub fn validation(detail: impl Into<String>) -> Self {
        Self {
            kind: ExecutionErrorKind::Validation,
            detail: detail.into(),
        }
    }
    pub fn policy(detail: impl Into<String>) -> Self {
        Self {
            kind: ExecutionErrorKind::Policy,
            detail: detail.into(),
        }
    }
    pub fn compilation(detail: impl Into<String>) -> Self {
        Self {
            kind: ExecutionErrorKind::Compilation,
            detail: detail.into(),
        }
    }
    pub fn scheduling(detail: impl Into<String>) -> Self {
        Self {
            kind: ExecutionErrorKind::Scheduling,
            detail: detail.into(),
        }
    }
    pub fn upstream(detail: impl Into<String>) -> Self {
        Self {
            kind: ExecutionErrorKind::Upstream,
            detail: detail.into(),
        }
    }
    pub fn internal(detail: impl Into<String>) -> Self {
        Self {
            kind: ExecutionErrorKind::Internal,
            detail: detail.into(),
        }
    }

    pub fn kind(&self) -> ExecutionErrorKind {
        self.kind
    }

    /// Full internal detail — server logs only.
    pub fn detail(&self) -> &str {
        &self.detail
    }

    pub fn status_code(&self) -> axum::http::StatusCode {
        use axum::http::StatusCode;
        match self.kind {
            // Client payload problem.
            ExecutionErrorKind::Validation => StatusCode::BAD_REQUEST,
            // Upstream provider failures are a gateway condition.
            ExecutionErrorKind::Upstream => StatusCode::BAD_GATEWAY,
            // Compilation/policy/scheduling/infrastructure faults are ours.
            ExecutionErrorKind::Policy
            | ExecutionErrorKind::Compilation
            | ExecutionErrorKind::Scheduling
            | ExecutionErrorKind::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Client-facing message. Validation failures describe the client's own
    /// payload problem (they are the client's fault and actionable — Law 5
    /// invariants require naming the violated rule). All other kinds are
    /// opaque: internal/upstream detail must never reach clients.
    pub fn public_message(&self) -> String {
        match self.kind {
            ExecutionErrorKind::Validation => {
                let sanitized: String = self
                    .detail
                    .chars()
                    .map(|c| if c.is_control() { ' ' } else { c })
                    .collect();
                let mut clipped: String = sanitized.chars().take(300).collect();
                if sanitized.chars().count() > 300 {
                    clipped.push('…');
                }
                format!("invalid workflow specification: {clipped}")
            }
            ExecutionErrorKind::Policy => "workflow rejected by policy configuration".into(),
            ExecutionErrorKind::Compilation => "workflow failed to compile".into(),
            ExecutionErrorKind::Scheduling => "workflow scheduling failed".into(),
            ExecutionErrorKind::Upstream => "upstream node execution failed".into(),
            ExecutionErrorKind::Internal => "internal execution error".into(),
        }
    }
}

/// Orchestrates a single workflow execution end-to-end.
pub struct ExecutionPlane {
    bus: Arc<BroadcastEventBus>,
    /// Builds a compiler per execution with the live policy snapshot attached,
    /// so deny/approval rules created at runtime are enforced immediately.
    compiler_factory: CompilerFactory,
    policy_registry: Arc<crate::policy::PolicyRegistry>,
    scheduler: Arc<dyn Scheduler>,
    executor: Arc<dyn Executor>,
    lifecycle: Arc<LifecycleManager>,
}

/// Factory producing the mandatory pass pipeline, optionally with the policy
/// pass appended (Law 2 / Law 5: deny ⇒ compile error).
pub type CompilerFactory =
    Arc<dyn Fn(Option<crate::policy::ir::PolicyIR>) -> Arc<dyn Compiler> + Send + Sync>;

impl ExecutionPlane {
    pub fn new(
        bus: Arc<BroadcastEventBus>,
        compiler_factory: CompilerFactory,
        policy_registry: Arc<crate::policy::PolicyRegistry>,
        scheduler: Arc<dyn Scheduler>,
        executor: Arc<dyn Executor>,
        lifecycle: Arc<LifecycleManager>,
    ) -> Self {
        Self {
            bus,
            compiler_factory,
            policy_registry,
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

    /// Runs the submitted workflow with typed, classifiable failures.
    pub async fn execute_typed(
        &self,
        request: ExecuteWorkflowRequest,
    ) -> Result<Value, ExecutionError> {
        let started_at = std::time::Instant::now();
        let execution_id = uuid::Uuid::new_v4().to_string();
        let workflow_id = request.workflow.plan_id.to_string();
        let session = self
            .lifecycle
            .create_session(&request.trigger_name, request.workflow.plan_id)
            .await
            .map_err(|e| ExecutionError::internal(format!("session creation failed: {e}")))?;
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

        let policy_ir = self
            .policy_registry
            .policy_ir()
            .map_err(|e| ExecutionError::policy(format!("policy configuration rejected: {e}")))?;
        let compiler = (self.compiler_factory)(policy_ir);
        let graph = compiler
            .compile(request.workflow.clone())
            .await
            .map_err(|e| match &e {
                // Malformed workflow structure is the caller's payload problem;
                // anything else is a server-side compilation fault.
                crate::types::CompilerError::ValidationError { pass, message, .. } => {
                    ExecutionError::validation(format!(
                        "workflow validation failed in pass '{pass}': {message}"
                    ))
                }
                _ => ExecutionError::compilation(format!("compilation failed: {e}")),
            })?;

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
            .map_err(|e| ExecutionError::scheduling(format!("scheduling failed: {e}")))?;

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
            return Err(ExecutionError::upstream(format!(
                "node {node_id} failed: {error}"
            )));
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
                Some(NodeState::Succeeded) => {
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
                    return Err(ExecutionError::upstream(error));
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

    /// Legacy stringly-typed entry point retained for existing callers
    /// (snapshot/replay tooling). New code should use [`Self::execute_typed`].
    pub async fn execute(&self, request: ExecuteWorkflowRequest) -> Result<Value, String> {
        self.execute_typed(request)
            .await
            .map_err(|e| e.detail().to_string())
    }
}

/// Scheduler concurrency used when the caller does not plumb a configured
/// value. Production wiring MUST use [`build_execution_plane_with_concurrency`]
/// with `resources.max_concurrent_nodes`.
pub const DEFAULT_EXECUTION_CONCURRENCY: u32 = 4;

/// Returns the production execution plane with an in-memory session store.
///
/// Law 5 / ADR-034: the plane compiles through a `build_compiler` factory —
/// the same mandatory pass pipeline as every other execution endpoint, with
/// the live policy snapshot attached per execution. An empty pass list is
/// never accepted here.
///
/// Kept for existing callers (tests, snapshot tooling); delegates to
/// [`build_execution_plane_with_concurrency`] with the default concurrency.
pub fn build_execution_plane(
    bus: Arc<BroadcastEventBus>,
    executor: Arc<dyn Executor>,
    model_catalog: crate::types::ModelCatalog,
    resource_manager: Arc<dyn crate::resource::ResourceManager>,
    policy_registry: Arc<crate::policy::PolicyRegistry>,
) -> Arc<ExecutionPlane> {
    build_execution_plane_with_concurrency(
        bus,
        executor,
        model_catalog,
        resource_manager,
        policy_registry,
        DEFAULT_EXECUTION_CONCURRENCY,
    )
}

/// Like [`build_execution_plane`], but honors the configured node-concurrency
/// ceiling (`resources.max_concurrent_nodes`) instead of a hardcoded value.
pub fn build_execution_plane_with_concurrency(
    bus: Arc<BroadcastEventBus>,
    executor: Arc<dyn Executor>,
    model_catalog: crate::types::ModelCatalog,
    resource_manager: Arc<dyn crate::resource::ResourceManager>,
    policy_registry: Arc<crate::policy::PolicyRegistry>,
    max_concurrent_nodes: u32,
) -> Arc<ExecutionPlane> {
    let lifecycle = Arc::new(LifecycleManager::new(Arc::new(InMemorySessionStore::new())));
    let compiler_factory: CompilerFactory = Arc::new(move |policy_ir| {
        Arc::new(crate::compiler::build_compiler(
            model_catalog.clone(),
            resource_manager.clone(),
            policy_ir,
        ))
    });
    let scheduler_concurrency = (max_concurrent_nodes.max(1)) as usize;
    tracing::debug!(
        scheduler_concurrency,
        "execution plane built with configured node concurrency"
    );
    Arc::new(ExecutionPlane::new(
        bus,
        compiler_factory,
        policy_registry,
        Arc::new(DefaultScheduler::new(scheduler_concurrency)),
        executor,
        lifecycle,
    ))
}

/// HTTP handler for `POST /v1/executions`.
///
/// Error mapping (details logged server-side only):
/// - malformed/invalid workflow payload → 400 with opaque validation message;
/// - compilation / policy / scheduling / internal failures → 500 with opaque
///   messages;
/// - upstream provider/node failures → 502 with an opaque message.
pub async fn execute_workflow_handler(
    axum::extract::State(plane): axum::extract::State<Arc<ExecutionPlane>>,
    axum::Json(request): axum::Json<ExecuteWorkflowRequest>,
) -> Result<axum::Json<Value>, (axum::http::StatusCode, axum::Json<Value>)> {
    let metrics = crate::telemetry::metrics::FusionMetrics::instance();
    metrics.requests_total.inc();
    let start = std::time::Instant::now();
    let result = plane.execute_typed(request).await;
    metrics
        .request_duration_seconds
        .with_label_values(&["/v1/executions"])
        .observe(start.elapsed().as_secs_f64());
    match result {
        Ok(result) => Ok(axum::Json(result)),
        Err(error) => {
            metrics.errors_total.inc();
            // Internal detail stays in logs; the response body is opaque.
            tracing::warn!(
                status = %error.status_code(),
                error = ?error,
                "workflow execution failed"
            );
            Err((
                error.status_code(),
                axum::Json(json!({ "error": error.public_message() })),
            ))
        }
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

    fn quota_for_tests() -> crate::types::Quota {
        crate::types::Quota {
            max_daily_cost: crate::types::NanoUSD::from_nanos(1_000_000_000_000),
            max_daily_tokens: 1_000_000_000,
            max_concurrent: 100,
            provider_limits: std::collections::HashMap::new(),
        }
    }

    #[tokio::test]
    async fn test_execute_workflow_success_emits_events_and_outputs() {
        let bus = Arc::new(BroadcastEventBus::new(64));
        let executor = Arc::new(crate::executor::DefaultExecutor::new(
            Arc::new(EchoProvider),
            HashMap::new(),
        ));
        let plane = build_execution_plane(
            bus.clone(),
            executor,
            crate::types::ModelCatalog::default(),
            Arc::new(crate::resource::DefaultResourceManager::new(
                quota_for_tests(),
            )),
            Arc::new(crate::policy::PolicyRegistry::new()),
        );
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
        let plane = build_execution_plane(
            bus,
            executor,
            crate::types::ModelCatalog::default(),
            Arc::new(crate::resource::DefaultResourceManager::new(
                quota_for_tests(),
            )),
            Arc::new(crate::policy::PolicyRegistry::new()),
        );

        let request = ExecuteWorkflowRequest {
            trigger_name: "api-test".into(),
            kind: "event_bus".into(),
            intent: "Quality".into(),
            payload: json!({}),
            workflow: test_workflow(),
        };

        let result = plane.execute_typed(request).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.detail().contains("provider exploded"),
            "detail must carry the provider error for server logs"
        );
        assert_eq!(err.kind(), ExecutionErrorKind::Upstream);
        assert_eq!(err.status_code(), axum::http::StatusCode::BAD_GATEWAY);
    }

    #[test]
    fn test_execution_error_classification_and_opaque_messages() {
        use axum::http::StatusCode;

        let cases: Vec<(ExecutionError, StatusCode)> = vec![
            (
                ExecutionError::validation("workflow node 3 references unknown field 'foo'"),
                StatusCode::BAD_REQUEST,
            ),
            (
                ExecutionError::policy("policy configuration rejected: rule 7 malformed"),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (
                ExecutionError::compilation("compilation failed: pass explosion panicked"),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (
                ExecutionError::scheduling("scheduling failed: lease lost"),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (
                ExecutionError::upstream("node abc failed: provider exploded"),
                StatusCode::BAD_GATEWAY,
            ),
            (
                ExecutionError::internal("session creation failed: db locked"),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        ];

        for (err, expected_status) in cases {
            assert_eq!(err.status_code(), expected_status, "kind {:?}", err.kind());
            let public = err.public_message();
            assert!(
                !public.contains(&err.detail().to_lowercase()),
                "public message must not embed internal detail"
            );
            assert!(!public.is_empty());
        }
    }

    #[test]
    fn test_validation_error_public_message_is_opaque() {
        let err = ExecutionError::validation(
            "unknown strategy kind 'QuantumConsensus' on node 5 with secret sk-abcdef",
        );
        let public = err.public_message();
        assert_eq!(public, "invalid workflow specification");
        assert!(!public.contains("sk-"), "must not leak credential material");
        assert!(!public.contains("node 5"));
    }

    #[tokio::test]
    async fn test_execute_workflow_route_returns_200() {
        use axum::routing::post;

        let bus = Arc::new(BroadcastEventBus::new(64));
        let executor = Arc::new(crate::executor::DefaultExecutor::new(
            Arc::new(EchoProvider),
            HashMap::new(),
        ));
        let plane = build_execution_plane(
            bus,
            executor,
            crate::types::ModelCatalog::default(),
            Arc::new(crate::resource::DefaultResourceManager::new(
                quota_for_tests(),
            )),
            Arc::new(crate::policy::PolicyRegistry::new()),
        );
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
