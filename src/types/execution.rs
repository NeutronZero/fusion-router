use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
        max_cost_usd: Option<f64>,
        max_tokens: Option<u64>,
        min_confidence: Option<f32>,
    },
}


#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OutputPreferences {
    #[serde(default)]
    pub include_report: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionReport {
    pub graph: GraphSummary,
    pub costs: Vec<ModelCost>,
    pub timing: TimingInfo,
    pub model_breakdown: HashMap<String, Usage>,
    pub decisions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphSummary {
    pub node_count: usize,
    pub max_depth: usize,
    pub strategy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCost {
    pub model: String,
    pub cost: f64,
    pub tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimingInfo {
    pub total_ms: u64,
    pub per_node: HashMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ChatCompletionRequest;

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
            max_cost_usd: Some(0.05),
            max_tokens: Some(4096),
            min_confidence: Some(0.8),
        };
        let json = serde_json::to_string(&intent).unwrap();
        let deserialized: ExecutionIntent = serde_json::from_str(&json).unwrap();
        match deserialized {
            ExecutionIntent::Constrained {
                max_latency_ms,
                max_cost_usd,
                max_tokens,
                min_confidence,
            } => {
                assert_eq!(max_latency_ms, Some(5000));
                assert_eq!(max_cost_usd, Some(0.05));
                assert_eq!(max_tokens, Some(4096));
                assert_eq!(min_confidence, Some(0.8));
            }
            _ => panic!("Expected Constrained variant"),
        }
    }

    #[test]
    fn test_constrained_with_all_none_fields() {
        let intent = ExecutionIntent::Constrained {
            max_latency_ms: None,
            max_cost_usd: None,
            max_tokens: None,
            min_confidence: None,
        };
        let json = serde_json::to_string(&intent).unwrap();
        let deserialized: ExecutionIntent = serde_json::from_str(&json).unwrap();
        match deserialized {
            ExecutionIntent::Constrained {
                max_latency_ms,
                max_cost_usd,
                max_tokens,
                min_confidence,
            } => {
                assert_eq!(max_latency_ms, None);
                assert_eq!(max_cost_usd, None);
                assert_eq!(max_tokens, None);
                assert_eq!(min_confidence, None);
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
    fn test_output_preferences_json_round_trip() {
        let prefs = OutputPreferences { include_report: true };
        let json = serde_json::to_string(&prefs).unwrap();
        let deserialized: OutputPreferences = serde_json::from_str(&json).unwrap();
        assert_eq!(prefs.include_report, deserialized.include_report);
    }

    #[test]
    fn test_output_preferences_default() {
        let prefs = OutputPreferences { include_report: false };
        let json = serde_json::to_string(&prefs).unwrap();
        let deserialized: OutputPreferences = serde_json::from_str(&json).unwrap();
        assert!(!deserialized.include_report);
    }

    #[test]
    fn test_chat_completion_request_with_execution_and_output() {
        let json = r#"{
            "model": "test-model",
            "messages": [{"role": "user", "content": "hello"}],
            "execution": {"mode": "speed"},
            "output": {"include_report": true}
        }"#;
        let request: ChatCompletionRequest = serde_json::from_str(json).unwrap();
        assert!(matches!(request.execution, Some(ExecutionIntent::Speed)));
        assert!(request.output.is_some());
        assert_eq!(request.output.unwrap().include_report, true);
    }

    #[test]
    fn test_chat_completion_request_without_execution() {
        let json = r#"{
            "model": "test-model",
            "messages": [{"role": "user", "content": "hello"}]
        }"#;
        let request: ChatCompletionRequest = serde_json::from_str(json).unwrap();
        assert!(request.execution.is_none());
        assert!(request.output.is_none());
    }

    #[test]
    fn test_execution_intent_default_is_balanced() {
        let intent = ExecutionIntent::default();
        assert!(matches!(intent, ExecutionIntent::Balanced));
    }

    #[test]
    fn test_execution_report_round_trip() {
        use std::collections::HashMap;
        let report = ExecutionReport {
            graph: GraphSummary {
                node_count: 5,
                max_depth: 3,
                strategy: "quality".to_string(),
            },
            costs: vec![ModelCost {
                model: "claude-sonnet".to_string(),
                cost: 0.05,
                tokens: 5000,
            }],
            timing: TimingInfo {
                total_ms: 1200,
                per_node: HashMap::new(),
            },
            model_breakdown: HashMap::new(),
            decisions: vec!["used quality mode".to_string()],
        };
        let json = serde_json::to_string(&report).unwrap();
        let deserialized: ExecutionReport = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.graph.node_count, 5);
        assert_eq!(deserialized.graph.max_depth, 3);
        assert_eq!(deserialized.graph.strategy, "quality");
    }

    #[test]
    fn test_execution_intent_flows_to_requirements() {
        let json = r#"{
            "model": "test",
            "messages": [{"role": "user", "content": "hello"}],
            "execution": {"mode": "constrained", "max_cost_usd": 0.01, "max_latency_ms": 1000, "max_tokens": 100, "min_confidence": 0.5}
        }"#;

        let request: ChatCompletionRequest = serde_json::from_str(json).unwrap();

        let mut reqs = crate::types::Requirements {
            intent_classification: crate::types::Intent::General,
            complexity: crate::types::ComplexityLevel::Medium,
            has_files: false,
            context_window: 4096,
            original_text: "test".to_string(),
            execution_intent: None,
            output_preferences: None,
        };
        reqs.execution_intent = request.execution.clone();
        reqs.output_preferences = request.output.clone();

        match reqs.execution_intent {
            Some(ExecutionIntent::Constrained { max_cost_usd, .. }) => {
                assert_eq!(max_cost_usd, Some(0.01));
            }
            _ => panic!("Expected Constrained variant"),
        }
        assert!(reqs.output_preferences.is_none());
    }
}
