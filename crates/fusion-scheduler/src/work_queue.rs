//! WorkQueue â€” maintains DAG execution state for topological scheduling.

use fusion_types::*;
use std::collections::{HashMap, HashSet};

pub struct WorkQueue {
    graph: std::sync::Arc<ExecutionGraph>,
    completed: HashSet<uuid::Uuid>,
    in_progress: HashSet<uuid::Uuid>,
    failed: HashSet<uuid::Uuid>,
    ready: HashSet<uuid::Uuid>,
    outgoing: HashMap<uuid::Uuid, Vec<(uuid::Uuid, Option<String>)>>,
    incoming: HashMap<uuid::Uuid, Vec<uuid::Uuid>>,
    outgoing_edges_map: HashMap<uuid::Uuid, Vec<ExecutionEdge>>,
    incoming_edges_map: HashMap<uuid::Uuid, Vec<ExecutionEdge>>,
    total_incoming: HashMap<uuid::Uuid, usize>,
    satisfied_incoming: HashMap<uuid::Uuid, usize>,
    activated_edges: HashSet<(uuid::Uuid, uuid::Uuid)>,
    /// Nodes whose outgoing edges are activated selectively by dedicated
    /// scheduler branches (`Conditional` matched-condition edges, `Loop`
    /// continue/exit edges) and must never be blanket-activated.
    selective_sources: HashSet<uuid::Uuid>,
    /// Edges (from, to) classified as iteration back-edges: an edge into a
    /// Loop head whose source is reachable from that head (the loop body),
    /// or a `"loop"`-conditioned edge into it. These are produced inside the
    /// loop and must never gate initial readiness.
    back_edges: HashSet<(uuid::Uuid, uuid::Uuid)>,
    /// Configurable dispatch cap; defaults to the historical 16.
    max_concurrent_nodes: usize,
}

impl WorkQueue {
    pub fn new(graph: impl Into<std::sync::Arc<ExecutionGraph>>) -> Self {
        let graph = graph.into();
        let n_nodes = graph.nodes.len();
        let mut outgoing: HashMap<uuid::Uuid, Vec<(uuid::Uuid, Option<String>)>> =
            HashMap::with_capacity(n_nodes);
        let mut incoming: HashMap<uuid::Uuid, Vec<uuid::Uuid>> = HashMap::with_capacity(n_nodes);
        let mut outgoing_edges_map: HashMap<uuid::Uuid, Vec<ExecutionEdge>> =
            HashMap::with_capacity(n_nodes);
        let mut incoming_edges_map: HashMap<uuid::Uuid, Vec<ExecutionEdge>> =
            HashMap::with_capacity(n_nodes);
        let mut total_incoming: HashMap<uuid::Uuid, usize> = HashMap::with_capacity(n_nodes);

        let mut loop_node_ids: HashSet<uuid::Uuid> = HashSet::new();
        let mut selective_sources: HashSet<uuid::Uuid> = HashSet::new();
        for node in &graph.nodes {
            if matches!(node.kind, ExecutionNodeKind::Loop) {
                loop_node_ids.insert(node.id);
            }
            if matches!(
                node.kind,
                ExecutionNodeKind::Loop | ExecutionNodeKind::Conditional
            ) {
                selective_sources.insert(node.id);
            }
            outgoing.entry(node.id).or_default();
            incoming.entry(node.id).or_default();
            outgoing_edges_map.entry(node.id).or_default();
            incoming_edges_map.entry(node.id).or_default();
            total_incoming.entry(node.id).or_insert(0);
        }

        for edge in &graph.edges {
            outgoing
                .entry(edge.from)
                .or_default()
                .push((edge.to, edge.condition.clone()));
            incoming.entry(edge.to).or_default().push(edge.from);
            outgoing_edges_map
                .entry(edge.from)
                .or_default()
                .push(edge.clone());
            incoming_edges_map
                .entry(edge.to)
                .or_default()
                .push(edge.clone());
        }

        // Classify iteration back-edges structurally: an edge into a Loop
        // head from a node the head can reach (i.e. part of its own body) is
        // produced inside the loop, regardless of edge conditioning. A
        // `"loop"`-conditioned edge into a Loop head is also treated as a
        // back-edge for compatibility with graphs that rely on the marker.
        let mut back_edges: HashSet<(uuid::Uuid, uuid::Uuid)> = HashSet::new();
        for &loop_id in &loop_node_ids {
            let mut seen: HashSet<uuid::Uuid> = HashSet::new();
            let mut stack = vec![loop_id];
            while let Some(current) = stack.pop() {
                if let Some(targets) = outgoing.get(&current) {
                    for (to, _) in targets {
                        if seen.insert(*to) {
                            stack.push(*to);
                        }
                    }
                }
            }
            for edge in &graph.edges {
                if edge.to == loop_id && (edge.from == loop_id || seen.contains(&edge.from)) {
                    back_edges.insert((edge.from, edge.to));
                }
            }
        }
        for edge in &graph.edges {
            if loop_node_ids.contains(&edge.to) && edge.condition.as_deref() == Some("loop") {
                back_edges.insert((edge.from, edge.to));
            }
        }

        // All incoming edges gate readiness â€” including edges into Loop
        // nodes, so a loop head with genuine upstream dependencies stays
        // un-ready until they complete. Back-edges are excluded: counting
        // them would deadlock the loop head at t=0.
        for edge in &graph.edges {
            if !back_edges.contains(&(edge.from, edge.to)) {
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
            incoming,
            outgoing_edges_map,
            incoming_edges_map,
            total_incoming,
            satisfied_incoming: HashMap::new(),
            activated_edges: HashSet::new(),
            selective_sources,
            back_edges,
            max_concurrent_nodes: 16,
        }
    }

    /// Overrides the per-dispatch concurrency cap (dispatch loop truncation).
    pub fn with_max_concurrent_nodes(mut self, n: usize) -> Self {
        self.max_concurrent_nodes = n.max(1);
        self
    }

    /// Nodes ready at construction time (test/inspection aid).
    pub fn initial_ready(&self) -> &HashSet<uuid::Uuid> {
        &self.ready
    }

    fn try_activate_downstream(&mut self, from: uuid::Uuid) {
        // Conditional and Loop sources pick their outgoing edges selectively
        // (matched-condition branches / continue-vs-exit) via
        // `activate_edge`; blanket activation here would fire untaken
        // branches (e.g. the `"exit"` target while the loop continues).
        if self.selective_sources.contains(&from) {
            return;
        }
        let Some(edges) = self.outgoing.get(&from).cloned() else {
            return;
        };
        for (to, _condition) in &edges {
            if !self.activated_edges.contains(&(from, *to)) {
                self.activated_edges.insert((from, *to));
                let satisfied = self.satisfied_incoming.entry(*to).or_insert(0);
                *satisfied += 1;
                let total = self.total_incoming.get(to).copied().unwrap_or(0);
                if *satisfied == total && !self.completed.contains(to) && !self.failed.contains(to)
                {
                    self.ready.insert(*to);
                }
            }
        }
    }

    pub fn get_ready(&self, node_states: &HashMap<uuid::Uuid, NodeState>) -> Vec<&ExecutionNode> {
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

        let available = self
            .max_concurrent_nodes
            .saturating_sub(self.in_progress.len());
        if result.len() > available {
            result.truncate(available);
        }

        result
    }

    pub fn mark_completed(&mut self, node_id: uuid::Uuid) {
        self.completed.insert(node_id);
        self.in_progress.remove(&node_id);
        self.ready.remove(&node_id);
        self.try_activate_downstream(node_id);
    }

    pub fn mark_conditional_completed(&mut self, node_id: uuid::Uuid) {
        self.completed.insert(node_id);
        self.in_progress.remove(&node_id);
        self.ready.remove(&node_id);
    }

    pub fn activate_edge(&mut self, from: uuid::Uuid, to: uuid::Uuid) {
        if self.back_edges.contains(&(from, to)) {
            self.ready.insert(to);
            return;
        }
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

    pub fn mark_failed(&mut self, node_id: uuid::Uuid) {
        self.failed.insert(node_id);
        self.in_progress.remove(&node_id);
        self.ready.remove(&node_id);
    }

    pub fn mark_in_progress(&mut self, node_id: uuid::Uuid) {
        self.in_progress.insert(node_id);
        self.ready.remove(&node_id);
    }

    pub fn reset_loop_body(&mut self, body_ids: &[uuid::Uuid]) {
        for id in body_ids {
            self.completed.remove(id);
            self.in_progress.remove(id);
            self.failed.remove(id);
            self.ready.remove(id);
            let total = self.total_incoming.get(id).copied().unwrap_or(0);
            let satisfied: usize = self
                .incoming
                .get(id)
                .map(|sources| {
                    sources
                        .iter()
                        .filter(|from| {
                            self.completed.contains(from)
                                && self.activated_edges.contains(&(**from, *id))
                        })
                        .count()
                })
                .unwrap_or(0);
            self.satisfied_incoming.insert(*id, satisfied);
            if satisfied == total && satisfied > 0 {
                self.ready.insert(*id);
            }
        }
    }

    pub fn is_done(&self, node_states: &HashMap<uuid::Uuid, NodeState>) -> bool {
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

    pub fn any_in_progress(&self) -> bool {
        !self.in_progress.is_empty()
    }

    pub fn graph(&self) -> &ExecutionGraph {
        &self.graph
    }

    pub fn outgoing_edges(&self, node_id: uuid::Uuid) -> &[ExecutionEdge] {
        self.outgoing_edges_map
            .get(&node_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn incoming_edges(&self, node_id: uuid::Uuid) -> &[ExecutionEdge] {
        self.incoming_edges_map
            .get(&node_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn has_loop_back_edge(&self, node_id: uuid::Uuid) -> bool {
        self.outgoing
            .get(&node_id)
            .map(|edges| {
                edges
                    .iter()
                    .any(|(to, _)| self.back_edges.contains(&(node_id, *to)))
            })
            .unwrap_or(false)
    }

    /// Returns the loop node reached from `node_id` via an iteration
    /// back-edge, if any (used to re-arm a loop body after an iteration).
    pub fn loop_back_target(&self, node_id: uuid::Uuid) -> Option<uuid::Uuid> {
        self.outgoing.get(&node_id).and_then(|edges| {
            edges
                .iter()
                .find(|(to, _)| self.back_edges.contains(&(node_id, *to)))
                .map(|(to, _)| *to)
        })
    }

    /// Re-arms a node for execution (used to re-run a loop node).
    pub fn reset_ready(&mut self, node_id: uuid::Uuid) {
        self.completed.remove(&node_id);
        self.in_progress.remove(&node_id);
        self.failed.remove(&node_id);
        self.ready.insert(node_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn make_node(id: uuid::Uuid, kind: ExecutionNodeKind) -> ExecutionNode {
        ExecutionNode {
            id,
            kind,
            strategy: StrategyKind::Single,
            model: "test".into(),
            retry_policy: RetryPolicy {
                max_retries: 0,
                backoff_ms: 0,
            },
            fallback: None,
            config: HashMap::new(),
            subgraph: None,
        }
    }

    fn make_graph(nodes: Vec<ExecutionNode>, edges: Vec<ExecutionEdge>) -> Arc<ExecutionGraph> {
        Arc::new(ExecutionGraph {
            graph_id: uuid::Uuid::new_v4(),
            metadata: GraphMetadata {
                estimated_cost: NanoUSD::ZERO,
                estimated_tokens: 0,
                policy_version: 0,
                max_depth: 1,
                node_count: nodes.len() as u32,
            },
            nodes,
            edges,
            total_tokens: 0,
            total_cost: NanoUSD::ZERO,
            primitive_graph_hash: 0,
        })
    }

    #[test]
    fn test_independent_nodes_are_all_ready() {
        let n1 = uuid::Uuid::new_v4();
        let n2 = uuid::Uuid::new_v4();
        let graph = make_graph(
            vec![
                make_node(n1, ExecutionNodeKind::LLMGenerate),
                make_node(n2, ExecutionNodeKind::LLMGenerate),
            ],
            vec![],
        );
        let queue = WorkQueue::new(graph);
        let ready = queue.get_ready(&HashMap::new());
        assert_eq!(ready.len(), 2);
    }

    #[test]
    fn test_sequential_dependency() {
        let n1 = uuid::Uuid::new_v4();
        let n2 = uuid::Uuid::new_v4();
        let graph = make_graph(
            vec![
                make_node(n1, ExecutionNodeKind::LLMGenerate),
                make_node(n2, ExecutionNodeKind::LLMGenerate),
            ],
            vec![ExecutionEdge {
                from: n1,
                to: n2,
                condition: None,
            }],
        );
        let mut queue = WorkQueue::new(graph);
        let states = HashMap::new();

        // Initially only n1 is ready
        let ready = queue.get_ready(&states);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, n1);

        // Complete n1 â†’ n2 becomes ready
        queue.mark_completed(n1);
        let ready = queue.get_ready(&states);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, n2);
    }

    #[test]
    fn test_join_requires_all_incoming() {
        let n1 = uuid::Uuid::new_v4();
        let n2 = uuid::Uuid::new_v4();
        let n3 = uuid::Uuid::new_v4();
        let graph = make_graph(
            vec![
                make_node(n1, ExecutionNodeKind::LLMGenerate),
                make_node(n2, ExecutionNodeKind::LLMGenerate),
                make_node(n3, ExecutionNodeKind::LLMGenerate),
            ],
            vec![
                ExecutionEdge {
                    from: n1,
                    to: n3,
                    condition: None,
                },
                ExecutionEdge {
                    from: n2,
                    to: n3,
                    condition: None,
                },
            ],
        );
        let mut queue = WorkQueue::new(graph);
        let states = HashMap::new();

        // n3 should not be ready yet (needs both n1 and n2)
        queue.mark_completed(n1);
        let ready = queue.get_ready(&states);
        assert!(!ready.iter().any(|n| n.id == n3));

        // Complete n2 â†’ n3 becomes ready
        queue.mark_completed(n2);
        let ready = queue.get_ready(&states);
        assert!(ready.iter().any(|n| n.id == n3));
    }

    #[test]
    fn test_conditional_node_does_not_activate_downstream_by_default() {
        let n1 = uuid::Uuid::new_v4();
        let n2 = uuid::Uuid::new_v4();
        let graph = make_graph(
            vec![
                make_node(n1, ExecutionNodeKind::Conditional),
                make_node(n2, ExecutionNodeKind::LLMGenerate),
            ],
            vec![ExecutionEdge {
                from: n1,
                to: n2,
                condition: Some("if_valid".into()),
            }],
        );
        let mut queue = WorkQueue::new(graph);
        let states = HashMap::new();

        // Complete conditional without activating edge â†’ n2 not ready
        queue.mark_conditional_completed(n1);
        let ready = queue.get_ready(&states);
        assert!(!ready.iter().any(|n| n.id == n2));

        // Manually activate edge â†’ n2 becomes ready
        queue.activate_edge(n1, n2);
        let ready = queue.get_ready(&states);
        assert!(ready.iter().any(|n| n.id == n2));
    }

    #[test]
    fn test_is_done() {
        let n1 = uuid::Uuid::new_v4();
        let n2 = uuid::Uuid::new_v4();
        let graph = make_graph(
            vec![
                make_node(n1, ExecutionNodeKind::LLMGenerate),
                make_node(n2, ExecutionNodeKind::LLMGenerate),
            ],
            vec![],
        );
        let queue = WorkQueue::new(graph);
        let mut states = HashMap::new();

        assert!(!queue.is_done(&states));

        states.insert(n1, NodeState::Succeeded);
        states.insert(n2, NodeState::Succeeded);
        assert!(queue.is_done(&states));
    }

    #[test]
    fn test_loop_back_edge_detection() {
        let head = uuid::Uuid::new_v4();
        let body = uuid::Uuid::new_v4();
        let graph = make_graph(
            vec![
                make_node(head, ExecutionNodeKind::Loop),
                make_node(body, ExecutionNodeKind::LLMGenerate),
            ],
            vec![
                ExecutionEdge {
                    from: head,
                    to: body,
                    condition: None,
                },
                ExecutionEdge {
                    from: body,
                    to: head,
                    condition: None,
                },
            ],
        );
        let queue = WorkQueue::new(graph);
        assert!(queue.has_loop_back_edge(body));
        assert_eq!(queue.loop_back_target(body), Some(head));
        assert!(!queue.has_loop_back_edge(head));
        let _states: HashMap<uuid::Uuid, NodeState> = HashMap::new();
        assert!(
            queue.initial_ready().contains(&head),
            "unconditioned back-edge must not gate loop-head readiness"
        );
    }

    #[test]
    fn test_loop_back_edge_condition_marker() {
        let head = uuid::Uuid::new_v4();
        let body = uuid::Uuid::new_v4();
        let graph = make_graph(
            vec![
                make_node(head, ExecutionNodeKind::Loop),
                make_node(body, ExecutionNodeKind::LLMGenerate),
            ],
            vec![ExecutionEdge {
                from: body,
                to: head,
                condition: Some("loop".into()),
            }],
        );
        let queue = WorkQueue::new(graph);
        assert!(queue.has_loop_back_edge(body));
        assert_eq!(queue.loop_back_target(body), Some(head));
    }

    #[test]
    fn test_genuine_dependency_into_loop_head_gates_readiness() {
        let dep = uuid::Uuid::new_v4();
        let head = uuid::Uuid::new_v4();
        let graph = make_graph(
            vec![
                make_node(dep, ExecutionNodeKind::LLMGenerate),
                make_node(head, ExecutionNodeKind::Loop),
            ],
            vec![ExecutionEdge {
                from: dep,
                to: head,
                condition: None,
            }],
        );
        let queue = WorkQueue::new(graph);
        assert!(
            !queue.has_loop_back_edge(dep),
            "a dependency edge into a loop head is not a back-edge"
        );
        let _states: HashMap<uuid::Uuid, NodeState> = HashMap::new();
        assert!(
            !queue.initial_ready().contains(&head),
            "dependency edge into a loop head must gate its readiness"
        );
        assert!(queue.initial_ready().contains(&dep));
    }
}
