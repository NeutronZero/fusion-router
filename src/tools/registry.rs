use std::collections::HashMap;
use std::sync::Arc;

use super::Tool;

pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool + Send + Sync>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: HashMap::new() }
    }

    pub fn register(&mut self, tool: Arc<dyn Tool + Send + Sync>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool + Send + Sync>> {
        self.tools.get(name)
    }

    pub fn list(&self) -> Vec<&str> {
        self.tools.keys().map(|k| k.as_str()).collect()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    pub fn unregister(&mut self, name: &str) {
        self.tools.remove(name);
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::builtin::CalculatorTool;

    #[test]
    fn test_registry_len_and_contains() {
        let mut reg = ToolRegistry::new();
        assert_eq!(reg.len(), 0);
        assert!(!reg.contains("calculator"));
        reg.register(Arc::new(CalculatorTool));
        assert_eq!(reg.len(), 1);
        assert!(reg.contains("calculator"));
    }

    #[test]
    fn test_unregister() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(CalculatorTool));
        assert!(reg.contains("calculator"));
        reg.unregister("calculator");
        assert!(!reg.contains("calculator"));
        assert_eq!(reg.len(), 0);
    }
}
