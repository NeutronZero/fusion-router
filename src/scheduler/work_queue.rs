use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::types::{ExecutionGraph, ExecutionNode, NodeState};

pub struct WorkQueue {
    graph: ExecutionGraph,
    completed: HashSet<Uuid>,
    in_progress: HashSet<Uuid>,
    failed: HashSet<Uuid>,
    incoming: HashMap<Uuid, Vec<Uuid>>,
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
                deps.iter().all(|dep_id| self.completed.contains(dep_id))
            })
            .collect()
    }

    pub fn mark_completed(&mut self, node_id: Uuid) {
        self.completed.insert(node_id);
        self.in_progress.remove(&node_id);
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
}
