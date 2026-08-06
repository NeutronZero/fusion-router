use crate::planner::resolver::capability::CapabilityGraph;

pub struct GraphVisualizer;

impl Default for GraphVisualizer {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphVisualizer {
    pub fn new() -> Self {
        Self
    }

    pub fn to_mermaid(&self, graph: &CapabilityGraph) -> String {
        let mut mermaid = String::from("graph TD;\n");
        let nodes = graph.nodes();
        
        for id in nodes.keys() {
            mermaid.push_str(&format!(
                "    {}[{}]\n",
                id.as_str().replace('.', "_"),
                id.as_str()
            ));
        }

        for edge in graph.dependencies() {
            mermaid.push_str(&format!(
                "    {} --> {}\n",
                edge.from.as_str().replace('.', "_"),
                edge.to.as_str().replace('.', "_")
            ));
        }
        
        for edge in graph.conflicts() {
            mermaid.push_str(&format!(
                "    {} -.-x {}\n",
                edge.capability_a.as_str().replace('.', "_"),
                edge.capability_b.as_str().replace('.', "_")
            ));
        }
        
        mermaid
    }

    pub fn to_ascii(&self, graph: &CapabilityGraph) -> String {
        let mut ascii = String::from("ASCII Graph:\n");
        
        ascii.push_str("Nodes:\n");
        for id in graph.nodes().keys() {
            ascii.push_str(&format!(" - {}\n", id.as_str()));
        }
        
        ascii.push_str("Dependencies:\n");
        for edge in graph.dependencies() {
            ascii.push_str(&format!("  {} -> {}\n", edge.from.as_str(), edge.to.as_str()));
        }

        ascii.push_str("Conflicts:\n");
        for edge in graph.conflicts() {
            ascii.push_str(&format!("  {} <-> {}\n", edge.capability_a.as_str(), edge.capability_b.as_str()));
        }
        
        ascii
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::resolver::capability::CapabilityGraph;
    use fusion_plugin_api::{CapabilityContract, CapabilityId};
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
    fn test_to_ascii_output() {
        let mut graph = CapabilityGraph::new();
        graph.add_node(make_contract("cap.a"));
        graph.add_node(make_contract("cap.b"));
        graph.add_dependency(CapabilityId::new("cap.a"), CapabilityId::new("cap.b"));

        let viz = GraphVisualizer::new();
        let ascii = viz.to_ascii(&graph);
        assert!(ascii.contains("ASCII Graph:"));
        assert!(ascii.contains("- cap.a"));
        assert!(ascii.contains("- cap.b"));
        assert!(ascii.contains("cap.a -> cap.b"));
    }

    #[test]
    fn test_to_mermaid_output() {
        let mut graph = CapabilityGraph::new();
        graph.add_node(make_contract("cap.a"));
        let viz = GraphVisualizer::new();
        let mermaid = viz.to_mermaid(&graph);
        assert!(mermaid.contains("graph TD;"));
        assert!(mermaid.contains("cap_a[cap.a]"));
    }
}
