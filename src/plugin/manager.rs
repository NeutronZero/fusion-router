use std::collections::HashMap;
use std::sync::Arc;

#[cfg(feature = "wasm-plugins")]
use std::path::Path;

use super::manifest::PluginManifest;
use super::PluginRegistry;
use crate::capability::{CapabilityRegistry, InMemoryCapabilityRegistry};
use fusion_plugin_api::{CapabilityPlugin, PluginMetadata};

/// Current supported engine versions for compatibility validation.
pub const CURRENT_API_VERSION: &str = "0.1.0";
pub const CURRENT_COMPILER_VERSION: &str = "0.9.0";

/// Validator for verifying plugin API and compiler version compatibility.
pub struct CompatibilityChecker;

impl CompatibilityChecker {
    /// Validates if `metadata` is compatible with the running engine.
    pub fn validate(metadata: &PluginMetadata) -> Result<(), String> {
        let current_api = semver::Version::parse(CURRENT_API_VERSION).unwrap();
        let current_compiler = semver::Version::parse(CURRENT_COMPILER_VERSION).unwrap();

        // API MAJOR version compatibility check
        if metadata.api_version.major != current_api.major {
            return Err(format!(
                "Incompatible API major version for plugin '{}': requires {}, engine supports {}",
                metadata.name, metadata.api_version, current_api
            ));
        }

        // Compiler version requirement check
        if metadata.min_compiler_version > current_compiler {
            return Err(format!(
                "Plugin '{}' requires compiler version >= {}, but engine runs {}",
                metadata.name, metadata.min_compiler_version, current_compiler
            ));
        }

        Ok(())
    }
}

pub struct PluginManager {
    registry: PluginRegistry,
    capability_registry: InMemoryCapabilityRegistry,
    manifests: HashMap<String, PluginManifest>,
    #[cfg(feature = "wasm-plugins")]
    wasm_runtime: Option<crate::wasm::WasmRuntime>,
    #[cfg(feature = "wasm-plugins")]
    wasm_modules: HashMap<String, crate::wasm::WasmModule>,
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            registry: PluginRegistry::new(),
            capability_registry: InMemoryCapabilityRegistry::new(),
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

    /// Registers a capability plugin, validates metadata compatibility, and populates contracts into `CapabilityRegistry`.
    pub fn register_capability_plugin(
        &mut self,
        plugin: &dyn CapabilityPlugin,
    ) -> Result<(), String> {
        let metadata = plugin.metadata();
        CompatibilityChecker::validate(&metadata)?;

        for contract in plugin.capabilities() {
            tracing::info!(capability = %contract.id, plugin = %metadata.name, "registered capability contract");
            self.capability_registry.register(contract).map_err(|e| e.to_string())?;
        }

        Ok(())
    }

    /// Freezes and returns an immutable `Arc<CapabilityRegistry>`.
    pub fn freeze_capability_registry(&mut self) -> Arc<dyn CapabilityRegistry> {
        let mut empty = InMemoryCapabilityRegistry::new();
        std::mem::swap(&mut self.capability_registry, &mut empty);
        empty.freeze();
        Arc::new(empty)
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
    fn load_wasm_plugin(
        &mut self,
        name: &str,
        manifest: &PluginManifest,
        dir: &str,
    ) -> anyhow::Result<()> {
        if self.wasm_runtime.is_none() {
            self.wasm_runtime = Some(crate::wasm::WasmRuntime::new()?);
        }
        let runtime = self.wasm_runtime.as_mut().unwrap();

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

    pub fn register_provider(
        &mut self,
        name: &str,
        provider: Arc<dyn crate::providers::ChatProvider + Send + Sync>,
    ) {
        self.registry.register_provider(name, provider);
    }

    pub fn register_strategy(
        &mut self,
        kind: crate::types::StrategyKind,
        strategy: Box<dyn crate::strategies::Strategy + Send + Sync>,
    ) {
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
    use fusion_plugin_echo::EchoPlugin;
    use fusion_plugin_api::CapabilityId;

    #[test]
    fn test_register_echo_capability_plugin() {
        let mut manager = PluginManager::new();
        let echo = EchoPlugin::new();

        manager.register_capability_plugin(&echo).unwrap();
        let frozen_reg = manager.freeze_capability_registry();

        assert!(frozen_reg.is_frozen());
        assert!(frozen_reg.contains(&CapabilityId::new("echo.text")));
        assert!(frozen_reg.contains(&CapabilityId::new("echo.uppercase")));
        assert_eq!(frozen_reg.list().len(), 2);
    }

    #[test]
    fn test_incompatible_plugin_rejected() {
        use fusion_plugin_api::{CapabilityContract, Plugin};

        struct BadPlugin;
        impl Plugin for BadPlugin {
            fn metadata(&self) -> PluginMetadata {
                PluginMetadata {
                    name: "bad-plugin".into(),
                    version: semver::Version::parse("1.0.0").unwrap(),
                    api_version: semver::Version::parse("9.0.0").unwrap(), // Incompatible major
                    min_compiler_version: semver::Version::parse("0.9.0").unwrap(),
                    capabilities: vec![],
                }
            }
        }
        impl CapabilityPlugin for BadPlugin {
            fn capabilities(&self) -> Vec<CapabilityContract> {
                vec![]
            }
        }

        let mut manager = PluginManager::new();
        let bad = BadPlugin;
        let res = manager.register_capability_plugin(&bad);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("Incompatible API major version"));
    }
}
