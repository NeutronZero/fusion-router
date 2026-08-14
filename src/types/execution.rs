// Re-export from fusion-types (canonical source)
pub use fusion_types::execution::*;

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
    fn test_constrained_with_all_none_fields() {
        let intent = ExecutionIntent::Constrained {
            max_latency_ms: None,
            max_cost: None,
            max_tokens: None,
            min_confidence: None,
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
                assert_eq!(max_latency_ms, None);
                assert_eq!(max_cost, None);
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
        assert!(request.output.unwrap().include_report);
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
    fn test_chat_completion_request_with_ensemble_strategy() {
        let json = r#"{
            "model": "test-model",
            "messages": [{"role": "user", "content": "review"}],
            "strategy": {
                "kind": "Consensus",
                "count": 3,
                "members": ["zen/model-a", "openrouter/model-b", "openrouter/model-c"],
                "max_tool_rounds": 5
            }
        }"#;
        let request: ChatCompletionRequest = serde_json::from_str(json).unwrap();
        let strategy = request.strategy.expect("strategy present");
        assert_eq!(strategy.kind, "Consensus");
        assert_eq!(strategy.count, 3);
        assert_eq!(strategy.members, vec!["zen/model-a", "openrouter/model-b", "openrouter/model-c"]);
        assert_eq!(strategy.max_tool_rounds, 5);
    }

    #[test]
    fn test_chat_completion_request_strategy_defaults() {
        let json = r#"{
            "model": "test-model",
            "messages": [{"role": "user", "content": "review"}],
            "strategy": {"kind": "Consensus", "members": ["a", "b"]}
        }"#;
        let request: ChatCompletionRequest = serde_json::from_str(json).unwrap();
        let strategy = request.strategy.expect("strategy present");
        assert_eq!(strategy.kind, "Consensus");
        assert_eq!(strategy.count, 3, "count defaults to 3");
        assert_eq!(strategy.max_tool_rounds, 8, "max_tool_rounds defaults to 8");
    }

    #[test]
    fn test_execution_intent_default_is_balanced() {
        let intent = ExecutionIntent::default();
        assert!(matches!(intent, ExecutionIntent::Balanced));
    }

    #[test]
    fn test_execution_report_round_trip() {
        use serde::{Deserialize, Serialize};
        use std::collections::HashMap;

        #[derive(Debug, Clone, Serialize, Deserialize)]
        struct GraphSummary {
            node_count: usize,
            max_depth: usize,
            strategy: String,
        }

        #[derive(Debug, Clone, Serialize, Deserialize)]
        struct ModelCost {
            model: String,
            cost: crate::types::NanoUSD,
            tokens: u64,
        }

        #[derive(Debug, Clone, Serialize, Deserialize)]
        struct TimingInfo {
            total_ms: u64,
            per_node: HashMap<String, u64>,
        }

        #[derive(Debug, Clone, Serialize, Deserialize)]
        struct Usage {
            prompt_tokens: u64,
            completion_tokens: u64,
            total_tokens: u64,
        }

        #[derive(Debug, Clone, Serialize, Deserialize)]
        struct ExecutionReport {
            graph: GraphSummary,
            costs: Vec<ModelCost>,
            timing: TimingInfo,
            model_breakdown: HashMap<String, Usage>,
            decisions: Vec<String>,
        }
        let report = ExecutionReport {
            graph: GraphSummary {
                node_count: 5,
                max_depth: 3,
                strategy: "quality".to_string(),
            },
            costs: vec![ModelCost {
                model: "claude-sonnet".to_string(),
                cost: crate::types::NanoUSD::from_nanos(50_000_000),
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
            "execution": {"mode": "constrained", "max_cost": 10000000, "max_latency_ms": 1000, "max_tokens": 100, "min_confidence": 0.5}
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
            model_requirements: None,
            requested_strategy: None,
        };
        reqs.execution_intent = request.execution.clone();
        reqs.output_preferences = request.output.clone();

        match reqs.execution_intent {
            Some(ExecutionIntent::Constrained { max_cost, .. }) => {
                assert_eq!(max_cost, Some(fusion_core::NanoUSD::from_nanos(10_000_000)));
            }
            _ => panic!("Expected Constrained variant"),
        }
        assert!(reqs.output_preferences.is_none());
    }
}
