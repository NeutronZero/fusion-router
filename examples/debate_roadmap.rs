use std::sync::Arc;

use fusion_router::compiler::context::CompilationContext;
use fusion_router::compiler::ir::{DebateRole, StrategyIR, PRIMITIVE_GRAPH_VERSION};
use fusion_router::compiler::optimization::OptimizationPipeline;
use fusion_router::providers::openrouter::OpenRouterProvider;
use fusion_router::providers::ChatProvider;
use fusion_router::strategies::debate::DebateStrategy;
use fusion_router::strategies::single::SingleStrategy;
use fusion_router::strategies::Strategy;
use fusion_router::types::ChatCompletionRequest;

const ROADMAP: &str = include_str!("../docs/roadmap-v0.9.md");

struct Role {
    name: &'static str,
    model: &'static str,
    system_prompt: &'static str,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let api_key = std::env::var("OPENROUTER_API_KEY")
        .expect("OPENROUTER_API_KEY must be set")
        .trim()
        .to_string();
    let provider: Arc<dyn ChatProvider + Send + Sync> = Arc::new(
        OpenRouterProvider::new(api_key),
    );

    let roles = vec![
        Role {
            name: "Defender",
            model: "deepseek/deepseek-chat",
            system_prompt: "You are a Defender evaluating the FusionRouter v0.9 roadmap. \
                Your role is to identify the strengths of this roadmap. Argue that the \
                current plan is well-structured, the phases flow logically from stabilization \
                to optimization to maturity, and the effort estimates are realistic for a \
                pre-1.0 project. Defend the sequencing decisions and highlight what the \
                roadmap gets right.",
        },
        Role {
            name: "Critic",
            model: "nousresearch/hermes-3-llama-3.1-405b",
            system_prompt: "You are a Critic evaluating the FusionRouter v0.9 roadmap. \
                Your role is to identify gaps, risks, and over-optimistic estimates. \
                Challenge whether Phase 3 items (WASM, missing ADRs) belong in v0.9 at all. \
                Question the PrimitiveGraph/ExecutionGraph drift risk. Point out what's \
                missing or underspecified. Be rigorous and demanding.",
        },
        Role {
            name: "Product",
            model: "meta-llama/llama-3.1-8b-instruct",
            system_prompt: "You are a Product Manager evaluating the FusionRouter v0.9 roadmap. \
                Assess whether this roadmap delivers user-facing value in each phase. \
                Evaluate the prioritization — are the right things being done in the right \
                order? Is the timeline feasible? What's missing from a user perspective? \
                Suggest what should be reprioritized or added.",
        },
        Role {
            name: "Architecture",
            model: "qwen/qwen-2.5-7b-instruct",
            system_prompt: "You are an Architect evaluating the FusionRouter v0.9 roadmap. \
                Focus on the technical architecture decisions. Assess the risk of \
                PrimitiveGraph vs ExecutionGraph drift, the optimization pass design, \
                the ADR-018 Phase 3 retirement of apply(), and whether the compiler \
                refactoring is complete enough. Evaluate the Artifact trait integration \
                and WASM plugin wiring from a system design perspective.",
        },
    ];

    let user_message = format!(
        "Analyze the following FusionRouter v0.9 roadmap document and provide your perspective:\n\n{}",
        ROADMAP
    );

    // ── Phase 1: Compiler Pipeline (FanOut / Barrier / Reducer) ──────────────

    println!("=== Compiler Manifest ================================================\n");

    let debate_strategy = DebateStrategy {
        debaters: vec![
            Box::new(SingleStrategy),
            Box::new(SingleStrategy),
            Box::new(SingleStrategy),
            Box::new(SingleStrategy),
        ],
        judge: Box::new(SingleStrategy),
    };

    let ctx = CompilationContext::new();
    let ir = StrategyIR::Debate {
        roles: roles.iter().map(|r| DebateRole {
            name: r.name.to_string(),
            model: r.model.to_string(),
            stance: r.system_prompt.to_string(),
        }).collect(),
    };

    let graph = debate_strategy.lower(&ir, &ctx).expect("lowering failed");
    let hash = graph.compute_hash();

    println!("Strategy:          Debate");
    println!("Graph Hash:        0x{:x}", hash);
    println!("PrimitiveGraph v{}", PRIMITIVE_GRAPH_VERSION);
    println!("Node Count:        {}", graph.nodes.len());
    println!("Edge Count:        {}", graph.edges.len());
    println!("OptimizationPass:  {} (no passes registered)", OptimizationPipeline::new().run(graph.clone()).ok().map(|_| "idempotent").unwrap_or("none"));
    println!();

    println!("--- PrimitiveGraph (Mermaid) ---");
    println!("{}", graph.to_mermaid());

    println!("--- PrimitiveGraph (DOT) ---");
    println!("{}", graph.to_dot());

    // ── Phase 2: Role Execution (4 parallel LLM calls) ──────────────────────

    println!("=== Role Analysis ===================================================\n");

    let mut responses: Vec<(String, String)> = Vec::new();

    for role in &roles {
        println!("--- {} ({}) ---", role.name, role.model);
        let request = ChatCompletionRequest {
            model: role.model.to_string(),
            messages: vec![
                fusion_router::types::ChatMessage {
                    role: "system".to_string(),
                    content: role.system_prompt.to_string(),
                },
                fusion_router::types::ChatMessage {
                    role: "user".to_string(),
                    content: user_message.clone(),
                },
            ],
            stream: false,
            temperature: Some(0.7),
            max_tokens: Some(2048),
            tools: None,
            files: None,
            execution: None,
            output: None,
        };

        let resp = provider.chat_completion(&request).await?;
        let content = resp.choices.first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        println!("{}\n", if content.len() > 500 {
            format!("{}...\n[{} chars total]", &content[..500], content.len())
        } else {
            content.clone()
        });

        responses.push((role.name.to_string(), content));
    }

    // ── Phase 3: Judge Synthesis (Reducer node) ─────────────────────────────

    println!("=== Judge's Synthesis ===============================================\n");

    let synthesis_prompt = format!(
        "You are a Judge synthesizing a structured debate about the FusionRouter v0.9 roadmap. \
        Below are four perspectives. Synthesize them into a coherent assessment: \
        identify where the perspectives converge, where they conflict, and what the \
        recommended actions should be. Provide a ranked list of priorities.\n\n{}",
        responses.iter()
            .map(|(name, content)| format!("<<<{}>>>\n{}", name, content))
            .collect::<Vec<_>>()
            .join("\n\n---\n\n")
    );

    let judge_request = ChatCompletionRequest {
        model: "deepseek/deepseek-chat".to_string(),
        messages: vec![
            fusion_router::types::ChatMessage {
                role: "user".to_string(),
                content: synthesis_prompt,
            },
        ],
        stream: false,
        temperature: Some(0.5),
        max_tokens: Some(4096),
        tools: None,
        files: None,
        execution: None,
        output: None,
    };

    let synthesis = provider.chat_completion(&judge_request).await?;
    let synthesis_content = synthesis.choices.first()
        .map(|c| c.message.content.clone())
        .unwrap_or_default();

    println!("{}", synthesis_content);

    // ── Phase 4: Provenance ──────────────────────────────────────────────────

    if let Some(usage) = synthesis.usage {
        println!("\n--- Usage ---");
        println!("Prompt tokens:     {}", usage.prompt_tokens);
        println!("Completion tokens: {}", usage.completion_tokens);
        println!("Total tokens:      {}", usage.total_tokens);
    }

    println!("\n--- Provenance ---");
    println!("Graph Hash:        0x{:x}", hash);
    println!("PrimitiveGraph v{}", PRIMITIVE_GRAPH_VERSION);
    println!("OptimizationPass:  None (idempotent)");
    println!("Artifact:          Debate");

    Ok(())
}
