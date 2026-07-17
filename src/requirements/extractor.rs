use crate::types::{ComplexityLevel, ContextSnapshot, Intent, Requirements};

pub trait RequirementsExtractor: Send + Sync {
    fn extract(&self, ctx: &ContextSnapshot) -> Requirements;
}

pub struct DefaultRequirementsExtractor;

impl RequirementsExtractor for DefaultRequirementsExtractor {
    fn extract(&self, ctx: &ContextSnapshot) -> Requirements {
        let intent = classify_intent(ctx);
        let complexity = compute_complexity(ctx);
        let original_text: String = ctx
            .messages
            .iter()
            .map(|m| m.content.clone())
            .collect::<Vec<_>>()
            .join("\n");

        Requirements {
            intent_classification: intent,
            complexity,
            has_files: !ctx.files.is_empty(),
            context_window: ctx.max_tokens as u64,
            original_text,
        }
    }
}

fn classify_intent(ctx: &ContextSnapshot) -> Intent {
    let combined: String = ctx
        .messages
        .iter()
        .map(|m| m.content.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");

    let keywords = [
        (Intent::Code, vec!["code", "function", "implement", "write a program", "class", "api"]),
        (Intent::Debug, vec!["bug", "error", "fix", "issue", "crash", "incorrect"]),
        (Intent::Architecture, vec!["design", "architecture", "system", "component", "module"]),
        (Intent::Analysis, vec!["analyze", "explain", "compare", "evaluate", "review"]),
        (Intent::Creative, vec!["story", "poem", "creative", "imagine", "generate"]),
    ];

    let mut max_score = 0usize;
    let mut best = Intent::General;

    for (intent, kws) in &keywords {
        let score = kws.iter().filter(|kw| combined.contains(*kw)).count();
        if score > max_score {
            max_score = score;
            best = intent.clone();
        }
    }

    best
}

fn compute_complexity(ctx: &ContextSnapshot) -> ComplexityLevel {
    let total_chars: usize = ctx.messages.iter().map(|m| m.content.len()).sum();
    let file_count = ctx.files.len();

    match (total_chars, file_count) {
        (c, _) if c > 10_000 => ComplexityLevel::Critical,
        (c, f) if c > 5_000 || f > 5 => ComplexityLevel::High,
        (c, f) if c > 1_000 || f > 2 => ComplexityLevel::Medium,
        _ => ComplexityLevel::Low,
    }
}
