use std::collections::HashMap;
use std::sync::Arc;

use super::manifest::PluginManifest;
use super::PluginRegistry;

pub struct PluginManager {
    registry: PluginRegistry,
    manifests: HashMap<String, PluginManifest>,
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            registry: PluginRegistry::new(),
            manifests: HashMap::new(),
        }
    }

    pub fn registry(&self) -> &PluginRegistry {
        &self.registry
    }

    pub fn registry_mut(&mut self) -> &mut PluginRegistry {
        &mut self.registry
    }

    pub fn load_manifests(&mut self, dir: &str) {
        let manifests = PluginManifest::discover(dir);
        for (name, manifest) in manifests {
            tracing::info!(plugin = %name, "discovered plugin manifest");
            self.manifests.insert(name, manifest);
        }
    }

    pub fn register_provider(&mut self, name: &str, provider: Arc<dyn crate::providers::ChatProvider + Send + Sync>) {
        self.registry.register_provider(name, provider);
    }

    pub fn register_strategy(&mut self, kind: crate::types::StrategyKind, strategy: Box<dyn crate::strategies::Strategy + Send + Sync>) {
        self.registry.register_strategy(kind, strategy);
    }

    pub fn register_pass(&mut self, pass: Box<dyn crate::compiler::CompilerPass + Send + Sync>) {
        self.registry.register_pass(pass);
    }

    pub fn manifests(&self) -> &HashMap<String, PluginManifest> {
        &self.manifests
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}
