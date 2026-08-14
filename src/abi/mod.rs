use serde::{Deserialize, Serialize};

pub mod from_graph;
pub mod to_graph;

pub const EXECUTION_ABI_VERSION: u16 = 1;

/// Stable executable workflow contract between compiler and runtime (v0.13 contract 3).
/// Provider-free: nodes reference capabilities, never models.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionAbi {
    pub version: u16,
    pub abi_id: String,
    pub nodes: Vec<ExecutionAbiNode>,
    pub edges: Vec<ExecutionAbiEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionAbiNode {
    pub node_id: String,
    pub role: String,
    pub capability: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub constraints: AbiConstraints,
    pub reasoning_budget: Option<ReasoningBudget>,
    pub retry_policy: Option<AbiRetryPolicy>,
    pub cache_policy: Option<CachePolicy>,
    pub security_policy: Option<SecurityPolicy>,
    pub evaluation_policy: Option<EvaluationPolicy>,
    pub telemetry_hooks: Vec<TelemetryHook>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AbiConstraints {
    pub max_latency_ms: Option<u64>,
    #[serde(default)]
    pub max_cost: Option<fusion_core::NanoUSD>,
    pub max_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReasoningBudget {
    pub max_tokens: Option<u64>,
    pub max_steps: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AbiRetryPolicy {
    pub max_retries: u32,
    pub backoff_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CachePolicy {
    pub ttl_secs: Option<u64>,
    pub key_hint: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SecurityPolicy {
    pub sandbox_required: bool,
    pub validation_required: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EvaluationPolicy {
    pub faithfulness: bool,
    pub relevance: bool,
    pub tool_correctness: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TelemetryHook {
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionAbiEdge {
    pub from: String,
    pub to: String,
    pub kind: AbiEdgeKind,
    pub condition: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AbiEdgeKind {
    Sequential,
    Parallel,
    Conditional,
    Retry,
    Merge,
    Loop,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_abi() -> ExecutionAbi {
        ExecutionAbi {
            version: EXECUTION_ABI_VERSION,
            abi_id: "abi-1".into(),
            nodes: vec![ExecutionAbiNode {
                node_id: "n1".into(),
                role: "generator".into(),
                capability: "CodeGeneration".into(),
                inputs: vec!["req".into()],
                outputs: vec!["code".into()],
                constraints: AbiConstraints {
                    max_tokens: Some(4096),
                    ..AbiConstraints::default()
                },
                reasoning_budget: Some(ReasoningBudget {
                    max_tokens: Some(2048),
                    max_steps: Some(4),
                }),
                retry_policy: Some(AbiRetryPolicy {
                    max_retries: 2,
                    backoff_ms: 100,
                }),
                cache_policy: Some(CachePolicy {
                    ttl_secs: Some(60),
                    key_hint: None,
                }),
                security_policy: Some(SecurityPolicy {
                    sandbox_required: true,
                    ..SecurityPolicy::default()
                }),
                evaluation_policy: Some(EvaluationPolicy {
                    faithfulness: true,
                    ..EvaluationPolicy::default()
                }),
                telemetry_hooks: vec![TelemetryHook {
                    name: "latency".into(),
                    enabled: true,
                }],
            }],
            edges: vec![ExecutionAbiEdge {
                from: "n1".into(),
                to: "n1".into(),
                kind: AbiEdgeKind::Sequential,
                condition: None,
            }],
        }
    }

    #[test]
    fn version_is_one() {
        assert_eq!(EXECUTION_ABI_VERSION, 1);
        assert_eq!(sample_abi().version, 1);
    }

    #[test]
    fn serde_round_trip_preserves_node_contract() {
        let abi = sample_abi();
        let json = serde_json::to_string(&abi).unwrap();
        let back: ExecutionAbi = serde_json::from_str(&json).unwrap();
        let node = &back.nodes[0];
        assert_eq!(node.node_id, "n1");
        assert_eq!(node.capability, "CodeGeneration");
        assert_eq!(node.retry_policy.as_ref().unwrap().max_retries, 2);
        assert!(node.security_policy.as_ref().unwrap().sandbox_required);
        assert_eq!(node.telemetry_hooks.len(), 1);
        assert_eq!(back.edges[0].kind, AbiEdgeKind::Sequential);
    }

    #[test]
    fn node_rejects_provider_fields() {
        let node_json = r#"{"node_id":"n1","role":"generator","capability":"CodeGeneration","inputs":[],"outputs":[],"constraints":{},"reasoning_budget":null,"retry_policy":null,"cache_policy":null,"security_policy":null,"evaluation_policy":null,"telemetry_hooks":[],"model":"gpt-4"}"#;
        assert!(serde_json::from_str::<ExecutionAbiNode>(node_json).is_err());
    }

    #[test]
    fn all_edge_kinds_round_trip() {
        for kind in [
            AbiEdgeKind::Sequential,
            AbiEdgeKind::Parallel,
            AbiEdgeKind::Conditional,
            AbiEdgeKind::Retry,
            AbiEdgeKind::Merge,
            AbiEdgeKind::Loop,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            let back: AbiEdgeKind = serde_json::from_str(&json).unwrap();
            assert_eq!(back, kind);
        }
    }

    #[test]
    fn policy_defaults() {
        let s = SecurityPolicy::default();
        assert!(!s.sandbox_required && !s.validation_required);
        let e = EvaluationPolicy::default();
        assert!(!e.faithfulness && !e.relevance && !e.tool_correctness);
    }
}
