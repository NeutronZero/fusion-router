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
                if matches!(
                    node_states.get(&node.id),
                    Some(NodeState::Succeeded | NodeState::Failed(_) | NodeState::Skipped)
                ) {
                    continue;
                }
                result.push(node);
            }
        }
        
        // Backpressure guardrail: limit ready nodes to prevent memory and concurrent execution overload
        // In a production system, this could be driven by the graph's metadata or configuration.
        let max_concurrent_nodes: usize = 16;
        let available = max_concurrent_nodes.saturating_sub(self.in_progress.len());
        if result.len() > available {
            result.truncate(available);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        ExecutionNodeKind, GraphMetadata, RetryPolicy, StrategyKind,
    };

    fn make_uuid(id: u32) -> Uuid {
        let mut bytes = [0u8; 16];
        bytes[0..4].copy_from_slice(&id.to_le_bytes());
        Uuid::from_bytes(bytes)
    }

    fn make_node(id: u32, kind: ExecutionNodeKind) -> ExecutionNode {
        ExecutionNode {
            id: make_uuid(id),
            kind,
            strategy: StrategyKind::Single,
            model: String::new(),
            retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
            fallback: None,
            config: HashMap::new(),
        }
    }

    fn make_edge(from: u32, to: u32, condition: Option<&str>) -> ExecutionEdge {
        ExecutionEdge {
            from: make_uuid(from),
            to: make_uuid(to),
            condition: condition.map(|s| s.to_string()),
        }
    }

    fn make_graph(nodes: Vec<ExecutionNode>, edges: Vec<ExecutionEdge>) -> ExecutionGraph {
        ExecutionGraph {
            graph_id: make_uuid(0),
            nodes,
            edges,
            metadata: GraphMetadata {
                estimated_cost: 0.0,
                estimated_tokens: 0,
                max_depth: 0,
                node_count: 0,
            },
            total_tokens: 0,
            total_cost: 0,
            primitive_graph_hash: 0,
        }
    }

    /// SC-01: DAG with 3 independent nodes (no edges). All 3 should be ready on init.
    #[test]
    fn test_independent_nodes_all_ready() {
        let nodes = vec![
            make_node(1, ExecutionNodeKind::LLMGenerate),
            make_node(2, ExecutionNodeKind::LLMGenerate),
            make_node(3, ExecutionNodeKind::LLMGenerate),
        ];
        let graph = make_graph(nodes, vec![]);
        let wq = WorkQueue::new(graph);
        let node_states = HashMap::new();
        let ready = wq.get_ready(&node_states);
        assert_eq!(ready.len(), 3);
    }

    /// SC-02: Nodes A → B → C. Only A ready initially; after A completes, B ready; then C.
    #[test]
    fn test_sequential_dependency() {
        let nodes = vec![
            make_node(1, ExecutionNodeKind::LLMGenerate),
            make_node(2, ExecutionNodeKind::LLMGenerate),
            make_node(3, ExecutionNodeKind::LLMGenerate),
        ];
        let edges = vec![make_edge(1, 2, None), make_edge(2, 3, None)];
        let graph = make_graph(nodes, edges);
        let mut wq = WorkQueue::new(graph);
        let node_states = HashMap::new();

        let ready = wq.get_ready(&node_states);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, make_uuid(1));

        wq.mark_completed(make_uuid(1));
        let ready = wq.get_ready(&node_states);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, make_uuid(2));

        wq.mark_completed(make_uuid(2));
        let ready = wq.get_ready(&node_states);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, make_uuid(3));
    }

    /// SC-03: Two predecessors feeding into join node.
    /// Join only becomes ready after both predecessors complete.
    #[test]
    fn test_join_node() {
        let nodes = vec![
            make_node(1, ExecutionNodeKind::LLMGenerate),
            make_node(2, ExecutionNodeKind::LLMGenerate),
            make_node(3, ExecutionNodeKind::Join),
        ];
        let edges = vec![make_edge(1, 3, None), make_edge(2, 3, None)];
        let graph = make_graph(nodes, edges);
        let mut wq = WorkQueue::new(graph);
        let node_states = HashMap::new();

        let ready = wq.get_ready(&node_states);
        assert_eq!(ready.len(), 2);

        wq.mark_completed(make_uuid(1));
        let ready = wq.get_ready(&node_states);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, make_uuid(2));

        wq.mark_completed(make_uuid(2));
        let ready = wq.get_ready(&node_states);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, make_uuid(3));
    }

    /// SC-04: Conditional node with two outgoing edges. On completion,
    /// only the edge matching the output condition is activated.
    #[test]
    fn test_conditional_branch() {
        let nodes = vec![
            make_node(1, ExecutionNodeKind::Conditional),
            make_node(2, ExecutionNodeKind::LLMGenerate),
            make_node(3, ExecutionNodeKind::LLMGenerate),
        ];
        let edges = vec![
            make_edge(1, 2, Some("a")),
            make_edge(1, 3, Some("b")),
        ];
        let graph = make_graph(nodes, edges);
        let mut wq = WorkQueue::new(graph);
        let node_states = HashMap::new();

        let ready = wq.get_ready(&node_states);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, make_uuid(1));

        wq.mark_conditional_completed(make_uuid(1));
        let ready = wq.get_ready(&node_states);
        assert_eq!(ready.len(), 0);

        wq.activate_edge(make_uuid(1), make_uuid(2));
        let ready = wq.get_ready(&node_states);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, make_uuid(2));
        assert!(!wq.ready.contains(&make_uuid(3)));
    }

    /// SC-05: Loop node with body nodes. reset_loop_body clears completion
    /// state and recomputes ready set.
    #[test]
    fn test_loop_body_reset() {
        let nodes = vec![
            make_node(1, ExecutionNodeKind::Loop),
            make_node(2, ExecutionNodeKind::LLMGenerate),
            make_node(3, ExecutionNodeKind::LLMGenerate),
        ];
        let edges = vec![make_edge(1, 2, None), make_edge(1, 3, None)];
        let graph = make_graph(nodes, edges);
        let mut wq = WorkQueue::new(graph);

        wq.mark_completed(make_uuid(1));
        wq.mark_completed(make_uuid(2));
        wq.mark_completed(make_uuid(3));

        assert!(wq.is_done(&HashMap::new()));

        wq.reset_loop_body(&[make_uuid(2), make_uuid(3)]);

        assert!(!wq.completed.contains(&make_uuid(2)));
        assert!(!wq.completed.contains(&make_uuid(3)));
        let node_states = HashMap::new();
        let ready = wq.get_ready(&node_states);
        assert_eq!(ready.len(), 2);
    }

    /// SC-06: mark_in_progress removes from ready; mark_completed activates downstream.
    #[test]
    fn test_concurrent_execution_tracking() {
        let nodes = vec![
            make_node(1, ExecutionNodeKind::LLMGenerate),
            make_node(2, ExecutionNodeKind::LLMGenerate),
            make_node(3, ExecutionNodeKind::LLMGenerate),
        ];
        let edges = vec![make_edge(1, 3, None)];
        let graph = make_graph(nodes, edges);
        let mut wq = WorkQueue::new(graph);
        let node_states = HashMap::new();

        let ready = wq.get_ready(&node_states);
        assert_eq!(ready.len(), 2);

        wq.mark_in_progress(make_uuid(1));
        let ready = wq.get_ready(&node_states);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, make_uuid(2));

        wq.mark_completed(make_uuid(1));
        let ready = wq.get_ready(&node_states);
        assert_eq!(ready.len(), 2);
        let ids: HashSet<_> = ready.iter().map(|n| n.id).collect();
        assert!(ids.contains(&make_uuid(2)));
        assert!(ids.contains(&make_uuid(3)));
    }

    /// SC-07: mark_failed removes from ready and completed; is_done returns
    /// true when nodes are failed.
    #[test]
    fn test_cancellation_via_failed() {
        let nodes = vec![
            make_node(1, ExecutionNodeKind::LLMGenerate),
            make_node(2, ExecutionNodeKind::LLMGenerate),
        ];
        let edges = vec![make_edge(1, 2, None)];
        let graph = make_graph(nodes, edges);
        let mut wq = WorkQueue::new(graph);
        let node_states = HashMap::new();

        let ready = wq.get_ready(&node_states);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, make_uuid(1));

        wq.mark_failed(make_uuid(1));
        assert!(wq.failed.contains(&make_uuid(1)));
        assert!(!wq.ready.contains(&make_uuid(1)));
        let ready = wq.get_ready(&node_states);
        assert_eq!(ready.len(), 0);

        // Downstream node 2 never became ready; cancel it too.
        wq.mark_failed(make_uuid(2));

        assert!(wq.is_done(&HashMap::new()));
    }
}
