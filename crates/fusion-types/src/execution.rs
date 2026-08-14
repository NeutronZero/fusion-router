use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
#[derive(Default)]
pub enum ExecutionIntent {
    Quality,
    Speed,
    #[default]
    Balanced,
    Exhaustive,
    Constrained {
        max_latency_ms: Option<u64>,
        #[serde(default, alias = "max_cost_usd")]
        max_cost: Option<fusion_core::NanoUSD>,
        max_tokens: Option<u64>,
        min_confidence: Option<f32>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OutputPreferences {
    #[serde(default)]
    pub include_report: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quality_json_round_trip() {
        let intent = ExecutionIntent::Quality;
        let json = serde_json::to_string(&intent).unwrap();
        let deserialized: ExecutionIntent = serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized, ExecutionIntent::Quality));
    }

    #[test]
    fn test_speed_json_round_trip() {
        let intent = ExecutionIntent::Speed;
        let json = serde_json::to_string(&intent).unwrap();
        let deserialized: ExecutionIntent = serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized, ExecutionIntent::Speed));
    }

    #[test]
    fn test_balanced_json_round_trip() {
        let intent = ExecutionIntent::Balanced;
        let json = serde_json::to_string(&intent).unwrap();
        let deserialized: ExecutionIntent = serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized, ExecutionIntent::Balanced));
    }

    #[test]
    fn test_exhaustive_json_round_trip() {
        let intent = ExecutionIntent::Exhaustive;
        let json = serde_json::to_string(&intent).unwrap();
        let deserialized: ExecutionIntent = serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized, ExecutionIntent::Exhaustive));
    }

    #[test]
    fn test_constrained_json_round_trip() {
        let intent = ExecutionIntent::Constrained {
            max_latency_ms: Some(5000),
            max_cost: Some(fusion_core::NanoUSD::from_nanos(50_000_000)),
            max_tokens: Some(4096),
            min_confidence: Some(0.8),
        };
        let json = serde_json::to_string(&intent).unwrap();
        let deserialized: ExecutionIntent = serde_json::from_str(&json).unwrap();
        match deserialized {
            ExecutionIntent::Constrained {
                max_latency_ms,
                max_cost,
                max_tokens,
                min_confidence,
            } => {
                assert_eq!(max_latency_ms, Some(5000));
                assert_eq!(max_cost, Some(fusion_core::NanoUSD::from_nanos(50_000_000)));
                assert_eq!(max_tokens, Some(4096));
                assert_eq!(min_confidence, Some(0.8));
            }
            _ => panic!("Expected Constrained variant"),
        }
    }

    #[test]
    fn test_tagged_json_deserialization() {
        let json = r#"{"mode": "quality"}"#;
        let intent: ExecutionIntent = serde_json::from_str(json).unwrap();
        assert!(matches!(intent, ExecutionIntent::Quality));

        let json = r#"{"mode": "speed"}"#;
        let intent: ExecutionIntent = serde_json::from_str(json).unwrap();
        assert!(matches!(intent, ExecutionIntent::Speed));

        let json = r#"{"mode": "balanced"}"#;
        let intent: ExecutionIntent = serde_json::from_str(json).unwrap();
        assert!(matches!(intent, ExecutionIntent::Balanced));

        let json = r#"{"mode": "exhaustive"}"#;
        let intent: ExecutionIntent = serde_json::from_str(json).unwrap();
        assert!(matches!(intent, ExecutionIntent::Exhaustive));
    }

    #[test]
    fn test_execution_intent_default_is_balanced() {
        let intent = ExecutionIntent::default();
        assert!(matches!(intent, ExecutionIntent::Balanced));
    }
}
