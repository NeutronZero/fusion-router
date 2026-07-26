use std::collections::HashMap;
use std::sync::Arc;
use crate::strategies::Strategy;
use crate::compiler::diagnostics::CompilerDiagnostic;

#[derive(Default)]
pub struct StrategyRegistry {
    strategies: HashMap<String, Arc<dyn Strategy>>,
}

impl StrategyRegistry {
    pub fn new() -> Self {
        Self {
            strategies: HashMap::new(),
        }
    }

    pub fn register(&mut self, strategy: Arc<dyn Strategy>) {
        let name = strategy.descriptor().name.to_lowercase();
        self.strategies.insert(name, strategy);
    }

    pub fn get(&self, name: &str) -> Result<Arc<dyn Strategy>, CompilerDiagnostic> {
        let key = name.to_lowercase();
        self.strategies.get(&key).cloned().ok_or_else(|| {
            CompilerDiagnostic::error(
                "E0100",
                format!("Strategy '{}' is not registered in StrategyRegistry", name),
            )
        })
    }

    pub fn contains(&self, name: &str) -> bool {
        self.strategies.contains_key(&name.to_lowercase())
    }
}
