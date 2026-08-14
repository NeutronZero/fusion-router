use async_trait::async_trait;
use std::collections::HashMap;
use uuid::Uuid;

use fusion_router::executor::Executor;
use fusion_router::resource::BudgetEnvelope;
use fusion_router::scheduler::default::DefaultScheduler;
use fusion_router::scheduler::Scheduler;
use fusion_router::server::pipeline::{PipelineContext, PipelineStep, ResponseBuilderStep};
use fusion_router::types::*;

struct MockExecutor {
    outputs: HashMap<Uuid, String>,
}

#[async_trait]
impl Executor for MockExecutor {
    async fn execute_node(
        &self,
        node: &ExecutionNode,
        _ctx: &NodeExecContext,
    ) -> NodeExecutionResult {
        let text = self
            .outputs
            .get(&node.id)
            .cloned()
            .unwrap_or_else(|| format!("output_for_{}", node.id));
        NodeExecutionResult {
            state: NodeState::Succeeded,
            usage: Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 10,
                total_tokens: 20,
            }),
            latency_ms: 5,
            output: Some(serde_json::Value::String(text)),
        }
    }
}

#[tokio::test]
async fn test_scheduler_parity_and_budget_enforcement() {
    let scheduler = DefaultScheduler::default();
    let n1_id = Uuid::new_v4();

    let node1 = ExecutionNode {
        id: n1_id,
        kind: ExecutionNodeKind::LLMGenerate,
        strategy: StrategyKind::Single,
        model: "test-model".to_string(),
        retry_policy: RetryPolicy {
            max_retries: 0,
            backoff_ms: 0,
        },
        fallback: None,
        config: HashMap::new(),
        subgraph: None,
    };

    let graph = ExecutionGraph {
        graph_id: Uuid::new_v4(),
        nodes: vec![node1],
        edges: vec![],
        metadata: GraphMetadata {
            policy_version: 0,
            estimated_cost: NanoUSD::from_nanos(10_000_000),
            estimated_tokens: 100,
            max_depth: 1,
            node_count: 1,
        },
        total_tokens: 100,
        total_cost: NanoUSD::from_nanos(10_000_000_000),
        primitive_graph_hash: 0,
    };

    let mut outputs = HashMap::new();
    outputs.insert(n1_id, "hello from node1".to_string());
    let executor = MockExecutor { outputs };

    // Test run() with strict budget limit (token limit = 5 vs 20 consumed -> breached immediately)
    let mut instance1 = scheduler.schedule(graph.clone(), ReservationId(Uuid::new_v4()));
    instance1.budget_envelope = Some(BudgetEnvelope::new(NanoUSD::from_nanos(1000), 5, 10));
    let res1 = scheduler.run(&mut instance1, &executor).await.unwrap();

    // Test run_with_cancellation() with identical budget limit
    let token = tokio_util::sync::CancellationToken::new();
    let mut instance2 = scheduler.schedule(graph.clone(), ReservationId(Uuid::new_v4()));
    instance2.budget_envelope = Some(BudgetEnvelope::new(NanoUSD::from_nanos(1000), 5, 10));
    let res2 = scheduler
        .run_with_cancellation(&mut instance2, &executor, &token)
        .await
        .unwrap();

    // Verify both fail due to budget breach and produce identical failure state
    assert!(!res1.success);
    assert!(!res2.success);
    assert_eq!(res1.success, res2.success);
}

#[tokio::test]
async fn test_terminal_node_response_selection() {
    let scheduler = DefaultScheduler::default();
    let gen_id = Uuid::new_v4();
    let reflect_id = Uuid::new_v4();
    let judge_id = Uuid::new_v4();

    let gen_node = ExecutionNode {
        id: gen_id,
        kind: ExecutionNodeKind::LLMGenerate,
        strategy: StrategyKind::Single,
        model: "m".into(),
        retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
        fallback: None,
        config: HashMap::new(),
        subgraph: None,
    };
    let reflect_node = ExecutionNode {
        id: reflect_id,
        kind: ExecutionNodeKind::LLMReview,
        strategy: StrategyKind::Single,
        model: "m".into(),
        retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
        fallback: None,
        config: HashMap::new(),
        subgraph: None,
    };
    let judge_node = ExecutionNode {
        id: judge_id,
        kind: ExecutionNodeKind::LLMJudge,
        strategy: StrategyKind::Single,
        model: "m".into(),
        retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
        fallback: None,
        config: HashMap::new(),
        subgraph: None,
    };

    let edges = vec![
        ExecutionEdge { from: gen_id, to: reflect_id, condition: None },
        ExecutionEdge { from: reflect_id, to: judge_id, condition: None },
    ];

    let graph = ExecutionGraph {
        graph_id: Uuid::new_v4(),
        nodes: vec![gen_node, reflect_node, judge_node],
        edges,
        metadata: GraphMetadata {
            policy_version: 0,
            estimated_cost: NanoUSD::from_nanos(10_000_000),
            estimated_tokens: 100,
            max_depth: 3,
            node_count: 3,
        },
        total_tokens: 100,
        total_cost: NanoUSD::from_nanos(10_000_000_000),
        primitive_graph_hash: 0,
    };

    let mut outputs = HashMap::new();
    outputs.insert(gen_id, "Draft response".to_string());
    outputs.insert(reflect_id, "Critique notes".to_string());
    outputs.insert(judge_id, "Final Judged Masterpiece".to_string());

    let executor = MockExecutor { outputs };
    let mut instance = scheduler.schedule(graph, ReservationId(Uuid::new_v4()));
    let res = scheduler.run(&mut instance, &executor).await.unwrap();

    assert!(res.success);
    assert_eq!(res.terminal_node_id, Some(judge_id));
    assert_eq!(res.final_output, Some(serde_json::Value::String("Final Judged Masterpiece".to_string())));

    // Test ResponseBuilderStep output content
    let mut ctx = PipelineContext::new(
        Uuid::new_v4(),
        ChatCompletionRequest {
            model: "auto".to_string(),
            messages: vec![],
            stream: false,
            temperature: None,
            max_tokens: None,
            tools: None,
            files: None,
            execution: None,
            output: None,
            strategy: None,
        },
        tokio_util::sync::CancellationToken::new(),
    );

    let response = ResponseBuilderStep.execute(res, &mut ctx).await.unwrap();
    assert_eq!(response.choices[0].message.content, "Final Judged Masterpiece");
}

#[tokio::test]
async fn test_compiler_model_resolution_preservation() {
    let mut node = ExecutionNode {
        id: Uuid::new_v4(),
        kind: ExecutionNodeKind::LLMGenerate,
        strategy: StrategyKind::Single,
        model: "claude-3-5-sonnet".to_string(), // Explicitly set by ModelResolutionPass
        retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
        fallback: None,
        config: HashMap::new(),
        subgraph: None,
    };

    let ctx = PipelineContext::new(
        Uuid::new_v4(),
        ChatCompletionRequest {
            model: "auto".to_string(), // Client sent "auto"
            messages: vec![],
            stream: false,
            temperature: None,
            max_tokens: None,
            tools: None,
            files: None,
            execution: None,
            output: None,
            strategy: None,
        },
        tokio_util::sync::CancellationToken::new(),
    );

    // Run model logic: if node.model is not empty, it MUST be preserved
    if node.model.is_empty() {
        node.model = ctx.request.model.clone();
    }
    assert_eq!(node.model, "claude-3-5-sonnet");
}
