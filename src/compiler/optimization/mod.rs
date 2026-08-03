use crate::compiler::ir::PrimitiveGraph;
use crate::compiler::diagnostics::CompilerDiagnostic;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptimizationGoal {
    Latency,
    TokenCost,
    Memory,
    GraphSimplification,
    ProviderUtilization,
    Determinism,
}

pub trait OptimizationPass: Send + Sync {
    fn name(&self) -> &str;

    fn goal(&self) -> OptimizationGoal {
        OptimizationGoal::GraphSimplification
    }

    fn optimize(&self, graph: PrimitiveGraph) -> Result<PrimitiveGraph, CompilerDiagnostic>;

    fn preconditions(&self, _graph: &PrimitiveGraph) -> Result<(), CompilerDiagnostic> {
        Ok(())
    }

    fn postconditions(
        &self,
        _original: &PrimitiveGraph,
        _optimized: &PrimitiveGraph,
    ) -> Result<(), CompilerDiagnostic> {
        Ok(())
    }
}

pub struct DeadNodeEliminationPass {
    pub version: u16,
}

impl DeadNodeEliminationPass {
    pub const fn new() -> Self {
        Self { version: 1 }
    }
}

impl Default for DeadNodeEliminationPass {
    fn default() -> Self {
        Self::new()
    }
}

impl OptimizationPass for DeadNodeEliminationPass {
    fn name(&self) -> &str {
        "dead_node_elimination"
    }

    fn goal(&self) -> OptimizationGoal {
        OptimizationGoal::GraphSimplification
    }

    fn optimize(&self, graph: PrimitiveGraph) -> Result<PrimitiveGraph, CompilerDiagnostic> {
        let graph_id = graph.graph_id.clone();

        let mut live: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut queue: Vec<String> = Vec::new();

        if let Some(first) = graph.nodes.first() {
            live.insert(first.id.clone());
            queue.push(first.id.clone());
        }

        while let Some(current) = queue.pop() {
            for e in &graph.edges {
                if e.from == current && !live.contains(&e.to) {
                    live.insert(e.to.clone());
                    queue.push(e.to.clone());
                }
            }
        }

        let live_nodes: Vec<crate::compiler::ir::PrimitiveNode> = graph.nodes
            .into_iter()
            .filter(|n| live.contains(&n.id))
            .collect();

        let live_edges: Vec<crate::compiler::ir::PrimitiveEdge> = graph.edges
            .into_iter()
            .filter(|e| live.contains(&e.from) && live.contains(&e.to))
            .collect();

        let mut result = crate::compiler::ir::PrimitiveGraph::new(&graph_id);
        result.nodes = live_nodes;
        result.edges = live_edges;
        result.version = crate::compiler::ir::PRIMITIVE_GRAPH_VERSION;

        Ok(result)
    }
}

pub struct FanOutConsolidationPass {
    pub version: u16,
}

impl FanOutConsolidationPass {
    pub const fn new() -> Self {
        Self { version: 1 }
    }
}

impl Default for FanOutConsolidationPass {
    fn default() -> Self {
        Self::new()
    }
}

impl OptimizationPass for FanOutConsolidationPass {
    fn name(&self) -> &str {
        "fanout_consolidation"
    }

    fn goal(&self) -> OptimizationGoal {
        OptimizationGoal::GraphSimplification
    }

    fn optimize(&self, graph: PrimitiveGraph) -> Result<PrimitiveGraph, CompilerDiagnostic> {
        use crate::compiler::ir::{PrimitiveNodeKind, PrimitiveEdge};

        let graph_id = graph.graph_id.clone();
        let mut nodes = graph.nodes;
        let mut edges = graph.edges;

        // Phase 1: Eliminate single-consumer FanOuts

        let single_consumer_fanouts: std::collections::HashSet<String> = nodes
            .iter()
            .filter(|n| matches!(n.kind, PrimitiveNodeKind::FanOut { count: 1 }))
            .map(|n| n.id.clone())
            .collect();

        let mut remove_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

        for id in &single_consumer_fanouts {
            remove_ids.insert(id.clone());
            let incoming: Vec<String> = edges.iter()
                .filter(|e| e.to == *id)
                .map(|e| e.from.clone())
                .collect();
            let outgoing: Vec<(String, Option<String>)> = edges.iter()
                .filter(|e| e.from == *id)
                .map(|e| (e.to.clone(), e.condition.clone()))
                .collect();
            for from in &incoming {
                for (to, cond) in &outgoing {
                    edges.push(PrimitiveEdge {
                        from: from.clone(),
                        to: to.clone(),
                        condition: cond.clone(),
                    });
                }
            }
        }

        // Phase 2: Merge adjacent FanOuts
        let mut adjacency: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
        for e in &edges {
            adjacency.entry(e.from.clone()).or_default().push(e.to.clone());
        }

        // Frozen node list: an id -> index map turns repeated linear scans into O(1) lookups.
        let node_index: std::collections::HashMap<String, usize> = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id.clone(), i))
            .collect();

        let fanout_ids: Vec<String> = nodes.iter()
            .filter(|n| matches!(n.kind, PrimitiveNodeKind::FanOut { .. }) && !remove_ids.contains(&n.id))
            .map(|n| n.id.clone())
            .collect();

        for id in &fanout_ids {
            if remove_ids.contains(id) {
                continue;
            }
            let successors: Vec<String> = adjacency.get(id).cloned().unwrap_or_default();
            for succ_id in &successors {
                if remove_ids.contains(succ_id) {
                    continue;
                }
                if matches!(nodes[node_index[succ_id]].kind, PrimitiveNodeKind::FanOut { .. }) {
                    let count1 = if let PrimitiveNodeKind::FanOut { count } = &nodes[node_index[id]].kind {
                        *count
                    } else {
                        1
                    };
                    let count2 = if let PrimitiveNodeKind::FanOut { count } = &nodes[node_index[succ_id]].kind {
                        *count
                    } else {
                        1
                    };
                    let merged_count = count1.max(count2);

                    // Update the first FanOut's count
                    nodes[node_index[id]].kind = PrimitiveNodeKind::FanOut { count: merged_count };

                    // Reroute edges: everything that went to succ_id now comes from id
                    let succ_outgoing: Vec<(String, Option<String>)> = edges.iter()
                        .filter(|e| e.from == *succ_id)
                        .map(|e| (e.to.clone(), e.condition.clone()))
                        .collect();
                    for (to, cond) in &succ_outgoing {
                        if !edges.iter().any(|e| e.from == *id && e.to == *to) {
                            edges.push(PrimitiveEdge {
                                from: id.clone(),
                                to: to.clone(),
                                condition: cond.clone(),
                            });
                        }
                    }
                    remove_ids.insert(succ_id.clone());
                }
            }
        }

        // Remove eliminated nodes and their edges
        nodes.retain(|n| !remove_ids.contains(&n.id));
        edges.retain(|e| !remove_ids.contains(&e.from) && !remove_ids.contains(&e.to));

        let mut result = PrimitiveGraph::new(&graph_id);
        result.nodes = nodes;
        result.edges = edges;
        result.version = crate::compiler::ir::PRIMITIVE_GRAPH_VERSION;

        Ok(result)
    }
}

#[derive(Default)]
pub struct OptimizationPipeline {
    passes: Vec<Box<dyn OptimizationPass>>,
}

impl OptimizationPipeline {
    pub fn new() -> Self {
        Self { passes: Vec::new() }
    }

    pub fn add_pass(&mut self, pass: Box<dyn OptimizationPass>) {
        self.passes.push(pass);
    }

    pub fn run(&self, graph: PrimitiveGraph) -> Result<PrimitiveGraph, CompilerDiagnostic> {
        let mut current = graph;
        for pass in &self.passes {
            let snapshot = current.clone();
            match pass.optimize(current) {
                Ok(next) => current = next,
                Err(e) => {
                    if cfg!(debug_assertions) {
                        let _ = snapshot;
                    }
                    return Err(e);
                }
            }
        }
        Ok(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::ir::{PrimitiveNode, PrimitiveNodeKind};

    struct TestPass {
        name: &'static str,
        should_fail: bool,
    }

    impl OptimizationPass for TestPass {
        fn name(&self) -> &str {
            self.name
        }

        fn goal(&self) -> OptimizationGoal {
            OptimizationGoal::GraphSimplification
        }

        fn optimize(&self, graph: PrimitiveGraph) -> Result<PrimitiveGraph, CompilerDiagnostic> {
            if self.should_fail {
                Err(CompilerDiagnostic::error("OPT001", "intentional failure"))
            } else {
                Ok(graph)
            }
        }
    }

    fn make_graph() -> PrimitiveGraph {
        let mut g = PrimitiveGraph::new("test");
        g.add_node(PrimitiveNode {
            id: "n1".into(),
            kind: PrimitiveNodeKind::LLMGenerate { model: "gpt-4".into(), role: None },
            artifact_kind: None,
        });
        g
    }

    #[test]
    fn test_pipeline_empty() {
        let pipeline = OptimizationPipeline::new();
        let graph = make_graph();
        let result = pipeline.run(graph).unwrap();
        assert_eq!(result.nodes.len(), 1);
    }

    #[test]
    fn test_pipeline_rollback_on_failure() {
        let mut pipeline = OptimizationPipeline::new();
        pipeline.add_pass(Box::new(TestPass { name: "fail", should_fail: true }));
        let graph = make_graph();
        let result = pipeline.run(graph);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, "OPT001");
    }

    #[test]
    fn test_pipeline_multi_pass() {
        let mut pipeline = OptimizationPipeline::new();
        pipeline.add_pass(Box::new(TestPass { name: "p1", should_fail: false }));
        pipeline.add_pass(Box::new(TestPass { name: "p2", should_fail: false }));
        let graph = make_graph();
        let result = pipeline.run(graph).unwrap();
        assert_eq!(result.nodes.len(), 1);
    }

    #[test]
    fn test_pass_goal() {
        let pass = TestPass { name: "test", should_fail: false };
        assert_eq!(pass.goal(), OptimizationGoal::GraphSimplification);
    }

    #[test]
    fn test_pass_preconditions_default_ok() {
        let pass = TestPass { name: "test", should_fail: false };
        let graph = make_graph();
        assert!(pass.preconditions(&graph).is_ok());
    }

    #[test]
    fn test_pass_postconditions_default_ok() {
        let pass = TestPass { name: "test", should_fail: false };
        let graph = make_graph();
        assert!(pass.postconditions(&graph, &graph).is_ok());
    }
}
