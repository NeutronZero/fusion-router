//! Phase 2A — `CapabilityGraph` (`src/planner/resolver/capability/graph.rs`)
//!
//! Represents inter-capability dependencies and conflicts as a validated DAG.

use std::collections::{HashMap, VecDeque};
use fusion_plugin_api::{CapabilityContract, CapabilityId};

#[derive(Debug, Clone)]
pub struct CapabilityNode {
    pub contract: CapabilityContract,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DependencyEdge {
    pub from: CapabilityId,
    pub to: CapabilityId,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConflictEdge {
    pub capability_a: CapabilityId,
    pub capability_b: CapabilityId,
}

#[derive(Debug, Clone)]
pub struct CapabilityGraph {
    nodes: HashMap<CapabilityId, CapabilityNode>,
    dependencies: Vec<DependencyEdge>,
    conflicts: Vec<ConflictEdge>,
}

impl CapabilityGraph {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            dependencies: Vec::new(),
            conflicts: Vec::new(),
        }
    }
    pub fn nodes(&self) -> &HashMap<CapabilityId, CapabilityNode> {
        &self.nodes
    }

    pub fn dependencies(&self) -> &Vec<DependencyEdge> {
        &self.dependencies
    }

    pub fn conflicts(&self) -> &Vec<ConflictEdge> {
        &self.conflicts
    }

    /// Adds a capability node to the graph.
    pub fn add_node(&mut self, contract: CapabilityContract) {
        let id = contract.id.clone();
        self.nodes.insert(id, CapabilityNode { contract });
    }

    /// Adds a dependency edge: `from` capability depends on `to` capability.
    pub fn add_dependency(&mut self, from: CapabilityId, to: CapabilityId) {
        self.dependencies.push(DependencyEdge { from, to });
    }

    /// Adds a conflict declaration between two capabilities.
    pub fn add_conflict(&mut self, capability_a: CapabilityId, capability_b: CapabilityId) {
        self.conflicts.push(ConflictEdge {
            capability_a,
            capability_b,
        });
    }

    /// Validates the graph for cycles and conflict violations.
    pub fn validate(&self) -> Result<(), String> {
        // 1. Conflict Validation
        for conflict in &self.conflicts {
            if self.nodes.contains_key(&conflict.capability_a)
                && self.nodes.contains_key(&conflict.capability_b)
            {
                return Err(format!(
                    "Capability conflict detected between '{}' and '{}'",
                    conflict.capability_a, conflict.capability_b
                ));
            }
        }

        // 2. Missing Dependency Check
        for dep in &self.dependencies {
            if self.nodes.contains_key(&dep.from) && !self.nodes.contains_key(&dep.to) {
                return Err(format!(
                    "Unresolved capability dependency: '{}' requires missing capability '{}'",
                    dep.from, dep.to
                ));
            }
        }

        // 3. Cycle Detection via Topological Sort (Kahn's Algorithm)
        self.topological_sort().map(|_| ())
    }

    /// Computes topological ordering of capabilities in dependency order.
    pub fn topological_sort(&self) -> Result<Vec<CapabilityId>, String> {
        let mut in_degree: HashMap<CapabilityId, usize> = HashMap::new();
        let mut adj: HashMap<CapabilityId, Vec<CapabilityId>> = HashMap::new();

        for id in self.nodes.keys() {
            in_degree.insert(id.clone(), 0);
            adj.insert(id.clone(), Vec::new());
        }

        for dep in &self.dependencies {
            if self.nodes.contains_key(&dep.from) && self.nodes.contains_key(&dep.to) {
                // edge dep.to -> dep.from (to must execute before from)
                adj.entry(dep.to.clone()).or_default().push(dep.from.clone());
                *in_degree.entry(dep.from.clone()).or_default() += 1;
            }
        }

        let mut queue: VecDeque<CapabilityId> = VecDeque::new();
        for (id, &deg) in &in_degree {
            if deg == 0 {
                queue.push_back(id.clone());
            }
        }

        let mut order = Vec::new();
        while let Some(curr) = queue.pop_front() {
            order.push(curr.clone());
            if let Some(neighbors) = adj.get(&curr) {
                for neighbor in neighbors {
                    if let Some(deg) = in_degree.get_mut(neighbor) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(neighbor.clone());
                        }
                    }
                }
            }
        }

        if order.len() != self.nodes.len() {
            return Err("Cyclic dependency detected in CapabilityGraph".into());
        }

        Ok(order)
    }

    pub fn get_node(&self, id: &CapabilityId) -> Option<&CapabilityNode> {
        self.nodes.get(id)
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
}

impl Default for CapabilityGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_contract(id: &str) -> CapabilityContract {
        CapabilityContract {
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
    fn test_topological_sort_valid_dag() {
        let mut graph = CapabilityGraph::new();
        graph.add_node(make_contract("browser"));
        graph.add_node(make_contract("filesystem"));
        graph.add_node(make_contract("shell"));

        // browser -> filesystem -> shell
        graph.add_dependency(CapabilityId::new("browser"), CapabilityId::new("filesystem"));
        graph.add_dependency(CapabilityId::new("filesystem"), CapabilityId::new("shell"));

        let order = graph.topological_sort().unwrap();
        assert_eq!(order.len(), 3);
        let shell_idx = order.iter().position(|r| r.as_str() == "shell").unwrap();
        let fs_idx = order.iter().position(|r| r.as_str() == "filesystem").unwrap();
        let browser_idx = order.iter().position(|r| r.as_str() == "browser").unwrap();

        assert!(shell_idx < fs_idx);
        assert!(fs_idx < browser_idx);
    }

    #[test]
    fn test_cycle_detection() {
        let mut graph = CapabilityGraph::new();
        graph.add_node(make_contract("a"));
        graph.add_node(make_contract("b"));

        graph.add_dependency(CapabilityId::new("a"), CapabilityId::new("b"));
        graph.add_dependency(CapabilityId::new("b"), CapabilityId::new("a"));

        assert!(graph.validate().is_err());
    }

    #[test]
    fn test_conflict_detection() {
        let mut graph = CapabilityGraph::new();
        graph.add_node(make_contract("plugin_a"));
        graph.add_node(make_contract("plugin_b"));

        graph.add_conflict(CapabilityId::new("plugin_a"), CapabilityId::new("plugin_b"));

        assert!(graph.validate().is_err());
    }
}
