use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::types::{ExecutionGraph, ExecutionNode, NodeState};

pub struct WorkQueue {
    graph: ExecutionGraph,
    completed: HashSet<Uuid>,
    in_progress: HashSet<Uuid>,
    failed: HashSet<Uuid>,
    incoming: HashMap<Uuid, Vec<Uuid>>,
    activated_edges: HashSet<(Uuid, Uuid)>,
}

impl WorkQueue {
    pub fn new(graph: ExecutionGraph) -> Self {
        let mut incoming: HashMap<Uuid, Vec<Uuid>> = HashMap::new();

        for node in &graph.nodes {
            incoming.entry(node.id).or_default();
        }

        for edge in &graph.edges {
            incoming.entry(edge.to).or_default().push(edge.from);
        }

        Self {
            graph,
            completed: HashSet::new(),
            in_progress: HashSet::new(),
            failed: HashSet::new(),
            incoming,
            activated_edges: HashSet::new(),
        }
    }

    pub fn get_ready(&self, node_states: &HashMap<Uuid, NodeState>) -> Vec<&ExecutionNode> {
        self.graph
            .nodes
            .iter()
            .filter(|node| {
                let id = node.id;
                if self.completed.contains(&id)
                    || self.in_progress.contains(&id)
                    || self.failed.contains(&id)
                {
                    return false;
                }
                if let Some(state) = node_states.get(&id) {
                    match state {
                        NodeState::Succeeded | NodeState::Failed(_) | NodeState::Skipped => {
                            return false;
                        }
                        _ => {}
                    }
                }
                let deps = self.incoming.get(&id).map(|v| v.as_slice()).unwrap_or(&[]);
                deps.iter().all(|dep_id| {
                    if self.completed.contains(dep_id) {
                        self.activated_edges.contains(&(*dep_id, id))
                    } else {
                        false
                    }
                })
            })
            .collect()
    }

    pub fn mark_completed(&mut self, node_id: Uuid) {
        self.completed.insert(node_id);
        self.in_progress.remove(&node_id);
        for edge in &self.graph.edges {
            if edge.from == node_id && edge.condition.as_deref() != Some("loop") {
                self.activated_edges.insert((edge.from, edge.to));
            }
        }
    }

    pub fn mark_conditional_completed(&mut self, node_id: Uuid) {
        self.completed.insert(node_id);
        self.in_progress.remove(&node_id);
    }

    pub fn activate_edge(&mut self, from: Uuid, to: Uuid) {
        self.activated_edges.insert((from, to));
    }

    pub fn mark_failed(&mut self, node_id: Uuid) {
        self.failed.insert(node_id);
        self.in_progress.remove(&node_id);
    }

    pub fn mark_in_progress(&mut self, node_id: Uuid) {
        self.in_progress.insert(node_id);
    }

    pub fn reset_ready(&mut self, node_id: Uuid) {
        self.in_progress.remove(&node_id);
        self.failed.remove(&node_id);
    }

    pub fn reset_loop_body(&mut self, body_ids: &[Uuid]) {
        for id in body_ids {
            self.completed.remove(id);
            self.in_progress.remove(id);
            self.failed.remove(id);
            self.activated_edges.retain(|(from, _to)| from != id);
        }
    }

    pub fn is_done(&self, node_states: &HashMap<Uuid, NodeState>) -> bool {
        self.graph.nodes.iter().all(|node| {
            let id = node.id;
            self.completed.contains(&id)
                || self.failed.contains(&id)
                || matches!(
                    node_states.get(&id),
                    Some(NodeState::Succeeded | NodeState::Failed(_) | NodeState::Skipped)
                )
        })
    }

    pub fn graph(&self) -> &ExecutionGraph {
        &self.graph
    }

    pub fn outgoing_edges(&self, node_id: Uuid) -> Vec<&crate::types::ExecutionEdge> {
        self.graph.edges.iter().filter(|e| e.from == node_id).collect()
    }

    pub fn incoming_edges(&self, node_id: Uuid) -> Vec<&crate::types::ExecutionEdge> {
        self.graph.edges.iter().filter(|e| e.to == node_id).collect()
    }

    pub fn has_loop_back_edge(&self, node_id: Uuid) -> bool {
        self.graph.edges.iter().any(|e| {
            e.from == node_id && e.condition.as_deref() == Some("loop")
        })
    }

    pub fn loop_back_target(&self, node_id: Uuid) -> Option<Uuid> {
        self.graph.edges.iter()
            .find(|e| e.from == node_id && e.condition.as_deref() == Some("loop"))
            .map(|e| e.to)
    }
}
