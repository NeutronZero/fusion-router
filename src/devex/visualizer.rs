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
