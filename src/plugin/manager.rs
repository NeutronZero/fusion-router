use std::collections::HashMap;
use std::sync::Arc;

#[cfg(feature = "wasm-plugins")]
use std::path::Path;

use super::manifest::PluginManifest;
use super::PluginRegistry;

#[cfg(feature = "wasm-plugins")]
use crate::wasm::{WasmModule, WasmRuntime};

pub struct PluginManager {
    registry: PluginRegistry,
    manifests: HashMap<String, PluginManifest>,
    #[cfg(feature = "wasm-plugins")]
    wasm_runtime: Option<WasmRuntime>,
    #[cfg(feature = "wasm-plugins")]
    wasm_modules: HashMap<String, WasmModule>,
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            registry: PluginRegistry::new(),
            manifests: HashMap::new(),
            #[cfg(feature = "wasm-plugins")]
            wasm_runtime: None,
            #[cfg(feature = "wasm-plugins")]
            wasm_modules: HashMap::new(),
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
            #[cfg(feature = "wasm-plugins")]
            if manifest.wasm.is_some() {
                if let Err(e) = self.load_wasm_plugin(&name, &manifest, dir) {
                    tracing::warn!(plugin = %name, error = %e, "failed to load wasm plugin");
                }
            }
            self.manifests.insert(name, manifest);
        }
    }

    #[cfg(feature = "wasm-plugins")]
    fn load_wasm_plugin(&mut self, name: &str, manifest: &PluginManifest, dir: &str) -> anyhow::Result<()> {
        let runtime = self.wasm_runtime.get_or_insert_with(|| {
            WasmRuntime::new().expect("Failed to create WasmRuntime")
        });

        let wasm_path = Path::new(dir).join(&manifest.plugin.entry);
        let wasm_bytes = std::fs::read(&wasm_path)?;
        let module = runtime.load_module(&wasm_bytes)?;
        self.wasm_modules.insert(name.to_string(), module);
        tracing::info!(plugin = %name, path = %wasm_path.display(), "loaded wasm plugin");
        Ok(())
    }

    #[cfg(feature = "wasm-plugins")]
    pub fn get_wasm_module(&self, name: &str) -> Option<&WasmModule> {
        self.wasm_modules.get(name)
    }

    #[cfg(feature = "wasm-plugins")]
    pub fn call_wasm_function(
        &self,
        name: &str,
        function: &str,
        params: &[wasmtime::Val],
    ) -> anyhow::Result<Vec<wasmtime::Val>> {
        let module = self.wasm_modules.get(name)
            .ok_or_else(|| anyhow::anyhow!("wasm plugin '{}' not found", name))?;
        let runtime = self.wasm_runtime.as_ref()
            .ok_or_else(|| anyhow::anyhow!("wasm runtime not initialized"))?;
        let mut instance = module.instantiate(runtime)?;
        instance.call_func(function, params)
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

    pub fn register_tool(&mut self, tool: Arc<dyn crate::tools::Tool + Send + Sync>) {
        self.registry.register_tool(tool);
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
