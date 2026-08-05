//! Deterministic lowering from `CapabilityGraph` to `ExecutionGraph`.
//!
//! This is a compiler transformation, not an intrinsic graph operation.
//! Keeping it as a separate type allows future optimization passes,
//! instrumentation insertion, and scheduling hints.

use std::collections::HashMap;
use uuid::Uuid;
use fusion_plugin_api::CapabilityId;
use crate::types::{
    ExecutionEdge, ExecutionGraph, ExecutionNode, ExecutionNodeKind,
    GraphMetadata, RetryPolicy, StrategyKind,
};
use super::CapabilityGraph;

/// Lowering component: `CapabilityGraph` → `ExecutionGraph`.
///
/// Deterministic: identical input produces identical output.
pub struct CapabilityGraphLowerer;

impl CapabilityGraphLowerer {
    /// Lowers a `CapabilityGraph` into the compiler's `ExecutionGraph`.
    ///
    /// Each capability node becomes a `Gate` execution node.
    /// Dependency edges become execution edges preserving topological order.
    pub fn lower(&self, cap_graph: &CapabilityGraph) -> ExecutionGraph {
        // Deterministic topological ordering
        let order = match cap_graph.topological_sort() {
            Ok(order) => order,
            Err(_) => return ExecutionGraph {
                graph_id: Uuid::nil(),
                nodes: Vec::new(),
                edges: Vec::new(),
                metadata: GraphMetadata {
                    estimated_cost: 0.0,
                    estimated_tokens: 0,
                    max_depth: 0,
                    node_count: 0,
                },
                total_tokens: 0,
                total_cost: 0,
                primitive_graph_hash: 0,
            },
        };

        let mut id_map: HashMap<CapabilityId, Uuid> = HashMap::new();
        let mut nodes = Vec::new();
        // Compute DAG depth via DP on topological order
        let mut depths: HashMap<CapabilityId, u32> = HashMap::new();
        for cap_id in &order {
            let mut d = 1u32;
            for dep in cap_graph.dependencies() {
                if dep.from == *cap_id {
                    if let Some(&pred_depth) = depths.get(&dep.to) {
                        d = d.max(1 + pred_depth);
                    }
                }
            }
            depths.insert(cap_id.clone(), d);
        }
        let max_depth = depths.values().max().copied().unwrap_or(0);

        let mut total_cost: u64 = 0;
        let total_tokens: u64 = 0;

        for cap_id in &order {
            let node_id = deterministic_uuid(cap_id);
            id_map.insert(cap_id.clone(), node_id);

            let node = cap_graph.get_node(cap_id).expect("node from topological sort must exist");
            total_cost += (node.contract.estimated_cost_usd * 1000.0) as u64;

            let mut config = std::collections::HashMap::new();
            config.insert("capability_id".into(), serde_json::json!(cap_id.as_str()));
            config.insert("description".into(), serde_json::json!(node.contract.description));
            if !node.contract.permissions.is_empty() {
                config.insert("permissions".into(), serde_json::json!(node.contract.permissions));
            }

            nodes.push(ExecutionNode {
                id: node_id,
                kind: ExecutionNodeKind::Gate,
                strategy: StrategyKind::Single,
                model: String::new(),
                retry_policy: RetryPolicy {
                    max_retries: 2,
                    backoff_ms: 1000,
                },
                fallback: None,
                config,
                subgraph: None,
            });
        }

        let mut edges = Vec::new();
        for dep in cap_graph.dependencies() {
            if let (Some(&from_id), Some(&to_id)) = (id_map.get(&dep.from), id_map.get(&dep.to)) {
                edges.push(ExecutionEdge {
                    from: from_id,
                    to: to_id,
                    condition: None,
                });
            }
        }

        ExecutionGraph {
            graph_id: deterministic_graph_uuid(&order),
            nodes,
            edges,
            metadata: GraphMetadata {
                estimated_cost: (total_cost as f64) / 1000.0,
                estimated_tokens: 0,
                max_depth,
                node_count: cap_graph.node_count() as u32,
            },
            total_tokens,
            total_cost,
            primitive_graph_hash: 0,
        }
    }
}

/// Deterministic UUID from a `CapabilityId` string.
/// Uses UUID v5 with a fixed namespace so the same ID always produces the same UUID.
fn deterministic_uuid(cap_id: &CapabilityId) -> Uuid {
    const CAPABILITY_NAMESPACE: Uuid = Uuid::from_u128(0x6ba7b810_9dad_11d1_80b4_00c04fd430c8);
    Uuid::new_v5(&CAPABILITY_NAMESPACE, cap_id.as_str().as_bytes())
}

/// Deterministic graph UUID from an ordered list of capability IDs.
fn deterministic_graph_uuid(order: &[CapabilityId]) -> Uuid {
    const GRAPH_NAMESPACE: Uuid = Uuid::from_u128(0x6ba7b811_9dad_11d1_80b4_00c04fd430c8);
    let mut bytes = Vec::new();
    for id in order {
        bytes.extend_from_slice(id.as_str().as_bytes());
        bytes.push(0);
    }
    Uuid::new_v5(&GRAPH_NAMESPACE, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_contract(id: &str) -> fusion_plugin_api::CapabilityContract {
        fusion_plugin_api::CapabilityContract {
            id: CapabilityId::new(id),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: format!("Test {}", id),
            inputs_schema: json!({}),
            outputs_schema: json!({}),
            permissions: vec![],
            dependencies: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 1,
            reliability_score: 1.0,
            supports_streaming: false,
            traits: vec![],
        }
    }

    #[test]
    fn lowering_produces_deterministic_output() {
        let mut graph = CapabilityGraph::new();
        graph.add_node(make_contract("alpha"));
        graph.add_node(make_contract("beta"));
        graph.add_dependency(CapabilityId::new("alpha"), CapabilityId::new("beta"));

        let lowerer = CapabilityGraphLowerer;
        let result_a = lowerer.lower(&graph);
        let result_b = lowerer.lower(&graph);

        assert_eq!(result_a.graph_id, result_b.graph_id);
        assert_eq!(result_a.nodes.len(), result_b.nodes.len());
        for (a, b) in result_a.nodes.iter().zip(result_b.nodes.iter()) {
            assert_eq!(a.id, b.id);
        }
    }

    #[test]
    fn lowering_preserves_topological_order() {
        let mut graph = CapabilityGraph::new();
        graph.add_node(make_contract("shell"));
        graph.add_node(make_contract("filesystem"));
        graph.add_node(make_contract("browser"));
        // browser -> filesystem -> shell
        graph.add_dependency(CapabilityId::new("browser"), CapabilityId::new("filesystem"));
        graph.add_dependency(CapabilityId::new("filesystem"), CapabilityId::new("shell"));

        let lowerer = CapabilityGraphLowerer;
        let exec_graph = lowerer.lower(&graph);

        let positions: HashMap<&str, usize> = exec_graph.nodes.iter().enumerate().map(|(i, n)| {
            let cap_id = n.config.get("capability_id").and_then(|v| v.as_str()).unwrap_or("");
            (cap_id, i)
        }).collect();

        assert!(positions.get("shell").unwrap() < positions.get("filesystem").unwrap());
        assert!(positions.get("filesystem").unwrap() < positions.get("browser").unwrap());
    }

    #[test]
    fn empty_graph_lowers() {
        let graph = CapabilityGraph::new();
        let lowerer = CapabilityGraphLowerer;
        let exec_graph = lowerer.lower(&graph);

        assert!(exec_graph.nodes.is_empty());
        assert!(exec_graph.edges.is_empty());
        assert_eq!(exec_graph.metadata.node_count, 0);
        assert_eq!(exec_graph.total_cost, 0);
        assert_eq!(exec_graph.total_tokens, 0);
    }
}
