use std::sync::Arc;
use std::collections::HashMap;
use crate::operations::{OperationError, RuntimeModuleCache};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InstanceDetail {
    pub capability_id: String,
    pub version: String,
    pub memory_usage_bytes: u64,
    pub fuel_consumed: u64,
    pub invocation_count: u64,
    pub last_error: Option<String>,
    pub host_call_breakdown: HashMap<String, u64>,
    pub uptime_secs: f64,
}

pub struct RuntimeInspector {
    module_cache: Arc<RuntimeModuleCache>,
}

impl RuntimeInspector {
    pub fn new(module_cache: Arc<RuntimeModuleCache>) -> Self {
        Self { module_cache }
    }

    pub fn list_instances(&self) -> Result<Vec<InstanceDetail>, OperationError> {
        let keys = self.module_cache.keys();
        let details = keys.into_iter().map(|key| InstanceDetail {
            capability_id: key.0.to_string(),
            version: key.1.to_string(),
            memory_usage_bytes: 0,
            fuel_consumed: 0,
            invocation_count: 0,
            last_error: None,
            host_call_breakdown: HashMap::new(),
            uptime_secs: 0.0,
        }).collect();
        Ok(details)
    }

    #[allow(dead_code)]
    pub fn get_instance(&self, capability_id: &str) -> Result<Option<InstanceDetail>, OperationError> {
        let keys = self.module_cache.keys();
        for key in keys {
            if key.0.as_str() == capability_id {
                return Ok(Some(InstanceDetail {
                    capability_id: key.0.to_string(),
                    version: key.1.to_string(),
                    memory_usage_bytes: 0,
                    fuel_consumed: 0,
                    invocation_count: 0,
                    last_error: None,
                    host_call_breakdown: HashMap::new(),
                    uptime_secs: 0.0,
                }));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::{CacheKey, RuntimeModuleCache};

    #[test]
    fn test_inspector_empty() {
        let cache = Arc::new(RuntimeModuleCache::new());
        let inspector = RuntimeInspector::new(cache.clone());
        let instances = inspector.list_instances().unwrap();
        assert!(instances.is_empty());
    }

    #[test]
    fn test_inspector_with_registered_instance() {
        let cache = Arc::new(RuntimeModuleCache::new());
        cache.insert(CacheKey(
            fusion_plugin_api::CapabilityId::new("test.cap"),
            semver::Version::parse("0.1.0").unwrap(),
        ));
        let inspector = RuntimeInspector::new(cache.clone());
        let instances = inspector.list_instances().unwrap();
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].capability_id, "test.cap");
    }

    #[test]
    fn test_get_instance_found() {
        let cache = Arc::new(RuntimeModuleCache::new());
        cache.insert(CacheKey(
            fusion_plugin_api::CapabilityId::new("test.cap"),
            semver::Version::parse("0.1.0").unwrap(),
        ));
        let inspector = RuntimeInspector::new(cache.clone());

        let detail = inspector
            .get_instance("test.cap")
            .unwrap()
            .expect("instance should be found");
        assert_eq!(detail.capability_id, "test.cap");
        assert_eq!(detail.version, "0.1.0");
    }

    #[test]
    fn test_get_instance_not_found() {
        let cache = Arc::new(RuntimeModuleCache::new());
        cache.insert(CacheKey(
            fusion_plugin_api::CapabilityId::new("test.cap"),
            semver::Version::parse("0.1.0").unwrap(),
        ));
        let inspector = RuntimeInspector::new(cache.clone());

        let detail = inspector.get_instance("other.cap").unwrap();
        assert!(detail.is_none());
    }

    #[test]
    fn test_get_instance_empty_cache() {
        let cache = Arc::new(RuntimeModuleCache::new());
        let inspector = RuntimeInspector::new(cache.clone());

        let detail = inspector.get_instance("anything.cap").unwrap();
        assert!(detail.is_none());
    }
}
