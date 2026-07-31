use std::collections::HashMap;

use fusion_plugin_api::{CapabilityContract, CapabilityId, CapabilityPlugin};

/// Dev-only. NOT for production use.
///
/// Dev-only runtime that loads capability logic as native Rust trait objects.
///
/// Maps `CapabilityId` to `Box<dyn CapabilityPlugin>` for in-process execution.
/// This is a development/testing adapter. Production deployments MUST use
/// `WasmtimeSandboxRuntime` from the Runtime subsystem.
pub struct NativeSandboxRuntime {
    capabilities: HashMap<CapabilityId, Box<dyn CapabilityPlugin>>,
}

impl NativeSandboxRuntime {
    pub fn new() -> Self {
        Self {
            capabilities: HashMap::new(),
        }
    }

    pub fn register(&mut self, id: CapabilityId, plugin: Box<dyn CapabilityPlugin>) {
        self.capabilities.insert(id, plugin);
    }

    pub fn get(&self, id: &CapabilityId) -> Option<&dyn CapabilityPlugin> {
        self.capabilities.get(id).map(|p| p.as_ref())
    }

    pub fn contracts(&self, id: &CapabilityId) -> Vec<CapabilityContract> {
        self.capabilities
            .get(id)
            .map(|p| p.capabilities())
            .unwrap_or_default()
    }

    pub fn contains(&self, id: &CapabilityId) -> bool {
        self.capabilities.contains_key(id)
    }

    pub fn registered_ids(&self) -> Vec<CapabilityId> {
        self.capabilities.keys().cloned().collect()
    }
}

impl Default for NativeSandboxRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EchoPlugin;

    impl fusion_plugin_api::Plugin for EchoPlugin {
        fn metadata(&self) -> fusion_plugin_api::PluginMetadata {
            fusion_plugin_api::PluginMetadata {
                name: "echo".into(),
                version: semver::Version::parse("0.1.0").unwrap(),
                api_version: semver::Version::parse("0.2.0").unwrap(),
                min_compiler_version: semver::Version::parse("0.11.0").unwrap(),
                capabilities: vec![CapabilityId::new("echo.text")],
            }
        }
    }

    impl CapabilityPlugin for EchoPlugin {
        fn capabilities(&self) -> Vec<CapabilityContract> {
            vec![CapabilityContract {
                id: CapabilityId::new("echo.text"),
                version: semver::Version::parse("0.1.0").unwrap(),
                description: "Echoes input text".into(),
                inputs_schema: serde_json::json!({}),
                outputs_schema: serde_json::json!({}),
                permissions: vec![],
                dependencies: vec![],
                estimated_cost_usd: 0.0,
                estimated_latency_ms: 1,
                reliability_score: 1.0,
                supports_streaming: false,
                traits: vec![],
            }]
        }
    }

    #[test]
    fn test_register_and_lookup() {
        let mut rt = NativeSandboxRuntime::new();
        let id = CapabilityId::new("echo.text");
        rt.register(id.clone(), Box::new(EchoPlugin));
        assert!(rt.contains(&id));
        assert!(rt.get(&id).is_some());
    }

    #[test]
    fn test_contracts_returned_for_registered_capability() {
        let mut rt = NativeSandboxRuntime::new();
        let id = CapabilityId::new("echo.text");
        rt.register(id.clone(), Box::new(EchoPlugin));
        let contracts = rt.contracts(&id);
        assert_eq!(contracts.len(), 1);
        assert_eq!(contracts[0].id, id);
    }

    #[test]
    fn test_unknown_capability_returns_none() {
        let rt = NativeSandboxRuntime::new();
        assert!(!rt.contains(&CapabilityId::new("nonexistent")));
        assert!(rt.get(&CapabilityId::new("nonexistent")).is_none());
    }

    #[test]
    fn test_registered_ids() {
        let mut rt = NativeSandboxRuntime::new();
        rt.register(CapabilityId::new("alpha"), Box::new(EchoPlugin));
        rt.register(CapabilityId::new("beta"), Box::new(EchoPlugin));
        let ids = rt.registered_ids();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&CapabilityId::new("alpha")));
        assert!(ids.contains(&CapabilityId::new("beta")));
    }
}
