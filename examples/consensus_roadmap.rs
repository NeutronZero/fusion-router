use std::sync::Arc;

use fusion_router::compiler::context::CompilationContext;
use fusion_router::compiler::ir::{StrategyIR, PRIMITIVE_GRAPH_VERSION};
use fusion_router::providers::openrouter::OpenRouterProvider;
use fusion_router::providers::ChatProvider;
use fusion_router::strategies::consensus::ConsensusStrategy;
use fusion_router::strategies::Strategy;
use fusion_router::types::{
    ChatCompletionRequest, ExecutionNode, ExecutionNodeKind, RetryPolicy, StrategyKind,
};

const CONSENSUS_COUNT: u32 = 3;

fn roadmap_context() -> String {
    std::fs::read_to_string(format!(
        "{}/docs/repair-phase0-scope.md",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|_| {
        "FusionRouter v0.9 roadmap: full roadmap document is unavailable.".to_string()
    })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let api_key = std::env::var("OPENROUTER_API_KEY")
        .expect("OPENROUTER_API_KEY must be set")
        .trim()
        .to_string();
    let provider: Arc<dyn ChatProvider + Send + Sync> = Arc::new(OpenRouterProvider::new(api_key));
    let model = "deepseek/deepseek-chat";

    // ── Phase 1: Compiler Pipeline ────────────────────────────────────────

    println!("=== Compiler Manifest ================================================\n");

    let strategy = ConsensusStrategy {
        count: CONSENSUS_COUNT,
    };
    let ctx = CompilationContext::new();
    let graph = strategy.lower(
        &StrategyIR::Consensus {
            count: CONSENSUS_COUNT,
            members: vec![],
        },
        &ctx,
    )?;
    let hash = graph.compute_hash();

    let template = ExecutionNode {
        id: uuid::Uuid::nil(),
        kind: ExecutionNodeKind::LLMGenerate,
        strategy: StrategyKind::Consensus,
        model: "default".into(),
        retry_policy: RetryPolicy {
            max_retries: 2,
            backoff_ms: 1000,
        },
        fallback: None,
        config: std::collections::HashMap::new(),
        subgraph: None,
    };
    let execution_graph = graph.to_execution_graph(
        template.strategy.clone(),
        &template.retry_policy,
        &template.fallback,
        &template.config,
    );

    println!("Strategy:          Consensus");
    println!("Graph Hash:        0x{:x}", hash);
    println!("PrimitiveGraph v{}", PRIMITIVE_GRAPH_VERSION);
    println!("Node Count:        {}", graph.nodes.len());
    println!("Edge Count:        {}", graph.edges.len());
    println!("ExecutionGraph Node Count: {}", execution_graph.nodes.len());
    println!(
        "ExecutionGraph Primitive Hash: 0x{:x}",
        execution_graph.primitive_graph_hash
    );
    println!("Parallel Copies:   {}", CONSENSUS_COUNT);
    println!();

    println!("{}", graph.to_mermaid());

    // ── Phase 2: Parallel Consensus Calls ─────────────────────────────────

    println!("=== Consensus Analysis =============================================\n");

    let user_message = format!(
        "Analyze the following FusionRouter v0.9 roadmap. Identify the single most \
         important risk, the single most important strength, and what should change:\n\n{}",
        roadmap_context()
    );

    let mut responses: Vec<(usize, String)> = Vec::new();

    for i in 0..CONSENSUS_COUNT {
        println!("--- Consensus Member {} ---", i + 1);
        let request = ChatCompletionRequest {
            model: model.to_string(),
            messages: vec![fusion_router::types::ChatMessage {
                role: "user".to_string(),
                content: user_message.clone(),
            }],
            stream: false,
            temperature: Some(0.7),
            max_tokens: Some(2048),
            tools: None,
            files: None,
            execution: None,
            output: None,
            strategy: None,
        };

        let resp = provider.chat_completion(&request).await?;
        let content = resp
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        println!(
            "{}\n",
            if content.len() > 500 {
                format!("{}...\n[{} chars total]", &content[..500], content.len())
            } else {
                content.clone()
            }
        );

        responses.push(((i + 1) as usize, content));
    }

    // ── Phase 3: Judge selects best response ──────────────────────────────

    println!("=== Judge Selection ================================================\n");

    let judge_prompt = format!(
        "You are a judge selecting the best analysis of the FusionRouter v0.9 roadmap. \
         Below are {} independent analyses. Evaluate each for accuracy, completeness, \
         and insight. Select the best one and explain why.\n\n{}",
        CONSENSUS_COUNT,
        responses
            .iter()
            .map(|(id, content)| format!("<<<Analysis {}>>>\n{}", id, content))
            .collect::<Vec<_>>()
            .join("\n\n---\n\n")
    );

    let judge_request = ChatCompletionRequest {
        model: model.to_string(),
        messages: vec![fusion_router::types::ChatMessage {
            role: "user".to_string(),
            content: judge_prompt,
        }],
        stream: false,
        temperature: Some(0.3),
        max_tokens: Some(2048),
        tools: None,
        files: None,
        execution: None,
        output: None,
        strategy: None,
    };

    let verdict = provider.chat_completion(&judge_request).await?;
    let verdict_content = verdict
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .unwrap_or_default();

    println!("{}", verdict_content);

    // ── Phase 4: Provenance ───────────────────────────────────────────────

    if let Some(usage) = verdict.usage {
        println!("\n--- Usage ---");
        println!("Prompt tokens:     {}", usage.prompt_tokens);
        println!("Completion tokens: {}", usage.completion_tokens);
        println!("Total tokens:      {}", usage.total_tokens);
    }

    println!("\n--- Provenance ---");
    println!("Graph Hash:        0x{:x}", hash);
    println!("PrimitiveGraph v{}", PRIMITIVE_GRAPH_VERSION);
    println!(
        "ExecutionGraph Primitive Hash: 0x{:x}",
        execution_graph.primitive_graph_hash
    );
    println!("Strategy:          Consensus (x{})", CONSENSUS_COUNT);
    println!("Artifact:          Consensus");

    Ok(())
}
