//! ABI generator (v0.13 contract 3): `ExecutionGraph` → `ExecutionAbi`.
//!
//! The ABI is provider-free and deterministic: same graph, same ABI.

use crate::abi::{
    AbiConstraints, AbiEdgeKind, AbiRetryPolicy, ExecutionAbi, ExecutionAbiNode,
    EXECUTION_ABI_VERSION,
};
use crate::types::{
    ExecutionEdge, ExecutionNodeKind,
};
use serde_json::Value;
use std::collections::HashMap;

/// Capability identifier for a node kind (provider-free capability naming).
pub fn kind_capability(kind: &ExecutionNodeKind) -> &'static str {
    match kind {
        ExecutionNodeKind::LLMGenerate => "llm.generate",
        ExecutionNodeKind::LLMReview => "llm.review",
        ExecutionNodeKind::LLMJudge => "llm.judge",
        ExecutionNodeKind::Transform => "data.transform",
        ExecutionNodeKind::Gate => "control.gate",
        ExecutionNodeKind::Conditional => "control.conditional",
        ExecutionNodeKind::Loop => "control.loop",
        ExecutionNodeKind::Split => "control.split",
        ExecutionNodeKind::Join => "control.join",
        ExecutionNodeKind::Barrier => "control.barrier",
    }
}

fn u64_from(config: &HashMap<String, Value>, key: &str) -> Option<u64> {
    config.get(key).and_then(|v| v.as_u64())
}

fn nanousd_from(config: &HashMap<String, Value>, key: &str) -> Option<fusion_core::NanoUSD> {
    config.get(key).and_then(|v| {
        if let Some(n) = v.as_u64() {
            Some(fusion_core::NanoUSD::from_nanos(n))
        } else if let Some(s) = v.as_str() {
            fusion_core::NanoUSD::checked_from_decimal_usd(s).ok()
        } else {
            None
        }
    })
}

/// Converts a compiled `ExecutionGraph` into the frozen `ExecutionAbi` contract.
pub fn abi_from_graph(graph: &crate::types::ExecutionGraph) -> ExecutionAbi {
    let incoming: HashMap<uuid::Uuid, Vec<String>> = graph
        .edges
        .iter()
        .fold(HashMap::new(), |mut acc, e| {
            acc.entry(e.to).or_default().push(e.from.to_string());
            acc
        });
    let outgoing: HashMap<uuid::Uuid, Vec<String>> = graph
        .edges
        .iter()
        .fold(HashMap::new(), |mut acc, e| {
            acc.entry(e.from).or_default().push(e.to.to_string());
            acc
        });

    let nodes = graph
        .nodes
        .iter()
        .map(|node| {
            let capability = kind_capability(&node.kind).to_string();
            let constraints = AbiConstraints {
                max_latency_ms: u64_from(&node.config, "max_latency_ms"),
                max_cost: nanousd_from(&node.config, "max_cost"),
                max_tokens: u64_from(&node.config, "max_tokens"),
            };
            let retry_policy = if node.retry_policy.max_retries > 0 {
                Some(AbiRetryPolicy {
                    max_retries: node.retry_policy.max_retries,
                    backoff_ms: node.retry_policy.backoff_ms,
                })
            } else {
                None
            };
            ExecutionAbiNode {
                node_id: node.id.to_string(),
                role: format!("{:?}", node.kind),
                capability,
                inputs: incoming.get(&node.id).cloned().unwrap_or_default(),
                outputs: outgoing.get(&node.id).cloned().unwrap_or_default(),
                constraints,
                reasoning_budget: None,
                retry_policy,
                cache_policy: None,
                security_policy: None,
                evaluation_policy: None,
                telemetry_hooks: vec![],
            }
        })
        .collect();

    let edges: Vec<crate::abi::ExecutionAbiEdge> = graph
        .edges
        .iter()
        .map(|e: &ExecutionEdge| crate::abi::ExecutionAbiEdge {
            from: e.from.to_string(),
            to: e.to.to_string(),
            kind: if e.condition.is_some() {
                AbiEdgeKind::Conditional
            } else {
                AbiEdgeKind::Sequential
            },
            condition: e.condition.clone(),
        })
        .collect();

    ExecutionAbi {
        version: EXECUTION_ABI_VERSION,
        abi_id: graph.graph_id.to_string(),
        nodes,
        edges,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ExecutionNode, GraphMetadata, NanoUSD, RetryPolicy, StrategyKind};
    use uuid::Uuid;

    fn node(id: &str, kind: ExecutionNodeKind) -> ExecutionNode {
        ExecutionNode {
            id: Uuid::parse_str(id).unwrap(),
            kind,
            strategy: StrategyKind::Single,
            model: "mock".into(),
            retry_policy: RetryPolicy {
                max_retries: 2,
                backoff_ms: 50,
            },
            fallback: None,
            config: HashMap::new(),
            subgraph: None,
        }
    }

    fn sample_graph() -> crate::types::ExecutionGraph {
        let n1 = node("550e8400-e29b-41d4-a716-446655440001", ExecutionNodeKind::LLMGenerate);
        let n2 = node("550e8400-e29b-41d4-a716-446655440002", ExecutionNodeKind::Transform);
        crate::types::ExecutionGraph {
            graph_id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440003").unwrap(),
            nodes: vec![n1.clone(), n2.clone()],
            edges: vec![ExecutionEdge {
                from: n1.id,
                to: n2.id,
                condition: None,
            }],
            metadata: GraphMetadata {
                estimated_cost: NanoUSD::ONE_DOLLAR,
                estimated_tokens: 10,
                max_depth: 2,
                node_count: 2,
            },
            total_tokens: 10,
            total_cost: NanoUSD::ONE_DOLLAR,
            primitive_graph_hash: 0,
        }
    }

    #[test]
    fn generator_is_deterministic() {
        let a = abi_from_graph(&sample_graph());
        let b = abi_from_graph(&sample_graph());
        assert_eq!(a.abi_id, b.abi_id);
        assert_eq!(a.nodes.len(), 2);
        assert_eq!(a.edges.len(), 1);
        assert_eq!(a.nodes[0].node_id, b.nodes[0].node_id);
        assert_eq!(a.edges[0].from, b.edges[0].from);
    }

    #[test]
    fn provider_free_and_policy_carried() {
        let abi = abi_from_graph(&sample_graph());
        assert!(abi.version > 0);
        assert_eq!(abi.nodes[0].capability, "llm.generate");
        assert_eq!(abi.nodes[1].capability, "data.transform");
        let retry = abi.nodes[0].retry_policy.as_ref().unwrap();
        assert_eq!(retry.max_retries, 2);
        assert_eq!(abi.edges[0].kind, AbiEdgeKind::Sequential);
    }

    #[test]
    fn one_to_one_edge_mapping_preserves_ids() {
        let abi = abi_from_graph(&sample_graph());
        assert_eq!(
            abi.edges[0].from,
            "550e8400-e29b-41d4-a716-446655440001"
        );
        assert_eq!(
            abi.edges[0].to,
            "550e8400-e29b-41d4-a716-446655440002"
        );
        assert_eq!(abi.nodes[0].outputs.len(), 1);
        assert_eq!(abi.nodes[1].inputs.len(), 1);
    }
}