//! ABI→graph converter for runtime binding (v0.13 contract 3 → live v0.12 types).
//!
//! The ABI is provider-free by contract; the runtime binds providers. This
//! converter therefore requires an explicit `model` string from the caller
//! (the execution runtime) and binds it to every node.

use crate::abi::{AbiEdgeKind, ExecutionAbi};
use crate::ir::adapter::uuid_for;
use crate::types::{
    ExecutionEdge, ExecutionGraph, ExecutionNode, ExecutionNodeKind, GraphMetadata, NanoUSD, RetryPolicy,
    StrategyKind,
};
use serde_json::Value;
use std::collections::HashMap;

/// Maps an ABI `role` string back onto the execution node kind.
///
/// Fails closed: unknown roles are rejected rather than silently downgraded.
pub fn kind_from_role(role: &str) -> Result<ExecutionNodeKind, String> {
    match role {
        "LLMGenerate" => Ok(ExecutionNodeKind::LLMGenerate),
        "LLMReview" => Ok(ExecutionNodeKind::LLMReview),
        "LLMJudge" => Ok(ExecutionNodeKind::LLMJudge),
        "Transform" => Ok(ExecutionNodeKind::Transform),
        "Gate" => Ok(ExecutionNodeKind::Gate),
        "Conditional" => Ok(ExecutionNodeKind::Conditional),
        "Loop" => Ok(ExecutionNodeKind::Loop),
        "Split" => Ok(ExecutionNodeKind::Split),
        "Join" => Ok(ExecutionNodeKind::Join),
        "Barrier" => Ok(ExecutionNodeKind::Barrier),
        other => Err(format!(
            "ABI role '{other}' is not a known execution node kind"
        )),
    }
}

/// Converts a frozen `ExecutionAbi` into an executable `ExecutionGraph`,
/// binding every node to the caller-supplied `model`.
pub fn graph_from_abi(abi: &ExecutionAbi, model: &str) -> Result<ExecutionGraph, String> {
    let mut nodes = Vec::with_capacity(abi.nodes.len());
    for node in &abi.nodes {
        let kind = kind_from_role(&node.role)?;
        let mut config = HashMap::new();
        config.insert("capability".into(), Value::String(node.capability.clone()));
        let mut retry = RetryPolicy {
            max_retries: 0,
            backoff_ms: 0,
        };
        if let Some(policy) = &node.retry_policy {
            retry = RetryPolicy {
                max_retries: policy.max_retries,
                backoff_ms: policy.backoff_ms,
            };
        }
        nodes.push(ExecutionNode {
            id: uuid_for(&node.node_id),
            kind,
            strategy: StrategyKind::Single,
            model: model.to_string(),
            retry_policy: retry,
            fallback: None,
            config,
            subgraph: None,
        });
    }

    let node_ids: std::collections::HashSet<uuid::Uuid> = nodes.iter().map(|n| n.id).collect();
    let mut edges = Vec::with_capacity(abi.edges.len());
    for edge in &abi.edges {
        let from = uuid_for(&edge.from);
        let to = uuid_for(&edge.to);
        if !node_ids.contains(&from) || !node_ids.contains(&to) {
            return Err(format!(
                "ABI edge references unknown node ({from} -> {to})"
            ));
        }
        edges.push(ExecutionEdge {
            from,
            to,
            condition: match edge.kind {
                AbiEdgeKind::Conditional => edge.condition.clone(),
                _ => None,
            },
        });
    }

    let node_count = nodes.len() as u32;
    Ok(ExecutionGraph {
        graph_id: uuid_for(&abi.abi_id),
        nodes,
        edges,
        metadata: GraphMetadata {
            estimated_cost: NanoUSD::ZERO,
            estimated_tokens: 0,
            max_depth: 0,
            node_count,
        },
        total_tokens: 0,
        total_cost: NanoUSD::ZERO,
        primitive_graph_hash: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::from_graph::abi_from_graph;

    fn round_trip_abi() -> ExecutionAbi {
        let mut graph = crate::types::ExecutionGraph {
            graph_id: uuid::Uuid::new_v4(),
            nodes: vec![ExecutionNode {
                id: uuid::Uuid::new_v4(),
                kind: ExecutionNodeKind::LLMGenerate,
                strategy: StrategyKind::Single,
                model: "mock".into(),
                retry_policy: RetryPolicy {
                    max_retries: 1,
                    backoff_ms: 10,
                },
                fallback: None,
                config: HashMap::new(),
                subgraph: None,
            }],
            edges: vec![],
            metadata: GraphMetadata {
                estimated_cost: NanoUSD::ZERO,
                estimated_tokens: 0,
                max_depth: 1,
                node_count: 1,
            },
            total_tokens: 0,
            total_cost: NanoUSD::ZERO,
            primitive_graph_hash: 0,
        };
        let nid = graph.nodes[0].id;
        graph.nodes[0].config.insert(
            "max_tokens".into(),
            Value::Number(2048.into()),
        );
        let mut abi = abi_from_graph(&graph);
        abi.edges = vec![crate::abi::ExecutionAbiEdge {
            from: nid.to_string(),
            to: nid.to_string(),
            kind: AbiEdgeKind::Sequential,
            condition: None,
        }];
        abi
    }

    #[test]
    fn binds_model_and_preserves_identity() {
        let abi = round_trip_abi();
        let graph = graph_from_abi(&abi, "bound-model").unwrap();
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.nodes[0].model, "bound-model");
        assert_eq!(graph.nodes[0].id.to_string(), abi.nodes[0].node_id);
        assert_eq!(graph.graph_id.to_string(), abi.abi_id);
        assert_eq!(graph.nodes[0].retry_policy.max_retries, 1);
        assert_eq!(graph.edges.len(), 1);
    }

    #[test]
    fn rejects_unknown_role() {
        let mut abi = round_trip_abi();
        abi.nodes[0].role = "mystery".into();
        assert!(graph_from_abi(&abi, "m").is_err());
    }

    #[test]
    fn rejects_edge_to_unknown_node() {
        let abi = round_trip_abi();
        let mut abi2 = abi.clone();
        abi2.edges = vec![crate::abi::ExecutionAbiEdge {
            from: "ghost".into(),
            to: abi.nodes[0].node_id.clone(),
            kind: AbiEdgeKind::Sequential,
            condition: None,
        }];
        assert!(graph_from_abi(&abi2, "m").is_err());
    }

    #[test]
    fn capability_round_trips_through_config() {
        let abi = round_trip_abi();
        let graph = graph_from_abi(&abi, "m").unwrap();
        assert_eq!(
            graph.nodes[0].config["capability"],
            Value::String("llm.generate".into())
        );
    }
}