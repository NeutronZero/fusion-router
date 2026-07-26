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

        if let Err(e) = self.try_register_wasm_strategy(&wasm_path, name) {
            tracing::debug!(plugin = %name, error = %e, "wasm module does not export strategy interface");
        }

        tracing::info!(plugin = %name, path = %wasm_path.display(), "loaded wasm plugin");
        Ok(())
    }

    #[cfg(feature = "wasm-plugins")]
    fn try_register_wasm_strategy(&mut self, wasm_path: &Path, name: &str) -> anyhow::Result<()> {
        let kind = crate::types::StrategyKind::Custom(name.to_string());
        super::wasm::load_and_register_wasm_strategy(&mut self.registry, wasm_path, Some(kind))?;
        tracing::info!(plugin = %name, "registered wasm strategy");
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

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn write_manifest(dir: &std::path::Path, name: &str, content: &str) {
        std::fs::write(dir.join(name), content).unwrap();
    }

    #[test]
    fn test_plugin_discovery() {
        let dir = std::env::temp_dir().join(format!("fusion_plugins_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        write_manifest(
            &dir,
            "good.toml",
            r#"[plugin]
name = "test-plugin"
version = "1.0.0"
entry = "plugin.wasm""#,
        );

        let mut manager = PluginManager::new();
        manager.load_manifests(dir.to_string_lossy().as_ref());

        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(manager.manifests().len(), 1);
        assert!(manager.manifests().contains_key("test-plugin"));
    }

    #[test]
    fn test_malformed_manifest() {
        let dir = std::env::temp_dir().join(format!("fusion_plugins_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        write_manifest(
            &dir,
            "good.toml",
            r#"[plugin]
name = "valid-plugin"
version = "1.0.0"
entry = "plugin.wasm""#,
        );

        write_manifest(&dir, "bad.toml", "this is not toml [[[");

        let mut manager = PluginManager::new();
        manager.load_manifests(dir.to_string_lossy().as_ref());

        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(manager.manifests().len(), 1);
        assert!(manager.manifests().contains_key("valid-plugin"));
    }
}
