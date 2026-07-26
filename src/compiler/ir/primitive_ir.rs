use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;

pub const PRIMITIVE_GRAPH_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum BarrierFailurePolicy {
    Continue,
    Retry,
    Abort,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReducerMode {
    Debate,
    Consensus,
    Majority,
    WeightedVote,
    Merge,
    Score,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PrimitiveNodeKind {
    LLMGenerate { model: String, role: Option<String> },
    LLMReview { model: String },
    FanOut { count: u32 },
    Barrier {
        min_completion: f32,
        timeout: Duration,
        on_failure: BarrierFailurePolicy,
    },
    Reducer {
        mode: ReducerMode,
        model: String,
    },
    FeedbackLoop { max_iterations: u32 },
    ConditionalBranch { condition: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PrimitiveNode {
    pub id: String,
    pub kind: PrimitiveNodeKind,
    pub artifact_kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PrimitiveEdge {
    pub from: String,
    pub to: String,
    pub condition: Option<String>,
}

/// PrimitiveGraph is the formal Runtime ABI contract between compiler and scheduler.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PrimitiveGraph {
    pub version: u16,
    pub graph_id: String,
    pub nodes: Vec<PrimitiveNode>,
    pub edges: Vec<PrimitiveEdge>,
}

impl PrimitiveGraph {
    pub fn new(graph_id: impl Into<String>) -> Self {
        Self {
            version: PRIMITIVE_GRAPH_VERSION,
            graph_id: graph_id.into(),
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }

    pub fn add_node(&mut self, node: PrimitiveNode) {
        self.nodes.push(node);
    }

    pub fn add_edge(&mut self, from: impl Into<String>, to: impl Into<String>, condition: Option<String>) {
        self.edges.push(PrimitiveEdge {
            from: from.into(),
            to: to.into(),
            condition,
        });
    }

    /// Compute deterministic 64-bit hash of the PrimitiveGraph
    pub fn compute_hash(&self) -> u64 {
        let json = serde_json::to_string(self).unwrap_or_default();
        let mut hasher = DefaultHasher::new();
        json.hash(&mut hasher);
        hasher.finish()
    }

    /// Export graph to Mermaid diagram markdown syntax
    pub fn to_mermaid(&self) -> String {
        let mut out = String::from("graph TD\n");
        for node in &self.nodes {
            let label = match &node.kind {
                PrimitiveNodeKind::LLMGenerate { model, role } => {
                    if let Some(r) = role {
                        format!("LLMGenerate ({} - {})", model, r)
                    } else {
                        format!("LLMGenerate ({})", model)
                    }
                }
                PrimitiveNodeKind::LLMReview { model } => format!("LLMReview ({})", model),
                PrimitiveNodeKind::FanOut { count } => format!("FanOut ({})", count),
                PrimitiveNodeKind::Barrier { min_completion, .. } => {
                    format!("Barrier ({:.0}%)", min_completion * 100.0)
                }
                PrimitiveNodeKind::Reducer { mode, model } => format!("Reducer ({:?} - {})", mode, model),
                PrimitiveNodeKind::FeedbackLoop { max_iterations } => {
                    format!("FeedbackLoop (max: {})", max_iterations)
                }
                PrimitiveNodeKind::ConditionalBranch { condition } => {
                    format!("Branch ({})", condition)
                }
            };
            out.push_str(&format!("    {}[\"{}\"]\n", node.id, label));
        }
        for edge in &self.edges {
            if let Some(cond) = &edge.condition {
                out.push_str(&format!("    {} -- \"{}\" --> {}\n", edge.from, cond, edge.to));
            } else {
                out.push_str(&format!("    {} --> {}\n", edge.from, edge.to));
            }
        }
        out
    }

    /// Export graph to Graphviz DOT syntax
    pub fn to_dot(&self) -> String {
        let mut out = String::from("digraph PrimitiveGraph {\n    rankdir=TB;\n");
        for node in &self.nodes {
            out.push_str(&format!("    \"{}\" [label=\"{}\"];\n", node.id, node.id));
        }
        for edge in &self.edges {
            if let Some(cond) = &edge.condition {
                out.push_str(&format!("    \"{}\" -> \"{}\" [label=\"{}\"];\n", edge.from, edge.to, cond));
            } else {
                out.push_str(&format!("    \"{}\" -> \"{}\";\n", edge.from, edge.to));
            }
        }
        out.push_str("}\n");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_primitive_graph_tooling() {
        let mut graph = PrimitiveGraph::new("test_graph");
        graph.add_node(PrimitiveNode {
            id: "node_1".into(),
            kind: PrimitiveNodeKind::FanOut { count: 2 },
            artifact_kind: None,
        });
        graph.add_node(PrimitiveNode {
            id: "node_2".into(),
            kind: PrimitiveNodeKind::LLMGenerate {
                model: "gpt-4".into(),
                role: Some("Worker".into()),
            },
            artifact_kind: Some("Generic".into()),
        });
        graph.add_edge("node_1", "node_2", None);

        let hash1 = graph.compute_hash();
        let hash2 = graph.compute_hash();
        assert_eq!(hash1, hash2);

        let mermaid = graph.to_mermaid();
        assert!(mermaid.contains("FanOut (2)"));
        assert!(mermaid.contains("node_1 --> node_2"));

        let dot = graph.to_dot();
        assert!(dot.contains("digraph PrimitiveGraph"));
    }
}
