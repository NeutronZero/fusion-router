use std::sync::{Arc, RwLock};

use fusion_plugin_api::CapabilityId;

use crate::capability::registry::{CapabilityRegistry, InMemoryCapabilityRegistry};
use crate::package::PackageError;
use crate::package::verifier::VerifiedPackage;

pub struct PackageLoader {
    pub registry: Arc<RwLock<InMemoryCapabilityRegistry>>,
    #[cfg(feature = "wasm-plugins")]
    pub module_cache: Arc<crate::runtime::RuntimeModuleCache>,
}

impl PackageLoader {
    pub fn new(
        registry: Arc<RwLock<InMemoryCapabilityRegistry>>,
        #[cfg(feature = "wasm-plugins")] module_cache: Arc<crate::runtime::RuntimeModuleCache>,
    ) -> Self {
        Self {
            registry,
            #[cfg(feature = "wasm-plugins")]
            module_cache,
        }
    }

    pub fn load(&self, pkg: VerifiedPackage) -> Result<CapabilityId, PackageError> {
        let manifest = pkg.manifest();

        #[cfg(feature = "wasm-plugins")]
        let wasm_bytes = pkg.wasm_bytes().to_vec();

        #[cfg(feature = "wasm-plugins")]
        {
            let engine = wasmtime::Engine::new(&wasmtime::Config::new())
                .map_err(|e| PackageError::WasmCompilationFailed(e.to_string()))?;

            let key = (
                manifest.capabilities[0].id.clone(),
                manifest.version.clone(),
            );
            self.module_cache
                .get_or_compile(&key, &engine, &wasm_bytes)
                .map_err(|e| PackageError::WasmCompilationFailed(e.to_string()))?;
        }

        let primary_id = manifest.capabilities[0].id.clone();
        let mut reg = self.registry.write().unwrap();

        for contract in &manifest.capabilities {
            reg.register(contract.clone())
                .map_err(|e| PackageError::Registry(e.to_string()))?;
        }

        Ok(primary_id)
    }
}

#[cfg(all(test, feature = "wasm-plugins"))]
mod tests {
    use super::*;

    #[test]
    fn test_load_verified_package() {
        let registry = Arc::new(RwLock::new(InMemoryCapabilityRegistry::new()));
        let cache = Arc::new(crate::runtime::RuntimeModuleCache::new());
        let loader = PackageLoader::new(registry.clone(), cache);

        let signer = Arc::new(crate::release::signing::MockSigner::new("test-key"));
        let verifier = crate::package::PackageVerifier::new(signer);
        let contract = fusion_plugin_api::CapabilityContract {
            id: fusion_plugin_api::CapabilityId::new("test.load"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "load test".into(),
            inputs_schema: serde_json::json!({}),
            outputs_schema: serde_json::json!({}),
            permissions: vec![],
            dependencies: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 0,
            reliability_score: 1.0,
            supports_streaming: false,
        };
        let pkg_bytes = crate::package::format::build_test_fusionpkg(
            "test.load", "0.1.0", &[contract], &[],
        );
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &pkg_bytes).unwrap();
        let verified = verifier.verify(tmp.path()).unwrap();
        let cap_id = loader.load(verified).unwrap();
        assert_eq!(cap_id.to_string(), "test.load");
    }
}
