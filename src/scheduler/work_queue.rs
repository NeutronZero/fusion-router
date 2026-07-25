use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::types::{ExecutionEdge, ExecutionGraph, ExecutionNode, NodeState};

pub struct WorkQueue {
    graph: ExecutionGraph,
    completed: HashSet<Uuid>,
    in_progress: HashSet<Uuid>,
    failed: HashSet<Uuid>,
    ready: HashSet<Uuid>,
    outgoing: HashMap<Uuid, Vec<(Uuid, Option<String>)>>,
    total_incoming: HashMap<Uuid, usize>,
    satisfied_incoming: HashMap<Uuid, usize>,
    activated_edges: HashSet<(Uuid, Uuid)>,
}

impl WorkQueue {
    pub fn new(graph: ExecutionGraph) -> Self {
        let mut outgoing: HashMap<Uuid, Vec<(Uuid, Option<String>)>> = HashMap::new();
        let mut total_incoming: HashMap<Uuid, usize> = HashMap::new();

        let mut loop_node_ids: HashSet<Uuid> = HashSet::new();
        for node in &graph.nodes {
            if matches!(node.kind, crate::types::ExecutionNodeKind::Loop) {
                loop_node_ids.insert(node.id);
            }
            outgoing.entry(node.id).or_default();
            total_incoming.entry(node.id).or_insert(0);
        }

        for edge in &graph.edges {
            outgoing.entry(edge.from).or_default().push((edge.to, edge.condition.clone()));
            if !loop_node_ids.contains(&edge.to) {
                *total_incoming.entry(edge.to).or_insert(0) += 1;
            }
        }

        let mut ready = HashSet::new();
        for node in &graph.nodes {
            if total_incoming.get(&node.id).copied().unwrap_or(0) == 0 {
                ready.insert(node.id);
            }
        }

        Self {
            graph,
            completed: HashSet::new(),
            in_progress: HashSet::new(),
            failed: HashSet::new(),
            ready,
            outgoing,
            total_incoming,
            satisfied_incoming: HashMap::new(),
            activated_edges: HashSet::new(),
        }
    }

    fn try_activate_downstream(&mut self, from: Uuid) {
        let Some(edges) = self.outgoing.get(&from).cloned() else { return };
        for (to, _condition) in &edges {
            if !self.activated_edges.contains(&(from, *to)) {
                self.activated_edges.insert((from, *to));
                let satisfied = self.satisfied_incoming.entry(*to).or_insert(0);
                *satisfied += 1;
                let total = self.total_incoming.get(to).copied().unwrap_or(0);
                if *satisfied == total && !self.completed.contains(to) && !self.failed.contains(to) {
                    self.ready.insert(*to);
                }
            }
        }
    }

    pub fn get_ready(&self, node_states: &HashMap<Uuid, NodeState>) -> Vec<&ExecutionNode> {
        let mut result = Vec::with_capacity(self.ready.len());
        for node in &self.graph.nodes {
            if self.ready.contains(&node.id)
                && !self.completed.contains(&node.id)
                && !self.in_progress.contains(&node.id)
                && !self.failed.contains(&node.id)
            {
                if let Some(state) = node_states.get(&node.id) {
                    match state {
                        NodeState::Succeeded | NodeState::Failed(_) | NodeState::Skipped => continue,
                        _ => {}
                    }
                }
                result.push(node);
            }
        }
        result
    }

    pub fn mark_completed(&mut self, node_id: Uuid) {
        self.completed.insert(node_id);
        self.in_progress.remove(&node_id);
        self.ready.remove(&node_id);
        self.try_activate_downstream(node_id);
    }

    pub fn mark_conditional_completed(&mut self, node_id: Uuid) {
        self.completed.insert(node_id);
        self.in_progress.remove(&node_id);
        self.ready.remove(&node_id);
    }

    pub fn activate_edge(&mut self, from: Uuid, to: Uuid) {
        if self.activated_edges.contains(&(from, to)) {
            return;
        }
        self.activated_edges.insert((from, to));
        let satisfied = self.satisfied_incoming.entry(to).or_insert(0);
        *satisfied += 1;
        let total = self.total_incoming.get(&to).copied().unwrap_or(0);
        if *satisfied == total && !self.completed.contains(&to) && !self.failed.contains(&to) {
            self.ready.insert(to);
        }
    }

    pub fn mark_failed(&mut self, node_id: Uuid) {
        self.failed.insert(node_id);
        self.in_progress.remove(&node_id);
        self.ready.remove(&node_id);
    }

    pub fn mark_in_progress(&mut self, node_id: Uuid) {
        self.in_progress.insert(node_id);
        self.ready.remove(&node_id);
    }

    pub fn reset_ready(&mut self, node_id: Uuid) {
        self.in_progress.remove(&node_id);
        self.failed.remove(&node_id);
        let total = self.total_incoming.get(&node_id).copied().unwrap_or(0);
        let satisfied = self.satisfied_incoming.get(&node_id).copied().unwrap_or(0);
        if satisfied == total {
            self.ready.insert(node_id);
        }
    }

    pub fn reset_loop_body(&mut self, body_ids: &[Uuid]) {
        for id in body_ids {
            self.completed.remove(id);
            self.in_progress.remove(id);
            self.failed.remove(id);
            self.ready.remove(id);
            let total = self.total_incoming.get(id).copied().unwrap_or(0);
            let satisfied: usize = self.graph.edges.iter()
                .filter(|e| e.to == *id)
                .filter(|e| self.completed.contains(&e.from) && self.activated_edges.contains(&(e.from, e.to)))
                .count();
            self.satisfied_incoming.insert(*id, satisfied);
            if satisfied == total && satisfied > 0 {
                self.ready.insert(*id);
            }
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

    pub fn outgoing_edges(&self, node_id: Uuid) -> Vec<&ExecutionEdge> {
        self.graph.edges.iter().filter(|e| e.from == node_id).collect()
    }

    pub fn incoming_edges(&self, node_id: Uuid) -> Vec<&ExecutionEdge> {
        self.graph.edges.iter().filter(|e| e.to == node_id).collect()
    }

    pub fn has_loop_back_edge(&self, node_id: Uuid) -> bool {
        self.outgoing.get(&node_id)
            .map(|edges| edges.iter().any(|(_, cond)| cond.as_deref() == Some("loop")))
            .unwrap_or(false)
    }

    pub fn loop_back_target(&self, node_id: Uuid) -> Option<Uuid> {
        self.outgoing.get(&node_id)?
            .iter()
            .find(|(_, cond)| cond.as_deref() == Some("loop"))
            .map(|(to, _)| *to)
    }
}
