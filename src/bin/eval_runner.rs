//! Eval harness: fusion-router vs. direct LLM call.
//!
//! Three-way comparison:
//!   A: Direct call to baseline model (no fusion-router)
//!   B: Routed-single (IntentPlanner picks model, StrategyKind::Single)
//!   C: Full fusion-router (routing + strategy selection)
//!
//! Usage:
//!   cargo run --bin eval_runner -- --suite benches/eval_suite/tasks.yaml
//!   cargo run --bin eval_runner -- --suite benches/eval_suite/tasks.yaml --dry-run
//!   cargo run --bin eval_runner -- --suite benches/eval_suite/tasks.yaml --bucket consensus

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use fusion_router::compiler::{build_compiler, Compiler, DefaultCompiler};
use fusion_router::config::AppConfig;
use fusion_router::executor::DefaultExecutor;
use fusion_router::planner::{IntentPlanner, Planner};
use fusion_router::providers::openrouter::OpenRouterProvider;
use fusion_router::providers::ChatProvider;
use fusion_router::resource::DefaultResourceManager;
use fusion_router::scheduler::default::DefaultScheduler;
use fusion_router::scheduler::Scheduler;
use fusion_router::strategies::chain::ChainStrategy;
use fusion_router::strategies::consensus::ConsensusStrategy;
use fusion_router::strategies::debate::DebateStrategy;
use fusion_router::strategies::fusion::FusionStrategy;
use fusion_router::strategies::react::ReActStrategy;
use fusion_router::strategies::reflection::ReflectionStrategy;
use fusion_router::strategies::single::SingleStrategy;
use fusion_router::strategies::Strategy;
use fusion_router::types::{
    ChatCompletionRequest, ChatMessage, ComplexityLevel, ExecutionGraph, ExecutionNodeKind,
    IRMetadata, IRNode, IRNodeKind, Intent, ReservationId, Requirements,
    StrategyKind, WorkflowIR,
};
use fusion_router::types::execution::ExecutionIntent;

// ── CLI ──────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "eval_runner", about = "fusion-router eval harness")]
struct Cli {
    /// Path to the eval suite YAML file.
    #[arg(long, default_value = "benches/eval_suite/tasks.yaml")]
    suite: PathBuf,

    /// Dry run: 1 task per bucket, repeats=2.
    #[arg(long)]
    dry_run: bool,

    /// Run only tasks from a specific bucket.
    #[arg(long)]
    bucket: Option<String>,

    /// Run only a specific task by ID.
    #[arg(long)]
    task: Option<String>,

    /// Override repeat count.
    #[arg(long)]
    repeats: Option<usize>,
}

// ── Eval config (top-level YAML) ────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
struct EvalConfig {
    baseline_model: String,
    fusion_config: String,
    baseline_provider: String,
    repeats: usize,
    #[allow(dead_code)]
    dry_run: bool,
}

// ── Task suite types ────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
struct TaskSuite {
    config: EvalConfig,
    buckets: HashMap<String, BucketDef>,
    tasks: Vec<TaskDef>,
}

#[derive(Debug, Clone, Deserialize)]
struct BucketDef {
    hypothesis: String,
    scoring: String,
}

#[derive(Debug, Clone, Deserialize)]
struct TaskDef {
    id: String,
    bucket: String,
    #[serde(default)]
    prompt: Option<String>,
    scoring: ScoringMethod,
    #[serde(default)]
    ground_truth: Option<GroundTruth>,
    #[serde(default)]
    rubric: Option<RubricDef>,
    #[serde(default)]
    reuse: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ScoringMethod {
    ExactMatch,
    NumericTolerance,
    ContainsCheck,
    UnitTest,
    RegexMatch,
    Rubric,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum GroundTruth {
    Simple(String),
    Struct(GroundTruthStruct),
}

impl GroundTruth {
    fn value(&self) -> Option<String> {
        match self {
            GroundTruth::Simple(s) => Some(s.clone()),
            GroundTruth::Struct(gt) => gt.value.clone(),
        }
    }
    fn tolerance(&self) -> f64 {
        match self {
            GroundTruth::Simple(_) => 0.01,
            GroundTruth::Struct(gt) => gt.tolerance.unwrap_or(0.01),
        }
    }
    fn must_contain(&self) -> Option<&[String]> {
        match self {
            GroundTruth::Simple(_) => None,
            GroundTruth::Struct(gt) => gt.must_contain.as_deref(),
        }
    }
    fn must_not_contain(&self) -> Option<&[String]> {
        match self {
            GroundTruth::Simple(_) => None,
            GroundTruth::Struct(gt) => gt.must_not_contain.as_deref(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct GroundTruthStruct {
    #[serde(default)]
    #[serde(deserialize_with = "de_value_to_string")]
    value: Option<String>,
    #[serde(default)]
    tolerance: Option<f64>,
    #[serde(default)]
    must_contain: Option<Vec<String>>,
    #[serde(default)]
    must_not_contain: Option<Vec<String>>,
}

fn de_value_to_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum FlexibleValue {
        Str(String),
        Num(f64),
        Bool(bool),
        Null,
    }
    match FlexibleValue::deserialize(deserializer)? {
        FlexibleValue::Str(s) => Ok(Some(s)),
        FlexibleValue::Num(n) => Ok(Some(format!("{}", n))),
        FlexibleValue::Bool(b) => Ok(Some(b.to_string())),
        FlexibleValue::Null => Ok(None),
    }
}

#[derive(Debug, Clone, Deserialize)]
struct RubricDef {
    dimensions: Vec<RubricDimension>,
    judge_model: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RubricDimension {
    name: String,
    description: String,
    weight: f64,
}

// ── Run result types ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
struct RunResult {
    task_id: String,
    condition: String,
    run_index: usize,
    output: String,
    quality_score: f64,
    cost_usd: f64,
    latency_ms: u64,
    tokens: u64,
    call_count: u32,
    model_used: String,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct BucketReport {
    bucket: String,
    hypothesis: String,
    condition_a: ConditionStats,
    condition_b: ConditionStats,
    condition_c: ConditionStats,
}

#[derive(Debug, Clone, Serialize)]
struct ConditionStats {
    mean_quality: f64,
    stddev_quality: f64,
    mean_cost_usd: f64,
    mean_latency_ms: f64,
    mean_tokens: f64,
    mean_call_count: f64,
    n: usize,
}

// ── Scoring ─────────────────────────────────────────────────────

fn score_output(task: &TaskDef, output: &str) -> f64 {
    match task.scoring {
        ScoringMethod::ExactMatch => {
            let expected = task
                .ground_truth
                .as_ref()
                .and_then(|gt| gt.value())
                .unwrap_or_default();
            let normalised = output.trim().to_lowercase();
            let expected_normalised = expected.trim().to_lowercase();
            if normalised == expected_normalised {
                1.0
            } else {
                0.0
            }
        }
        ScoringMethod::NumericTolerance => {
            let gt = task.ground_truth.as_ref().expect("numeric_tolerance requires ground_truth");
            let expected_val: f64 = gt
                .value()
                .as_ref()
                .and_then(|v| v.parse().ok())
                .expect("ground_truth.value must be a number");
            let tolerance = gt.tolerance();
            let parsed: Option<f64> = output.trim().parse().ok();
            match parsed {
                Some(v) if (v - expected_val).abs() <= tolerance => 1.0,
                _ => 0.0,
            }
        }
        ScoringMethod::ContainsCheck => {
            let gt = task.ground_truth.as_ref().expect("contains_check requires ground_truth");
            let mut score = 1.0;
            if let Some(must) = gt.must_contain() {
                for s in must {
                    if !output.to_lowercase().contains(&s.to_lowercase()) {
                        score = 0.0;
                    }
                }
            }
            if let Some(must_not) = gt.must_not_contain() {
                for s in must_not {
                    if output.to_lowercase().contains(&s.to_lowercase()) {
                        score = 0.0;
                    }
                }
            }
            score
        }
        ScoringMethod::UnitTest => {
            if output.contains("def ") || output.contains("class ") {
                0.5
            } else {
                0.0
            }
        }
        ScoringMethod::RegexMatch => {
            if output.contains('^') || output.contains('$') || output.contains('\\') {
                1.0
            } else {
                0.5
            }
        }
        ScoringMethod::Rubric => {
            // Stubbed — needs LLM judge in full mode
            0.0
        }
    }
}

fn score_rubric(task: &TaskDef, output: &str, provider: &dyn ChatProvider) -> f64 {
    let rubric = match &task.rubric {
        Some(r) => r,
        None => return 0.0,
    };

    let judge_prompt = format!(
        "You are a strict evaluator. Score the following response on each dimension.\n\n\
         Response to evaluate:\n{}\n\n\
         Dimensions:\n{}\n\n\
         Reply with ONLY a JSON object mapping dimension names to scores (0.0 to 1.0). \
         Example: {{\"dimension_name\": 0.8}}",
        output,
        rubric
            .dimensions
            .iter()
            .map(|d| format!(
                "- {} (weight {:.0}): {}",
                d.name, d.weight, d.description
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );

    let request = ChatCompletionRequest {
        model: rubric.judge_model.clone(),
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: judge_prompt,
        }],
        stream: false,
        temperature: Some(0.0),
        max_tokens: Some(512),
        tools: None,
        files: None,
        execution: None,
        output: None,
        strategy: None,
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    let response = rt.block_on(provider.chat_completion(&request));
    match response {
        Ok(resp) => {
            let text = resp
                .choices
                .first()
                .map(|c| c.message.content.as_str())
                .unwrap_or("");
            parse_rubric_scores(text, &rubric.dimensions)
        }
        Err(_) => 0.0,
    }
}

fn parse_rubric_scores(text: &str, dimensions: &[RubricDimension]) -> f64 {
    // Try to extract JSON from the response
    let json_start = text.find('{');
    let json_end = text.rfind('}');
    if let (Some(start), Some(end)) = (json_start, json_end) {
        let json_str = &text[start..=end];
        if let Ok(scores) = serde_json::from_str::<HashMap<String, f64>>(json_str) {
            let mut weighted_sum = 0.0;
            let mut total_weight = 0.0;
            for dim in dimensions {
                if let Some(score) = scores.get(&dim.name) {
                    weighted_sum += score * dim.weight;
                    total_weight += dim.weight;
                }
            }
            if total_weight > 0.0 {
                return weighted_sum / total_weight;
            }
        }
    }
    0.0
}

// ── Provider setup ──────────────────────────────────────────────

fn build_provider_from_config(config: &AppConfig, provider_name: &str) -> Result<Arc<dyn ChatProvider + Send + Sync>> {
    let provider_cfg = config
        .providers
        .get(provider_name)
        .with_context(|| format!("provider '{}' not found in config", provider_name))?;

    let api_key_env = provider_cfg
        .api_key_env
        .as_deref()
        .with_context(|| format!("provider '{}' has no api_key_env", provider_name))?;

    let api_key = std::env::var(api_key_env)
        .with_context(|| format!("env var '{}' not set (needed for provider '{}')", api_key_env, provider_name))?;

    let provider: Arc<dyn ChatProvider + Send + Sync> = match provider_name {
        "openrouter" => Arc::new(OpenRouterProvider::with_base_url(api_key, provider_cfg.base_url.clone())),
        "zen" => Arc::new(fusion_router::providers::zen::ZenProvider::with_base_url(
            api_key,
            provider_cfg.base_url.clone(),
        )),
        "ollama" => Arc::new(fusion_router::providers::ollama::OllamaProvider::new()),
        _ => {
            return Err(anyhow::anyhow!(
                "unknown provider '{}'. Add a match arm in eval_runner.rs",
                provider_name
            ))
        }
    };

    Ok(provider)
}

// ── Condition A: Direct call ────────────────────────────────────

async fn run_condition_a(
    provider: &dyn ChatProvider,
    task: &TaskDef,
    model: &str,
) -> RunResult {
    let messages = vec![ChatMessage {
        role: "user".to_string(),
        content: task.prompt.clone().unwrap_or_default(),
    }];

    let request = ChatCompletionRequest {
        model: model.to_string(),
        messages,
        stream: false,
        temperature: None,
        max_tokens: None,
        tools: None,
        files: None,
        execution: None,
        output: None,
        strategy: None,
    };

    let start = Instant::now();
    let result = provider.chat_completion(&request).await;
    let latency = start.elapsed().as_millis() as u64;

    match result {
        Ok(resp) => {
            let output = resp
                .choices
                .first()
                .map(|c| c.message.content.clone())
                .unwrap_or_default();
            let usage = resp.usage.as_ref();
            let tokens = usage.map(|u| u.total_tokens as u64).unwrap_or(0);
            let cost = estimate_cost(model, usage);

            RunResult {
                task_id: task.id.clone(),
                condition: "A".to_string(),
                run_index: 0,
                output: output.clone(),
                quality_score: score_output(task, &output),
                cost_usd: cost,
                latency_ms: latency,
                tokens,
                call_count: 1,
                model_used: model.to_string(),
                error: None,
            }
        }
        Err(e) => RunResult {
            task_id: task.id.clone(),
            condition: "A".to_string(),
            run_index: 0,
            output: String::new(),
            quality_score: 0.0,
            cost_usd: 0.0,
            latency_ms: latency,
            tokens: 0,
            call_count: 0,
            model_used: model.to_string(),
            error: Some(e.to_string()),
        },
    }
}

// ── Condition B & C: Pipeline execution ────────────────────────

fn build_single_node_ir(_task: &TaskDef, model: &str) -> WorkflowIR {
    WorkflowIR {
        plan_id: Uuid::new_v4(),
        nodes: vec![IRNode {
            id: Uuid::new_v4(),
            kind: IRNodeKind::Generate,
            strategy: StrategyKind::Single,
            model: Some(model.to_string()),
            config: HashMap::new(),
        }],
        edges: vec![],
        metadata: IRMetadata {
            policy_applied: vec!["eval:single".into()],
            estimated_cost: 0.01,
            estimated_tokens: 1000,
        },
    }
}

fn inject_messages(graph: &mut ExecutionGraph, messages: &[ChatMessage]) {
    let messages_val = serde_json::to_value(messages).unwrap_or_default();
    for node in &mut graph.nodes {
        if matches!(
            node.kind,
            ExecutionNodeKind::LLMGenerate | ExecutionNodeKind::LLMReview | ExecutionNodeKind::LLMJudge
        ) {
            node.config.insert("messages".to_string(), messages_val.clone());
            if let Some(subgraph) = node.subgraph.as_mut() {
                for sub_node in &mut subgraph.nodes {
                    if matches!(
                        sub_node.kind,
                        ExecutionNodeKind::LLMGenerate | ExecutionNodeKind::LLMReview | ExecutionNodeKind::LLMJudge
                    ) {
                        sub_node.config.insert("messages".to_string(), messages_val.clone());
                    }
                }
            }
        }
    }
}

fn task_to_execution_intent(task: &TaskDef) -> ExecutionIntent {
    match task.bucket.as_str() {
        "consensus" => ExecutionIntent::Quality,
        "reflection" => ExecutionIntent::Balanced,
        "debate" => ExecutionIntent::Exhaustive,
        "chain" => ExecutionIntent::Balanced,
        "react" => ExecutionIntent::Balanced,
        "fusion" => ExecutionIntent::Quality,
        "model_routing" => ExecutionIntent::Speed,
        _ => ExecutionIntent::Balanced,
    }
}

fn task_to_intent(task: &TaskDef) -> Intent {
    match task.bucket.as_str() {
        "consensus" => Intent::General,
        "reflection" => Intent::Code,
        "debate" => Intent::Analysis,
        "chain" => Intent::General,
        "react" => Intent::General,
        "fusion" => Intent::Code,
        "model_routing" => Intent::General,
        _ => Intent::General,
    }
}

async fn run_condition_b(
    compiler: &DefaultCompiler,
    scheduler: &DefaultScheduler,
    executor: &DefaultExecutor,
    planner: &IntentPlanner,
    task: &TaskDef,
    baseline_model: &str,
) -> RunResult {
    let intent = task_to_execution_intent(task);
    let requirements = Requirements {
        intent_classification: task_to_intent(task),
        complexity: ComplexityLevel::High,
        has_files: false,
        context_window: 4096,
        original_text: task.prompt.clone().unwrap_or_default(),
        execution_intent: Some(intent),
        output_preferences: None,
        model_requirements: None,
    };

    // Let planner generate the IR (picks model, strategy structure)
    let mut ir = planner.plan(&requirements, &[], None).await;

    // Force all nodes to Single strategy (condition B = routing only, no multi-call)
    for node in &mut ir.nodes {
        node.strategy = StrategyKind::Single;
        // Keep the model the planner selected (routing value)
    }

    // Compile
    let mut graph = match compiler.compile(ir).await {
        Ok(g) => g,
        Err(e) => {
            return RunResult {
                task_id: task.id.clone(),
                condition: "B".to_string(),
                run_index: 0,
                output: String::new(),
                quality_score: 0.0,
                cost_usd: 0.0,
                latency_ms: 0,
                tokens: 0,
                call_count: 0,
                model_used: baseline_model.to_string(),
                error: Some(format!("compile error: {}", e)),
            }
        }
    };

    // Inject task prompt as messages
    let messages = vec![ChatMessage {
        role: "user".to_string(),
        content: task.prompt.clone().unwrap_or_default(),
    }];
    inject_messages(&mut graph, &messages);

    // Schedule and execute
    let reservation = ReservationId(Uuid::new_v4());
    let mut instance = scheduler.schedule(graph, reservation);

    let start = Instant::now();
    let result = scheduler.run(&mut instance, executor).await;
    let latency = start.elapsed().as_millis() as u64;

    match result {
        Ok(exec_result) => {
            let output = exec_result
                .final_output
                .as_ref()
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            // If no final_output, try to collect from node outputs
            let output = if output.is_empty() {
                exec_result
                    .outputs
                    .values()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                output
            };

            let model_used = extract_model_from_graph(&instance.graph);

            RunResult {
                task_id: task.id.clone(),
                condition: "B".to_string(),
                run_index: 0,
                output: output.clone(),
                quality_score: score_output(task, &output),
                cost_usd: exec_result.total_cost,
                latency_ms: latency,
                tokens: exec_result.total_tokens,
                call_count: count_llm_nodes(&instance.graph),
                model_used,
                error: None,
            }
        }
        Err(e) => RunResult {
            task_id: task.id.clone(),
            condition: "B".to_string(),
            run_index: 0,
            output: String::new(),
            quality_score: 0.0,
            cost_usd: 0.0,
            latency_ms: latency,
            tokens: 0,
            call_count: 0,
            model_used: baseline_model.to_string(),
            error: Some(format!("execution error: {}", e)),
        },
    }
}

async fn run_condition_c(
    compiler: &DefaultCompiler,
    scheduler: &DefaultScheduler,
    executor: &DefaultExecutor,
    planner: &IntentPlanner,
    task: &TaskDef,
    baseline_model: &str,
) -> RunResult {
    let intent = task_to_execution_intent(task);
    let requirements = Requirements {
        intent_classification: task_to_intent(task),
        complexity: ComplexityLevel::High,
        has_files: false,
        context_window: 4096,
        original_text: task.prompt.clone().unwrap_or_default(),
        execution_intent: Some(intent),
        output_preferences: None,
        model_requirements: None,
    };

    // Full pipeline: planner picks model AND strategy
    let ir = planner.plan(&requirements, &[], None).await;

    // Compile (strategy expansion happens here)
    let mut graph = match compiler.compile(ir).await {
        Ok(g) => g,
        Err(e) => {
            return RunResult {
                task_id: task.id.clone(),
                condition: "C".to_string(),
                run_index: 0,
                output: String::new(),
                quality_score: 0.0,
                cost_usd: 0.0,
                latency_ms: 0,
                tokens: 0,
                call_count: 0,
                model_used: baseline_model.to_string(),
                error: Some(format!("compile error: {}", e)),
            }
        }
    };

    // Inject task prompt as messages
    let messages = vec![ChatMessage {
        role: "user".to_string(),
        content: task.prompt.clone().unwrap_or_default(),
    }];
    inject_messages(&mut graph, &messages);

    // Schedule and execute
    let reservation = ReservationId(Uuid::new_v4());
    let mut instance = scheduler.schedule(graph, reservation);

    let start = Instant::now();
    let result = scheduler.run(&mut instance, executor).await;
    let latency = start.elapsed().as_millis() as u64;

    match result {
        Ok(exec_result) => {
            let output = exec_result
                .final_output
                .as_ref()
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let output = if output.is_empty() {
                exec_result
                    .outputs
                    .values()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                output
            };

            let model_used = extract_model_from_graph(&instance.graph);

            RunResult {
                task_id: task.id.clone(),
                condition: "C".to_string(),
                run_index: 0,
                output: output.clone(),
                quality_score: score_output(task, &output),
                cost_usd: exec_result.total_cost,
                latency_ms: latency,
                tokens: exec_result.total_tokens,
                call_count: count_llm_nodes(&instance.graph),
                model_used,
                error: None,
            }
        }
        Err(e) => RunResult {
            task_id: task.id.clone(),
            condition: "C".to_string(),
            run_index: 0,
            output: String::new(),
            quality_score: 0.0,
            cost_usd: 0.0,
            latency_ms: latency,
            tokens: 0,
            call_count: 0,
            model_used: baseline_model.to_string(),
            error: Some(format!("execution error: {}", e)),
        },
    }
}

// ── Helpers ─────────────────────────────────────────────────────

fn extract_model_from_graph(graph: &ExecutionGraph) -> String {
    graph
        .nodes
        .first()
        .map(|n| n.model.clone())
        .unwrap_or_default()
}

fn count_llm_nodes(graph: &ExecutionGraph) -> u32 {
    graph
        .nodes
        .iter()
        .filter(|n| {
            matches!(
                n.kind,
                ExecutionNodeKind::LLMGenerate | ExecutionNodeKind::LLMReview | ExecutionNodeKind::LLMJudge
            )
        })
        .count() as u32
}

fn estimate_cost(model: &str, usage: Option<&fusion_router::types::Usage>) -> f64 {
    let usage = match usage {
        Some(u) => u,
        None => return 0.0,
    };
    // Rough cost estimates (USD per 1M tokens)
    let (input_cost, output_cost) = match model {
        m if m.contains("gpt-4o") && !m.contains("mini") => (2.50, 10.0),
        m if m.contains("gpt-4o-mini") => (0.15, 0.60),
        m if m.contains("claude-sonnet") => (3.00, 15.0),
        m if m.contains("claude-opus") => (15.0, 75.0),
        m if m.contains("claude-haiku") => (0.25, 1.25),
        _ => (1.0, 3.0), // conservative default
    };
    (usage.prompt_tokens as f64 / 1_000_000.0) * input_cost
        + (usage.completion_tokens as f64 / 1_000_000.0) * output_cost
}

// ── Report generation ───────────────────────────────────────────

fn compute_condition_stats(results: &[&RunResult]) -> ConditionStats {
    if results.is_empty() {
        return ConditionStats {
            mean_quality: 0.0,
            stddev_quality: 0.0,
            mean_cost_usd: 0.0,
            mean_latency_ms: 0.0,
            mean_tokens: 0.0,
            mean_call_count: 0.0,
            n: 0,
        };
    }

    let n = results.len() as f64;
    let mean_quality = results.iter().map(|r| r.quality_score).sum::<f64>() / n;
    let mean_cost = results.iter().map(|r| r.cost_usd).sum::<f64>() / n;
    let mean_latency = results.iter().map(|r| r.latency_ms as f64).sum::<f64>() / n;
    let mean_tokens = results.iter().map(|r| r.tokens as f64).sum::<f64>() / n;
    let mean_calls = results.iter().map(|r| r.call_count as f64).sum::<f64>() / n;

    let variance = results
        .iter()
        .map(|r| {
            let diff = r.quality_score - mean_quality;
            diff * diff
        })
        .sum::<f64>()
        / n;
    let stddev = variance.sqrt();

    ConditionStats {
        mean_quality,
        stddev_quality: stddev,
        mean_cost_usd: mean_cost,
        mean_latency_ms: mean_latency,
        mean_tokens,
        mean_call_count: mean_calls,
        n: results.len(),
    }
}

fn generate_report(results: &[RunResult], tasks: &[TaskDef]) -> Vec<BucketReport> {
    let mut by_bucket: HashMap<String, Vec<&RunResult>> = HashMap::new();
    for r in results {
        by_bucket
            .entry(r.task_id.clone())
            .or_default()
            .push(r);
    }

    let mut reports = Vec::new();
    let task_map: HashMap<&str, &TaskDef> = tasks.iter().map(|t| (t.id.as_str(), t)).collect();

    // Group by bucket
    let mut bucket_groups: HashMap<String, Vec<&RunResult>> = HashMap::new();
    for r in results {
        if let Some(task) = task_map.get(r.task_id.as_str()) {
            bucket_groups
                .entry(task.bucket.clone())
                .or_default()
                .push(r);
        }
    }

    for (bucket_name, bucket_results) in &bucket_groups {
        let a: Vec<&RunResult> = bucket_results
            .iter()
            .filter(|r| r.condition == "A")
            .copied()
            .collect();
        let b: Vec<&RunResult> = bucket_results
            .iter()
            .filter(|r| r.condition == "B")
            .copied()
            .collect();
        let c: Vec<&RunResult> = bucket_results
            .iter()
            .filter(|r| r.condition == "C")
            .copied()
            .collect();

        let hypothesis = bucket_results
            .first()
            .and_then(|r| task_map.get(r.task_id.as_str()))
            .map(|t| t.bucket.clone())
            .unwrap_or_default();

        reports.push(BucketReport {
            bucket: bucket_name.clone(),
            hypothesis,
            condition_a: compute_condition_stats(&a),
            condition_b: compute_condition_stats(&b),
            condition_c: compute_condition_stats(&c),
        });
    }

    reports.sort_by(|a, b| a.bucket.cmp(&b.bucket));
    reports
}

fn print_report(reports: &[BucketReport]) {
    println!("\n{}", "=".repeat(60));
    println!("  fusion-router eval report");
    println!("{}", "=".repeat(60));

    for report in reports {
        println!("── {} ──", report.bucket);
        println!("  A (naive baseline):    quality={:.2}±{:.2}  cost=${:.4}  latency={:.0}ms  tokens={:.0}  calls={:.1}  n={}",
            report.condition_a.mean_quality, report.condition_a.stddev_quality,
            report.condition_a.mean_cost_usd, report.condition_a.mean_latency_ms,
            report.condition_a.mean_tokens, report.condition_a.mean_call_count, report.condition_a.n);
        println!("  B (routed-single):     quality={:.2}±{:.2}  cost=${:.4}  latency={:.0}ms  tokens={:.0}  calls={:.1}  n={}",
            report.condition_b.mean_quality, report.condition_b.stddev_quality,
            report.condition_b.mean_cost_usd, report.condition_b.mean_latency_ms,
            report.condition_b.mean_tokens, report.condition_b.mean_call_count, report.condition_b.n);
        println!("  C (full fusion-router): quality={:.2}±{:.2}  cost=${:.4}  latency={:.0}ms  tokens={:.0}  calls={:.1}  n={}",
            report.condition_c.mean_quality, report.condition_c.stddev_quality,
            report.condition_c.mean_cost_usd, report.condition_c.mean_latency_ms,
            report.condition_c.mean_tokens, report.condition_c.mean_call_count, report.condition_c.n);

        let a_to_c_quality = report.condition_c.mean_quality - report.condition_a.mean_quality;
        let a_to_c_cost = report.condition_c.mean_cost_usd - report.condition_a.mean_cost_usd;
        println!(
            "  A→C: Δquality={:+.2}  Δcost=${:+.4}",
            a_to_c_quality, a_to_c_cost
        );
        println!();
    }
}

// ── Main ────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    // Load eval suite
    let suite_content = std::fs::read_to_string(&cli.suite)
        .with_context(|| format!("failed to read {}", cli.suite.display()))?;
    let mut suite: TaskSuite = serde_yaml::from_str(&suite_content)
        .with_context(|| "failed to parse eval suite YAML")?;

    // Load fusion-router config
    let fusion_config_path = std::env::var("FUSION_CONFIG")
        .unwrap_or_else(|_| suite.config.fusion_config.clone());
    let app_config = AppConfig::load(&fusion_config_path)
        .with_context(|| format!("failed to load fusion-router config from {}", fusion_config_path))?;

    // Resolve repeat count
    let repeats = cli.repeats.unwrap_or(if cli.dry_run {
        2
    } else {
        suite.config.repeats
    });

    // Resolve reuse references: copy prompt and ground_truth from the referenced task
    let task_map: HashMap<String, TaskDef> = suite
        .tasks
        .iter()
        .map(|t| (t.id.clone(), t.clone()))
        .collect();
    for task in &mut suite.tasks {
        if task.prompt.is_none() {
            if let Some(ref reuse_id) = task.reuse {
                if let Some(source) = task_map.get(reuse_id) {
                    task.prompt = source.prompt.clone();
                    if task.ground_truth.is_none() {
                        task.ground_truth = source.ground_truth.clone();
                    }
                }
            }
        }
    }

    // Filter tasks
    let tasks: Vec<TaskDef> = if cli.dry_run {
        // Dry run: first task per bucket only
        let mut seen_buckets = std::collections::HashSet::new();
        suite
            .tasks
            .into_iter()
            .filter(|t| {
                if let Some(ref bucket) = cli.bucket {
                    t.bucket == *bucket && seen_buckets.insert(t.bucket.clone())
                } else if let Some(ref task_id) = cli.task {
                    t.id == *task_id
                } else {
                    seen_buckets.insert(t.bucket.clone())
                }
            })
            .collect()
    } else {
        suite
            .tasks
            .into_iter()
            .filter(|t| {
                if let Some(ref bucket) = cli.bucket {
                    t.bucket == *bucket
                } else if let Some(ref task_id) = cli.task {
                    t.id == *task_id
                } else {
                    true
                }
            })
            .collect()
    };

    if tasks.is_empty() {
        anyhow::bail!("no tasks matched the given filters");
    }

    // Build provider for condition A
    let provider_a = build_provider_from_config(&app_config, &suite.config.baseline_provider)?;

    // Build pipeline components for conditions B & C
    let resource_manager = Arc::new(DefaultResourceManager::new(fusion_router::types::Quota {
        max_daily_cost: 100.0,
        max_daily_tokens: 10_000_000,
        max_concurrent: 10,
        provider_limits: HashMap::new(),
    }));

    let compiler = build_compiler(
        app_config.model_catalog.clone(),
        resource_manager.clone(),
        None,
    );

    let scheduler = DefaultScheduler::new(4);

    // Build strategies map for executor
    let mut strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>> = HashMap::new();
    strategies.insert(StrategyKind::Single, Box::new(SingleStrategy));
    strategies.insert(
        StrategyKind::Consensus,
        Box::new(ConsensusStrategy { count: 3 }),
    );
    strategies.insert(
        StrategyKind::Reflection,
        Box::new(ReflectionStrategy::default()),
    );
    strategies.insert(
        StrategyKind::Chain,
        Box::new(ChainStrategy { stages: vec![] }),
    );
    strategies.insert(
        StrategyKind::Debate,
        Box::new(DebateStrategy {
            debaters: vec![],
            judge: Box::new(SingleStrategy),
        }),
    );
    strategies.insert(
        StrategyKind::ReAct,
        Box::new(ReActStrategy::default()),
    );
    strategies.insert(
        StrategyKind::Fusion,
        Box::new(FusionStrategy::new(vec![])),
    );

    let executor = Arc::new(DefaultExecutor::new(provider_a.clone(), strategies));

    let planner = IntentPlanner::new(app_config.model_catalog.clone());

    println!(
        "Running {} tasks × 3 conditions × {} repeats",
        tasks.len(),
        repeats
    );
    println!("Baseline model: {}", suite.config.baseline_model);
    println!("Baseline provider: {}", suite.config.baseline_provider);
    println!();

    let mut all_results: Vec<RunResult> = Vec::new();

    for task in &tasks {
        eprintln!("  {} ({})", task.id, task.bucket);

        for _run_idx in 0..repeats {
            // Condition A: direct call
            let result_a = run_condition_a(
                provider_a.as_ref(),
                task,
                &suite.config.baseline_model,
            )
            .await;
            all_results.push(result_a);

            // Condition B: routed-single
            let result_b = run_condition_b(
                &compiler,
                &scheduler,
                executor.as_ref(),
                &planner,
                task,
                &suite.config.baseline_model,
            )
            .await;
            all_results.push(result_b);

            // Condition C: full fusion-router
            let result_c = run_condition_c(
                &compiler,
                &scheduler,
                executor.as_ref(),
                &planner,
                task,
                &suite.config.baseline_model,
            )
            .await;
            all_results.push(result_c);
        }
    }

    // Generate and print report
    let reports = generate_report(&all_results, &tasks);
    print_report(&reports);

    // Write raw results to JSON
    let output_path = cli.suite.parent().unwrap().join("results.json");
    let results_json = serde_json::to_string_pretty(&all_results)?;
    std::fs::write(&output_path, &results_json)?;
    println!("Raw results written to: {}", output_path.display());

    // Write report to JSON
    let report_path = cli.suite.parent().unwrap().join("report.json");
    let report_json = serde_json::to_string_pretty(&reports)?;
    std::fs::write(&report_path, &report_json)?;
    println!("Report written to: {}", report_path.display());

    Ok(())
}
