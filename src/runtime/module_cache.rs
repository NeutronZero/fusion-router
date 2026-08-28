use fusion_plugin_api::CapabilityId;
use parking_lot::RwLock;
use semver::Version;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use wasmtime::{Engine, Module};

type CacheKey = (CapabilityId, Version, u64);

#[derive(Default)]
pub struct RuntimeModuleCache {
    modules: RwLock<HashMap<CacheKey, Module>>,
}

impl RuntimeModuleCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_or_compile(
        &self,
        key: &(CapabilityId, Version),
        engine: &Engine,
        wasm_bytes: &[u8],
    ) -> Result<Module, super::RuntimeError> {
        let mut hasher = DefaultHasher::new();
        wasm_bytes.hash(&mut hasher);
        let content_hash = hasher.finish();
        let cache_key = (key.0.clone(), key.1.clone(), content_hash);
        if let Some(module) = self.modules.read().get(&cache_key) {
            return Ok(module.clone());
        }
        let module = Module::new(engine, wasm_bytes)
            .map_err(|e| super::RuntimeError::CompilationFailed(e.to_string()))?;
        self.modules.write().insert(cache_key, module.clone());
        Ok(module)
    }

    pub fn evict(&self, key: &(CapabilityId, Version)) {
        self.modules
            .write()
            .retain(|k, _| !(k.0 == key.0 && k.1 == key.1));
    }

    pub fn clear(&self) {
        self.modules.write().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    const VALID_WASM: &[u8] = &[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
    const INVALID_WASM: &[u8] = &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

    fn test_engine() -> Engine {
        Engine::default()
    }

    fn test_key(id: &str, ver: &str) -> (CapabilityId, Version) {
        (CapabilityId::new(id), Version::parse(ver).unwrap())
    }

    #[test]
    fn cache_miss_compiles() {
        let cache = RuntimeModuleCache::new();
        let engine = test_engine();
        let key = test_key("test.miss", "1.0.0");
        let result = cache.get_or_compile(&key, &engine, VALID_WASM);
        assert!(result.is_ok());
    }

    #[test]
    fn cache_hit_returns_same() {
        let cache = RuntimeModuleCache::new();
        let engine = test_engine();
        let key = test_key("test.hit", "1.0.0");
        let module_a = cache.get_or_compile(&key, &engine, VALID_WASM).unwrap();
        let module_b = cache.get_or_compile(&key, &engine, VALID_WASM).unwrap();
        let serialized_a = module_a.serialize().unwrap();
        let serialized_b = module_b.serialize().unwrap();
        assert_eq!(serialized_a, serialized_b);
    }

    #[test]
    fn eviction() {
        let cache = RuntimeModuleCache::new();
        let engine = test_engine();
        let key = test_key("test.evict", "1.0.0");
        cache.get_or_compile(&key, &engine, VALID_WASM).unwrap();
        cache.evict(&key);
        let result = cache.get_or_compile(&key, &engine, VALID_WASM);
        assert!(result.is_ok());
    }

    #[test]
    fn clear() {
        let cache = RuntimeModuleCache::new();
        let engine = test_engine();
        let key1 = test_key("test.clear1", "1.0.0");
        let key2 = test_key("test.clear2", "1.0.0");
        cache.get_or_compile(&key1, &engine, VALID_WASM).unwrap();
        cache.get_or_compile(&key2, &engine, VALID_WASM).unwrap();
        cache.clear();
        let r1 = cache.get_or_compile(&key1, &engine, VALID_WASM);
        let r2 = cache.get_or_compile(&key2, &engine, VALID_WASM);
        assert!(r1.is_ok());
        assert!(r2.is_ok());
    }

    #[test]
    fn concurrent_access_safety() {
        let cache = Arc::new(RuntimeModuleCache::new());
        let engine = Arc::new(test_engine());
        let mut handles = vec![];
        for i in 0..10 {
            let cache = cache.clone();
            let engine = engine.clone();
            handles.push(std::thread::spawn(move || {
                let key = test_key(&format!("test.concurrent.{i}"), "1.0.0");
                cache.get_or_compile(&key, &engine, VALID_WASM).unwrap();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
    }

    #[test]
    fn invalid_wasm_returns_error() {
        let cache = RuntimeModuleCache::new();
        let engine = test_engine();
        let key = test_key("test.invalid", "1.0.0");
        let result = cache.get_or_compile(&key, &engine, INVALID_WASM);
        assert!(result.is_err());
        match result {
            Err(crate::runtime::RuntimeError::CompilationFailed(_)) => {}
            _ => panic!("expected CompilationFailed"),
        }
    }

    #[test]
    fn evict_nonexistent_key() {
        let cache = RuntimeModuleCache::new();
        let key = test_key("test.nonexistent", "1.0.0");
        cache.evict(&key);
    }

    #[test]
    fn clear_empty_cache() {
        let cache = RuntimeModuleCache::new();
        cache.clear();
    }
}
