//! Phase 1 exit criteria (v0.13.1 charter) — Law 1/2/4/5 end to end.
//!
//! Law 5: `POST /v1/executions` compiles through the shared `build_compiler`
//! pipeline, so input violating constraints, budget, or policy is rejected
//! (no execution occurs).

use axum::routing::post;
use fusion_router::compiler::{build_compiler, Compiler};
use fusion_router::events::BroadcastEventBus;
use fusion_router::executor::DefaultExecutor;
use fusion_router::policy::ast::PolicyParser;
use fusion_router::providers::ChatProvider;
use fusion_router::resource::DefaultResourceManager;
use fusion_router::server::execution::{build_execution_plane, execute_workflow_handler};
use fusion_router::types::{ModelCatalog, Quota};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

struct EchoProvider;

#[async_trait::async_trait]
impl ChatProvider for EchoProvider {
    fn name(&self) -> &str {
        "echo"
    }

    async fn chat_completion(
        &self,
        _request: &fusion_router::types::ChatCompletionRequest,
    ) -> anyhow::Result<fusion_router::types::ChatCompletionResponse> {
        anyhow::bail!("not implemented")
    }
}

struct WorkingEchoProvider;

#[async_trait::async_trait]
impl ChatProvider for WorkingEchoProvider {
    fn name(&self) -> &str {
        "echo"
    }

    async fn chat_completion(
        &self,
        _request: &fusion_router::types::ChatCompletionRequest,
    ) -> anyhow::Result<fusion_router::types::ChatCompletionResponse> {
        Ok(fusion_router::types::ChatCompletionResponse {
            id: "echo-1".into(),
            object: "chat.completion".into(),
            created: 0,
            model: "echo".into(),
            choices: vec![fusion_router::types::Choice {
                index: 0,
                message: fusion_router::types::ChatMessage {
                    role: "assistant".into(),
                    content: "hello from echo".into(),
                },
                finish_reason: "stop".into(),
            }],
            native_tool_calls: None,
            usage: Some(fusion_router::types::Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
        })
    }
}

fn generous_quota() -> Quota {
    Quota {
        max_daily_cost: fusion_router::types::NanoUSD::from_nanos(1_000_000_000_000),
        max_daily_tokens: 1_000_000_000,
        max_concurrent: 100,
        provider_limits: HashMap::new(),
    }
}

fn small_quota() -> Quota {
    Quota {
        max_daily_cost: fusion_router::types::NanoUSD::ONE_DOLLAR,
        max_daily_tokens: 1_000,
        max_concurrent: 1,
        provider_limits: HashMap::new(),
    }
}

/// Policy IR denying `shell.exec` (ADR-034 / Law 2).
fn deny_shell_policy() -> fusion_router::policy::ir::PolicyIR {
    let json_raw = r#"{
        "version": "1.0",
        "declarations": [
            {
                "name": "deny-shell",
                "priority": 100,
                "match_target": "shell.exec",
                "effect": "deny",
                "conditions": {},
                "annotations": {}
            }
        ]
    }"#;
    let (ast, _) = PolicyParser::parse_json(json_raw).unwrap();
    fusion_router::policy::ir::PolicyIR::from_ast(&ast).unwrap()
}

async fn spawn_plane(
    quota: Quota,
    policy: Option<fusion_router::policy::ir::PolicyIR>,
) -> String {
    let event_bus = Arc::new(BroadcastEventBus::new(64));
    let executor = Arc::new(DefaultExecutor::new(Arc::new(EchoProvider), HashMap::new()));
    let compiler: Arc<dyn Compiler> = Arc::new(build_compiler(
        ModelCatalog::default(),
        Arc::new(DefaultResourceManager::new(quota)),
        policy,
    ));
    let plane = build_execution_plane(event_bus, executor, compiler);
    let app = axum::Router::new()
        .route("/v1/executions", post(execute_workflow_handler))
        .with_state(plane);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr.to_string()
}

async fn spawn_plane_with(executor: Arc<dyn fusion_router::executor::Executor>) -> String {
    let event_bus = Arc::new(BroadcastEventBus::new(64));
    let compiler: Arc<dyn Compiler> = Arc::new(build_compiler(
        ModelCatalog::default(),
        Arc::new(DefaultResourceManager::new(generous_quota())),
        None,
    ));
    let plane = build_execution_plane(event_bus, executor, compiler);
    let app = axum::Router::new()
        .route("/v1/executions", post(execute_workflow_handler))
        .with_state(plane);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr.to_string()
}

async fn post_workflow(addr: &str, workflow: Value) -> (u16, Value) {
    let body = json!({
        "trigger_name": "api-test",
        "kind": "webhook",
        "intent": "Quality",
        "payload": {},
        "workflow": workflow
    });
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{addr}/v1/executions"))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap())
}

fn node_json(id: &str, capability: &str) -> Value {
    json!({
        "id": id,
        "kind": "Generate",
        "strategy": "Single",
        "model": "echo",
        "config": { "capability": capability }
    })
}

fn clean_workflow(plan_id: &str, node_id: &str, capability: &str) -> Value {
    json!({
        "plan_id": plan_id,
        "nodes": [node_json(node_id, capability)],
        "edges": [],
        "metadata": {
            "policy_applied": [],
            "estimated_cost": 100_000_000,
            "estimated_tokens": 100
        }
    })
}

#[tokio::test]
async fn law5_execution_plane_rejects_deny_policy_target_end_to_end() {
    let addr = spawn_plane(generous_quota(), Some(deny_shell_policy())).await;

    let (status, body) = post_workflow(
        &addr,
        clean_workflow(
            &uuid::Uuid::new_v4().to_string(),
            &uuid::Uuid::new_v4().to_string(),
            "shell.exec",
        ),
    )
    .await;

    assert_eq!(status, 400, "deny-listed target must be rejected: {body}");
    assert!(
        body["error"].as_str().unwrap().contains("deny"),
        "error must identify the deny policy: {body}"
    );
}

#[tokio::test]
async fn law5_non_denied_target_passes_deny_compiler() {
    let addr = spawn_plane(generous_quota(), Some(deny_shell_policy())).await;

    let (status, body) = post_workflow(
        &addr,
        clean_workflow(
            &uuid::Uuid::new_v4().to_string(),
            &uuid::Uuid::new_v4().to_string(),
            "web.fetch",
        ),
    )
    .await;

    assert_eq!(status, 400, "EchoProvider aborts execution, but compile must succeed: {body}");
    assert!(
        body["error"].as_str().unwrap().contains("Provider error"),
        "workflow must fail in the executor, not the compiler: {body}"
    );
}

#[tokio::test]
async fn law5_execution_plane_rejects_dangling_edge() {
    let addr = spawn_plane(generous_quota(), None).await;

    let node_id = uuid::Uuid::new_v4().to_string();
    let workflow = json!({
        "plan_id": uuid::Uuid::new_v4().to_string(),
        "nodes": [node_json(&node_id, "web.fetch")],
        "edges": [{
            "from": uuid::Uuid::new_v4().to_string(),
            "to": node_id,
            "condition": null
        }],
        "metadata": {
            "policy_applied": [],
            "estimated_cost": 100_000_000,
            "estimated_tokens": 100
        }
    });

    let (status, body) = post_workflow(&addr, workflow).await;
    assert_eq!(status, 400, "dangling edge must be rejected: {body}");
    assert!(
        body["error"].as_str().unwrap().contains("unknown source node"),
        "error must identify the dangling edge: {body}"
    );
}

#[tokio::test]
async fn law5_execution_plane_rejects_over_budget() {
    let addr = spawn_plane(small_quota(), None).await;

    let mut workflow = clean_workflow(
        &uuid::Uuid::new_v4().to_string(),
        &uuid::Uuid::new_v4().to_string(),
        "web.fetch",
    );
    workflow["metadata"]["estimated_cost"] = json!(10_000_000_000_000u64);

    let (status, body) = post_workflow(&addr, workflow).await;
    assert_eq!(status, 400, "over-budget workflow must be rejected: {body}");
    assert!(
        body["error"].as_str().unwrap().contains("Budget exceeded"),
        "error must identify the budget violation: {body}"
    );
}

#[tokio::test]
async fn law5_execution_plane_uses_full_passes() {
    let executor = Arc::new(DefaultExecutor::new(
        Arc::new(WorkingEchoProvider),
        HashMap::new(),
    ));
    let addr = spawn_plane_with(executor).await;

    let (status, body) = post_workflow(
        &addr,
        clean_workflow(
            &uuid::Uuid::new_v4().to_string(),
            &uuid::Uuid::new_v4().to_string(),
            "web.fetch",
        ),
    )
    .await;

    assert_eq!(status, 200, "clean workflow must compile and run: {body}");
    assert_eq!(body["status"], "completed", "execution must complete: {body}");
}

#[tokio::test]
async fn law1_build_compiler_produces_mandatory_passes() {
    let compiler = build_compiler(
        ModelCatalog::default(),
        Arc::new(DefaultResourceManager::new(generous_quota())),
        Some(deny_shell_policy()),
    );
    assert!(!compiler.passes.is_empty(), "Law 1: pass list must never be empty");
    let names: Vec<&str> = compiler.passes.iter().map(|p| p.name()).collect();
    for mandatory in [
        "constraint_validation",
        "control_flow_validation",
        "dead_node_elimination",
        "model_resolution",
        "budget_optimisation",
        "policy",
    ] {
        assert!(names.contains(&mandatory), "missing mandatory pass {mandatory}: {names:?}");
    }
}

/// Law 6 (ADR-035): a default install fails closed — every insecure
/// combination is rejected by release-mode validation, and `--unsafe-dev`
/// (`AppConfig::unsafe_dev`) is the only escape hatch.
#[test]
fn law6_release_fails_closed() {
    let default = fusion_router::config::AppConfig::load("config/default.yaml")
        .expect("config/default.yaml must parse");

    assert_eq!(default.server.host, "127.0.0.1", "default bind must be loopback");
    assert!(default.auth.enabled, "auth must be enabled by default");
    assert!(default.rate_limiting.enabled, "rate limiting must be enabled by default");
    assert!(default.server.cors.allowed_origins.is_empty(), "CORS must be same-origin by default");
    assert!(default.tools.allowed_shell_commands.is_empty(), "shell tools must be disabled by default");
    assert!(!default.tools.enable_http_tool, "HTTP tool must be disabled by default");

    // Default install has no API keys configured: boot must be refused
    // (unreachable without authentication) in any profile.
    assert!(
        default.validate().is_err(),
        "default install with empty api_keys must not boot"
    );
    let errors = default.validate_with_profile(true).unwrap_err();
    assert!(
        errors.iter().any(|e| e.field == "auth.api_keys"),
        "must flag missing api_keys: {errors:?}"
    );

    // Every insecure combination is rejected in release mode without
    // --unsafe-dev, and accepted with it.
    let mut insecure = default.clone();
    insecure.auth.enabled = false;
    insecure.rate_limiting.enabled = false;
    insecure.server.cors.allowed_origins = vec!["*".into()];
    insecure.tools.allowed_shell_commands = vec!["cat".into()];
    insecure.tools.enable_http_tool = true;

    let errors = insecure.validate_with_profile(true).unwrap_err();
    for field in [
        "auth.enabled",
        "rate_limiting.enabled",
        "server.cors.allowed_origins",
        "tools.allowed_shell_commands",
        "tools.enable_http_tool",
    ] {
        assert!(
            errors.iter().any(|e| e.field == field),
            "release mode must flag insecure '{field}': {errors:?}"
        );
    }

    insecure.unsafe_dev = true;
    assert!(
        insecure.validate_with_profile(true).is_ok(),
        "--unsafe-dev must be the escape hatch for insecure configuration"
    );
}

/// Law 7 (ADR-037): tool execution is fed ONLY from provider-native
/// `tool_calls` under a per-request allowlist. Model output containing a
/// free-form tool JSON object is returned as TEXT and never executed, and
/// native calls outside the allowlist are never executed either.
#[tokio::test]
async fn law7_no_freeform_tool_parsing() {
use fusion_router::executor::{DefaultExecutor, Executor};
    use fusion_router::providers::ChatProvider;
    use fusion_router::strategies::single::SingleStrategy;
    use fusion_router::tools::builtin::CalculatorTool;
    use fusion_router::tools::ToolRegistry;
    use fusion_router::types::{
        ChatCompletionRequest, ChatCompletionResponse, ChatMessage, Choice, ExecutionNode,
        ExecutionNodeKind, RetryPolicy, StrategyKind, ToolCall, Usage,
    };
    use std::collections::HashMap;
    use uuid::Uuid;

    struct ToolProvider {
        content: String,
        tool_calls: Option<Vec<ToolCall>>,
    }

    #[async_trait::async_trait]
    impl ChatProvider for ToolProvider {
        fn name(&self) -> &str {
            "tool-provider"
        }

        async fn chat_completion(
            &self,
            _request: &ChatCompletionRequest,
        ) -> anyhow::Result<ChatCompletionResponse> {
            Ok(ChatCompletionResponse {
                id: "t".into(),
                object: "chat.completion".into(),
                created: 0,
                model: "t".into(),
                choices: vec![Choice {
                    index: 0,
                    message: ChatMessage {
                        role: "assistant".into(),
                        content: self.content.clone(),
                    },
                    finish_reason: "stop".into(),
                }],
                usage: Some(Usage {
                    prompt_tokens: 1,
                    completion_tokens: 1,
                    total_tokens: 2,
                }),
                native_tool_calls: self.tool_calls.clone(),
            })
        }
    }

    fn registry() -> Arc<ToolRegistry> {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(CalculatorTool));
        Arc::new(reg)
    }

    fn node(config: serde_json::Value) -> ExecutionNode {
        ExecutionNode {
            id: Uuid::new_v4(),
            kind: ExecutionNodeKind::LLMGenerate,
            strategy: StrategyKind::Single,
            model: "t".into(),
            retry_policy: RetryPolicy {
                max_retries: 0,
                backoff_ms: 0,
            },
            fallback: None,
            config: serde_json::from_value(config).unwrap_or_default(),
            subgraph: None,
        }
    }

    // 1. Free-form tool JSON in model output: returned as text, never run.
    let tool_json = r#"{"tool": "calculator", "args": {"expression": "2+2"}}"#;
    let executor = Arc::new(
        DefaultExecutor::new(
            Arc::new(ToolProvider {
                content: tool_json.to_string(),
                tool_calls: None,
            }),
            HashMap::new(),
        )
        .with_tool_registry(registry())
        .with_allow_auto_exec(true),
    );
    let result = executor
        .execute_node(
            &node(serde_json::json!({ "tool_allowlist": ["calculator"] })),
            &fusion_router::types::NodeExecContext::default(),
        )
        .await;
    assert_eq!(result.state, fusion_router::types::NodeState::Succeeded);
    let output = result.output.expect("output must be present");
    assert_eq!(
        output,
        serde_json::Value::String(tool_json.to_string()),
        "tool-shaped JSON in content must be returned as text"
    );
    assert!(
        !output.to_string().contains("\"result\""),
        "the calculator must never have run from free-form JSON"
    );

    // 2. Native tool_calls: only allowlisted tools execute.
    let mut strategies: HashMap<StrategyKind, Box<dyn fusion_router::strategies::Strategy + Send + Sync>> =
        HashMap::new();
    strategies.insert(StrategyKind::Single, Box::new(SingleStrategy));
    let executor = Arc::new(
        DefaultExecutor::new(
            Arc::new(ToolProvider {
                content: String::new(),
                tool_calls: Some(vec![ToolCall {
                    id: "c1".into(),
                    name: "calculator".into(),
                    arguments: serde_json::json!({"expression": "2+2"}),
                }]),
            }),
            strategies,
        )
        .with_tool_registry(registry())
        .with_allow_auto_exec(true),
    );
    let result = executor
        .execute_node(
            &node(serde_json::json!({ "tool_allowlist": ["calculator"] })),
            &fusion_router::types::NodeExecContext::default(),
        )
        .await;
    assert_eq!(result.state, fusion_router::types::NodeState::Succeeded);
    let output = result.output.expect("tool call results must be produced");
    assert_eq!(output["tool_calls"][0]["executed"], true);
    assert_eq!(output["tool_calls"][0]["result"]["result"], 4.0);

    // 3. Native tool_calls for a NON-allowlisted tool: never executed.
    let executor = Arc::new(
        DefaultExecutor::new(
            Arc::new(ToolProvider {
                content: String::new(),
                tool_calls: Some(vec![ToolCall {
                    id: "s1".into(),
                    name: "search".into(),
                    arguments: serde_json::json!({"query": "x"}),
                }]),
            }),
            HashMap::new(),
        )
        .with_tool_registry(registry())
        .with_allow_auto_exec(true),
    );
    let result = executor
        .execute_node(
            &node(serde_json::json!({ "tool_allowlist": ["calculator"] })),
            &fusion_router::types::NodeExecContext::default(),
        )
        .await;
    assert_eq!(result.state, fusion_router::types::NodeState::Succeeded);
    let output = result.output.expect("tool call results must be produced");
    assert_eq!(output["tool_calls"][0]["executed"], false);
    assert!(
        output["tool_calls"][0]["reason"].as_str().unwrap_or("").contains("allowlist"),
        "non-allowlisted calls must be surfaced as text with a reason"
    );
}
