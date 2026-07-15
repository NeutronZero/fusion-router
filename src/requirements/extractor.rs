use std::collections::HashMap;

use crate::types::{Complexity, ContextSnapshot, Intent, Requirements};

pub trait RequirementsExtractor: Send + Sync {
    fn extract(&self, ctx: &ContextSnapshot) -> Requirements;
}

pub struct DefaultRequirementsExtractor;

impl RequirementsExtractor for DefaultRequirementsExtractor {
    fn extract(&self, ctx: &ContextSnapshot) -> Requirements {
        let intent = classify_intent(ctx);
        let complexity = compute_complexity(ctx);

        let mut soft_scores = HashMap::new();
        soft_scores.insert("intent_confidence".to_string(), 1.0);
        soft_scores.insert("complexity_score".to_string(), complexity_score(&complexity));

        let mut hard_constraints = HashMap::new();
        hard_constraints.insert("max_tokens".to_string(), ctx.max_tokens.to_string());

        Requirements {
            intent,
            complexity,
            soft_scores,
            hard_constraints,
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

fn compute_complexity(ctx: &ContextSnapshot) -> Complexity {
    let total_chars: usize = ctx.messages.iter().map(|m| m.content.len()).sum();
    let file_count = ctx.files.len();

    match (total_chars, file_count) {
        (c, _) if c > 10_000 => Complexity::Critical,
        (c, f) if c > 5_000 || f > 5 => Complexity::High,
        (c, f) if c > 1_000 || f > 2 => Complexity::Medium,
        _ => Complexity::Low,
    }
}

fn complexity_score(c: &Complexity) -> f32 {
    match c {
        Complexity::Low => 0.25,
        Complexity::Medium => 0.5,
        Complexity::High => 0.75,
        Complexity::Critical => 1.0,
    }
}
