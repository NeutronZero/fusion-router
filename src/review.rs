//! Offline multi-model review runner.
//!
//! `fusion-router review` runs a consensus-ensembled review completely
//! in-process: every `--members` model reviews the same `--files` with its
//! own bounded tool loop (file_read), and the judge (the last member model)
//! consolidates the member reviews into a single report. No HTTP server or
//! process lifecycle is involved — the runner exits when the review ends.
//!
//! Machinery is the production path: `ProviderRegistry` routing by model key
//! prefix, compile-time consensus `expanded_subgraph` lowering with
//! per-member models, `DefaultExecutor` with the bounded tool loop, and
//! `DefaultScheduler` for execution.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Context;
use uuid::Uuid;

use crate::config::AppConfig;
use crate::executor::DefaultExecutor;
use crate::providers::circuit_breaker::CircuitBreaker;
use crate::providers::openrouter::OpenRouterProvider;
use crate::providers::registry::ProviderRegistry;
use crate::providers::router::ProviderTarget;
use crate::providers::zen::ZenProvider;
use crate::scheduler::default::DefaultScheduler;
use crate::scheduler::Scheduler;
use crate::strategies::consensus::ConsensusStrategy;
use crate::strategies::single::SingleStrategy;
use crate::strategies::Strategy;
use crate::tools::builtin::{CalculatorTool, FileReadTool};
use crate::tools::{HTTPRequestTool, ShellCommandTool, ToolRegistry};
use crate::types::{ExecutionGraph, ExecutionNode, ExecutionNodeKind, GraphMetadata, ReservationId, RetryPolicy, StrategyKind};

/// Resolves an API key from the environment, mirroring `main::resolve_api_key`.
fn resolve_api_key(env_var: &str, placeholder: &str, unsafe_dev: bool) -> anyhow::Result<String> {
    if let Ok(key) = std::env::var(env_var) {
        if !key.trim().is_empty() {
            return Ok(key);
        }
    }
    if cfg!(debug_assertions) || unsafe_dev {
        tracing::warn!(env_var = %env_var, "API key missing; placeholder (debug/--unsafe-dev only)");
        return Ok(placeholder.to_string());
    }
    anyhow::bail!("API key environment variable '{env_var}' is required but missing or empty")
}

/// Builds the same provider registry the main server uses.
pub fn build_provider_registry(
    config: &AppConfig,
    unsafe_dev: bool,
) -> anyhow::Result<Arc<ProviderRegistry>> {
    let openrouter_key = resolve_api_key("OPENROUTER_API_KEY", "test-key", unsafe_dev)?;
    let default_target = ProviderTarget::new(
        "default".to_string(),
        CircuitBreaker::new(5, 3, 30),
        Box::new(move || -> Arc<dyn crate::providers::ChatProvider + Send + Sync> {
            Arc::new(OpenRouterProvider::new(openrouter_key.clone()))
        }),
    );
    let registry = Arc::new(ProviderRegistry::new(default_target));

    for (name, cfg) in &config.providers {
        let api_key = match cfg.api_key_env.as_ref() {
            Some(var) => resolve_api_key(var, &format!("test-key-{name}"), unsafe_dev)?,
            None if cfg!(debug_assertions) || unsafe_dev => {
                tracing::warn!(provider = %name, "no api_key_env configured; placeholder key");
                format!("test-key-{name}")
            }
            None => anyhow::bail!(
                "provider '{name}' has no api_key_env configured; refusing to run"
            ),
        };

        let circuit_breaker = CircuitBreaker::new(cfg.failure_threshold, 3, cfg.cooldown_secs);
        let factory_name = name.clone();
        let factory_key = api_key.clone();
        let target = ProviderTarget::new(
            name.clone(),
            circuit_breaker,
            Box::new(move || -> Arc<dyn crate::providers::ChatProvider + Send + Sync> {
                if factory_name == "openrouter" {
                    Arc::new(OpenRouterProvider::new(factory_key.clone()))
                } else {
                    Arc::new(ZenProvider::new(factory_key.clone()))
                }
            }),
        );
        registry.register_target(vec![name.clone() + "/"], target);
    }

    Ok(registry)
}

/// Builds the tool registry like `AppState::new` in handlers.rs.
fn build_tool_registry(config: &AppConfig) -> ToolRegistry {
    let mut tool_registry = ToolRegistry::new();
    tool_registry.register(Arc::new(CalculatorTool));
    for dir in &config.tools.allowed_read_directories {
        tool_registry.register(Arc::new(FileReadTool::new(dir.clone())));
    }
    if config.tools.enable_http_tool {
        tool_registry.register(Arc::new(HTTPRequestTool::new()));
    }
    tool_registry.register(Arc::new(ShellCommandTool::new(
        config.tools.allowed_shell_commands.clone(),
        config.tools.shell_timeout_secs,
        config.tools.allowed_read_directories.clone(),
        config.tools.allow_unrestricted_args,
    )));
    tool_registry
}

fn default_message() -> String {
    "Review these FusionRouter source files for correctness, security, robustness, and architecture. Read each file fully with the file_read tool, then publish your numbered prioritized review. Format every entry as '[Severity: Critical|High|Medium] <file:line> <finding> — <why>'. Answer with the review itself, no preamble.".into()
}

/// Arguments parsed from the CLI (see `usage`).
pub struct ReviewArgs {
    pub config_path: String,
    pub members: Vec<String>,
    pub max_tool_rounds: u64,
    pub files: Vec<String>,
    pub message: Option<String>,
}

impl Default for ReviewArgs {
    fn default() -> Self {
        Self {
            config_path: std::env::var("FUSION_CONFIG").unwrap_or_else(|_| "config/self-analysis.yaml".into()),
            members: vec![
                "zen/deepseek-v4-flash-free".into(),
                "openrouter/openai/gpt-oss-20b:free".into(),
                "openrouter/nvidia/nemotron-3-nano-30b-a3b:free".into(),
            ],
            max_tool_rounds: 6,
            files: vec![
                "src/executor/mod.rs".into(),
                "src/server/handlers.rs".into(),
                "src/providers/registry.rs".into(),
                "src/transport/http.rs".into(),
                "src/resource/cancelling_stream.rs".into(),
                "src/config/mod.rs".into(),
            ],
            message: None,
        }
    }
}

impl ReviewArgs {
    pub fn from_args() -> Self {
        let mut args = Self::default();
        let raw: Vec<String> = std::env::args().skip(2).collect();
        let mut i = 0;
        let mut members_given = false;
        while i < raw.len() {
            match raw[i].as_str() {
                "--config" => {
                    if let Some(v) = raw.get(i + 1) {
                        args.config_path = v.clone();
                        i += 1;
                    }
                }
                "--members" => {
                    if !members_given {
                        args.members.clear();
                        members_given = true;
                    }
                    let mut taken = 0;
                    while let Some(v) = raw.get(i + 1 + taken) {
                        if v.starts_with("--") {
                            break;
                        }
                        args.members.push(v.clone());
                        taken += 1;
                    }
                    i += taken;
                }
                "--max-tool-rounds" | "--rounds" => {
                    if let Some(v) = raw.get(i + 1) {
                        args.max_tool_rounds = v.parse().unwrap_or(args.max_tool_rounds);
                        i += 1;
                    }
                }
                "--files" => {
                    let mut taken = 0;
                    while let Some(v) = raw.get(i + 1 + taken) {
                        if v.starts_with("--") {
                            break;
                        }
                        args.files.push(v.clone());
                        taken += 1;
                    }
                    i += taken;
                }
                "--message" => {
                    if let Some(v) = raw.get(i + 1) {
                        args.message = Some(v.clone());
                        i += 1;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        if members_given && args.members.len() < 2 {
            args.members = Self::default().members;
        }
        args
    }
}

pub fn usage() -> String {
    "fusion-router review — multi-model self-review of the FusionRouter codebase\n\
     \n\
     Usage:\n\
       fusion-router review [--config PATH] [--members MODEL]...\n\
                            [--max-tool-rounds N] [--files FILE]... [--message TEXT]\n\
     \n\
     Defaults: 3 free models (zen + openrouter), 6 core source files, 6 tool rounds/\n\
     member. Every member reviews the files with its own file_read tool loop and the\n\
     judge (last member) consolidates the reviews.\n"
        .to_string()
}

/// Runs the in-process multi-model review and prints the consolidated report.
pub async fn run(args: ReviewArgs) -> anyhow::Result<()> {
    let unsafe_dev = std::env::args().any(|a| a == "--unsafe-dev");
    let config = AppConfig::load(&args.config_path)
        .with_context(|| format!("failed to load config from {}", args.config_path))?;
    if config.unsafe_dev && !unsafe_dev {
        anyhow::bail!("config sets `unsafe_dev: true`; re-run with `--unsafe-dev`");
    }
    if let Err(errors) = config.validate() {
        anyhow::bail!("configuration validation failed: {errors:?}");
    }

    let provider_registry = build_provider_registry(&config, unsafe_dev)?;

    let member_models = args.members.clone();
    for model in &member_models {
        if provider_registry.get_matching_targets(model).is_empty() {
            anyhow::bail!("no provider can route model '{model}'");
        }
    }

    let tool_registry = build_tool_registry(&config);
    let mut strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>> = HashMap::new();
    strategies.insert(StrategyKind::Single, Box::new(SingleStrategy));
    strategies.insert(
        StrategyKind::Consensus,
        Box::new(ConsensusStrategy::default()),
    );

    let executor = Arc::new(
        DefaultExecutor::new(provider_registry.clone(), strategies)
            .with_tool_registry(Arc::new(tool_registry))
            .with_allow_auto_exec(config.tools.allow_auto_exec),
    );

    // Build the review node the same way the request-level strategy override
    // does (handlers.rs), then attach the compile-time consensus subgraph.
    let mut node_config: HashMap<String, serde_json::Value> = HashMap::new();
    node_config.insert("count".into(), serde_json::json!(member_models.len()));
    node_config.insert("members".into(), serde_json::json!(member_models));
    node_config.insert("max_tool_rounds".into(), serde_json::json!(args.max_tool_rounds));
    node_config.insert(
        "messages".into(),
        serde_json::json!([
            {
                "role": "system",
                "content": "You are a reviewer in a multi-model code review ensemble. Use the file_read tool to read the listed source files, cite exact file:line locations, and produce a critical review. End with a numbered prioritized list of findings in the form '[Severity: Critical|High|Medium] finding — why'."
            },
            {
                "role": "user",
                "content": format!(
                    "{}\n\nFiles to review:\n{}",
                    args.message.clone().unwrap_or_else(default_message),
                    args.files.iter().map(|f| format!("- {f}")).collect::<Vec<_>>().join("\n")
                )
            }
        ]),
    );
    node_config.insert("tool_allowlist".into(), serde_json::json!(["file_read", "calculator"]));

    let judge_model = member_models.last().cloned().unwrap_or_else(|| "zen/deepseek-v4-flash-free".into());
    let mut node = ExecutionNode {
        id: Uuid::new_v4(),
        kind: ExecutionNodeKind::LLMGenerate,
        strategy: StrategyKind::Consensus,
        model: judge_model.clone(),
        retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
        fallback: None,
        config: node_config,
        subgraph: None,
    };
    let subgraph = crate::compiler::strategy_expansion::expanded_subgraph(&node)
        .ok_or_else(|| anyhow::anyhow!("consensus expansion produced no subgraph"))?;
    node.subgraph = Some(subgraph);

    let graph = ExecutionGraph {
        graph_id: Uuid::new_v4(),
        nodes: vec![node],
        edges: vec![],
        metadata: GraphMetadata { estimated_cost: 0.0, estimated_tokens: 0, max_depth: 0, node_count: 1 },
        total_tokens: 0,
        total_cost: 0,
        primitive_graph_hash: 0,
    };

    tracing::info!(
        members = ?member_models,
        files = args.files.len(),
        rounds = args.max_tool_rounds,
        "starting multi-model review"
    );

    let scheduler = DefaultScheduler::new(1);
    let mut instance = scheduler.schedule(graph, ReservationId(Uuid::new_v4()));
    let start = std::time::Instant::now();
    let result = scheduler
        .run(&mut instance, executor.as_ref())
        .await
        .context("review execution failed")?;
    let elapsed = start.elapsed();

    println!("\n========== MULTI-MODEL REVIEW RESULT ==========");
    println!("success: {}", result.success);
    println!("took: {:.1}s", elapsed.as_secs_f64());
    println!("tokens: {}", result.total_tokens);
    println!("cost: ${:.4}", result.total_cost);
    println!("\n=== Consolidated report (judge: {judge_model}) ===");
    let report = result
        .final_output
        .as_ref()
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| result.final_output.as_ref().map(|v| v.to_string()))
        .unwrap_or_else(|| "(no textual report)".to_string());
    println!("{report}");

    Ok(())
}