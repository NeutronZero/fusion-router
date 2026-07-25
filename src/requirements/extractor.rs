use crate::types::{ComplexityLevel, ContextSnapshot, Intent, Requirements};

pub trait RequirementsExtractor: Send + Sync {
    fn extract(&self, ctx: &ContextSnapshot) -> Requirements;
}

pub struct DefaultRequirementsExtractor;

impl DefaultRequirementsExtractor {
    fn build_model_requirements(&self, ctx: &ContextSnapshot, intent: &Intent) -> crate::providers::ModelRequirements {
        let mut mr = crate::providers::ModelRequirements::default();

        match intent {
            Intent::Code | Intent::Debug => {
                mr.min_coding_score = Some(0.8);
            }
            Intent::Architecture => {
                mr.min_reasoning_score = Some(0.85);
                mr.min_coding_score = Some(0.7);
            }
            Intent::Analysis => {
                mr.min_reasoning_score = Some(0.7);
            }
            _ => {}
        }

        if !ctx.tools.is_empty() {
            mr.requires_tools = true;
        }

        if ctx.messages.iter().any(|m| m.content.len() > 10_000) {
            mr.min_context_tokens = Some(32_000);
        } else if ctx.max_tokens > 16_000 || ctx.max_tokens as u64 > 16_000 {
            mr.min_context_tokens = Some(ctx.max_tokens);
        }

        // Default: prefer streaming-capable models
        mr.requires_streaming = true;

        mr
    }
}

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
            intent_classification: intent.clone(),
            complexity,
            has_files: !ctx.files.is_empty(),
            context_window: ctx.max_tokens as u64,
            original_text,
            execution_intent: None,
            output_preferences: None,
            model_requirements: Some(self.build_model_requirements(ctx, &intent)),
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

#[cfg(test)]
mod tests {
    use crate::types::ChatMessage;
    use super::RequirementsExtractor;

    fn code_context() -> crate::types::ContextSnapshot {
        crate::types::ContextSnapshot {
            messages: vec![ChatMessage {
                role: "user".into(),
                content: "Write a function that calculates fibonacci numbers".into(),
            }],
            files: vec![],
            tools: vec![],
            max_tokens: 4096,
            temperature: 0.7,
        }
    }

    fn tools_context() -> crate::types::ContextSnapshot {
        crate::types::ContextSnapshot {
            messages: vec![ChatMessage {
                role: "user".into(),
                content: "Analyze this dataset".into(),
            }],
            files: vec![],
            tools: vec![crate::types::ToolDefinition {
                name: "calculator".into(),
                description: "performs math".into(),
                parameters: None,
            }],
            max_tokens: 4096,
            temperature: 0.7,
        }
    }

    fn large_context() -> crate::types::ContextSnapshot {
        let long = "word ".repeat(3000);
        crate::types::ContextSnapshot {
            messages: vec![ChatMessage {
                role: "user".into(),
                content: long,
            }],
            files: vec![],
            tools: vec![],
            max_tokens: 128_000,
            temperature: 0.7,
        }
    }

    #[test]
    fn test_code_intent_sets_coding_score() {
        let extractor = super::DefaultRequirementsExtractor;
        let reqs = extractor.extract(&code_context());
        let mr = reqs.model_requirements.expect("should set model_requirements");
        assert_eq!(mr.min_coding_score, Some(0.8));
        assert!(mr.requires_streaming);
    }

    #[test]
    fn test_tools_present_sets_requires_tools() {
        let extractor = super::DefaultRequirementsExtractor;
        let reqs = extractor.extract(&tools_context());
        let mr = reqs.model_requirements.expect("should set model_requirements");
        assert!(mr.requires_tools);
    }

    #[test]
    fn test_large_context_sets_min_tokens() {
        let extractor = super::DefaultRequirementsExtractor;
        let reqs = extractor.extract(&large_context());
        let mr = reqs.model_requirements.expect("should set model_requirements");
        assert!(mr.min_context_tokens.is_some());
        assert!(mr.min_context_tokens.unwrap() > 0);
    }
}
