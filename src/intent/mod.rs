use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod lowering;

pub use lowering::intent_to_workflow;

/// Canonical representation of goals and constraints (v0.13 contract 1).
/// Provider-free by contract: no model, provider, or endpoint fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedIntent {
    pub intent_id: Uuid,
    pub goal: String,
    pub kind: IntentKind,
    pub constraints: Constraints,
    pub budget: Budget,
    pub session_id: Option<Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntentKind {
    Code,
    Debug,
    Architecture,
    General,
    Creative,
    Analysis,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Constraints {
    pub max_latency_ms: Option<u64>,
    pub max_cost_usd: Option<f64>,
    pub max_tokens: Option<u64>,
    pub min_confidence: Option<f32>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Budget {
    pub max_cost_usd: Option<f64>,
    pub max_tokens: Option<u64>,
    pub max_execution_ms: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> NormalizedIntent {
        NormalizedIntent {
            intent_id: Uuid::new_v4(),
            goal: "implement the payments endpoint".into(),
            kind: IntentKind::Code,
            constraints: Constraints {
                max_latency_ms: Some(5_000),
                ..Constraints::default()
            },
            budget: Budget::default(),
            session_id: None,
        }
    }

    #[test]
    fn serde_round_trip_preserves_fields() {
        let intent = sample();
        let json = serde_json::to_string(&intent).unwrap();
        let back: NormalizedIntent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.intent_id, intent.intent_id);
        assert_eq!(back.goal, intent.goal);
        assert_eq!(back.kind, IntentKind::Code);
        assert_eq!(back.constraints.max_latency_ms, Some(5_000));
        assert!(back.budget.max_tokens.is_none());
    }

    #[test]
    fn rejects_provider_fields() {
        let intent = sample();
        let mut json = serde_json::to_string(&intent).unwrap();
        let trimmed = json.trim_end_matches('}');
        json = format!("{trimmed}, \"model\":\"gpt-4\"}}");
        assert!(serde_json::from_str::<NormalizedIntent>(&json).is_err());
    }

    #[test]
    fn all_intent_kinds_round_trip() {
        for kind in [
            IntentKind::Code,
            IntentKind::Debug,
            IntentKind::Architecture,
            IntentKind::General,
            IntentKind::Creative,
            IntentKind::Analysis,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            let back: IntentKind = serde_json::from_str(&json).unwrap();
            assert_eq!(back, kind);
        }
    }

    #[test]
    fn budget_serde_round_trip() {
        let budget = Budget {
            max_cost_usd: Some(0.05),
            max_tokens: Some(4096),
            max_execution_ms: Some(30_000),
        };
        let json = serde_json::to_string(&budget).unwrap();
        let back: Budget = serde_json::from_str(&json).unwrap();
        assert_eq!(back, budget);
    }
}
