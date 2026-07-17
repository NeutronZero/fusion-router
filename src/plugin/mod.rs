mod manager;
mod manifest;

#[allow(unused_imports)]
pub use manager::PluginManager;
#[allow(unused_imports)]
pub use manifest::PluginManifest;

use std::collections::HashMap;
use std::sync::Arc;

use crate::compiler::CompilerPass;
use crate::providers::ChatProvider;
use crate::strategies::Strategy;

pub type BoxedProvider = Box<dyn ChatProvider + Send + Sync>;
pub type BoxedStrategy = Box<dyn Strategy + Send + Sync>;
pub type BoxedPass = Box<dyn CompilerPass + Send + Sync>;

pub struct PluginRegistry {
    pub providers: HashMap<String, Arc<dyn ChatProvider + Send + Sync>>,
    pub strategies: HashMap<crate::types::StrategyKind, Box<dyn Strategy + Send + Sync>>,
    pub passes: Vec<Box<dyn CompilerPass + Send + Sync>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
            strategies: HashMap::new(),
            passes: Vec::new(),
        }
    }

    pub fn register_provider(&mut self, name: &str, provider: Arc<dyn ChatProvider + Send + Sync>) {
        tracing::info!(provider = %name, "registered plugin provider");
        self.providers.insert(name.to_string(), provider);
    }

    pub fn register_strategy(&mut self, kind: crate::types::StrategyKind, strategy: Box<dyn Strategy + Send + Sync>) {
        tracing::info!(strategy = ?kind, "registered plugin strategy");
        self.strategies.insert(kind, strategy);
    }

    pub fn register_pass(&mut self, pass: Box<dyn CompilerPass + Send + Sync>) {
        tracing::info!(pass = %pass.name(), "registered plugin compiler pass");
        self.passes.push(pass);
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}
