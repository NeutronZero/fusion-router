pub mod dashboard;
pub mod runtime_inspector;
pub mod policy_admin;
pub mod attestation_viewer;
pub mod handlers;

use std::collections::HashMap;
use fusion_plugin_api::CapabilityId;
use parking_lot::RwLock;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum OperationError {
    Registry(String),
    Runtime(String),
    Policy(String),
    Attestation(String),
    Internal(String),
}

impl std::fmt::Display for OperationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OperationError::Registry(msg) => write!(f, "Registry error: {msg}"),
            OperationError::Runtime(msg) => write!(f, "Runtime error: {msg}"),
            OperationError::Policy(msg) => write!(f, "Policy error: {msg}"),
            OperationError::Attestation(msg) => write!(f, "Attestation error: {msg}"),
            OperationError::Internal(msg) => write!(f, "Internal error: {msg}"),
        }
    }
}

impl std::error::Error for OperationError {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RegistrySummary {
    pub total_capabilities: usize,
    pub by_source: HashMap<String, usize>,
    pub frozen: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RuntimeSummary {
    pub loaded_instances: usize,
    pub total_memory_bytes: u64,
    pub total_fuel_consumed: u64,
    pub active_sessions: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InvocationMetric {
    pub capability_id: String,
    pub invocation_count: u64,
    pub error_count: u64,
    pub avg_latency_ms: f64,
    pub window_start_secs: i64,
    pub window_end_secs: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TimeWindow {
    pub start_secs: i64,
    pub end_secs: i64,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct CacheKey(pub CapabilityId, pub semver::Version);

#[derive(Debug)]
pub struct RuntimeModuleCache {
    modules: RwLock<HashMap<CacheKey, ()>>,
}

impl RuntimeModuleCache {
    pub fn new() -> Self {
        Self { modules: RwLock::new(HashMap::new()) }
    }

    pub fn len(&self) -> usize {
        self.modules.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.modules.read().is_empty()
    }

    pub fn insert(&self, key: CacheKey) {
        self.modules.write().insert(key, ());
    }

    pub fn clear(&self) {
        self.modules.write().clear();
    }

    pub fn keys(&self) -> Vec<CacheKey> {
        self.modules.read().keys().cloned().collect()
    }
}

impl Default for RuntimeModuleCache {
    fn default() -> Self {
        Self::new()
    }
}

pub trait PackageVerifier: Send + Sync {
    fn verified_packages(&self) -> Vec<(String, String)>;
    fn verify_package(&self, package_id: &str, version: &str) -> Result<(), OperationError>;
}

pub struct MockPackageVerifier;

impl PackageVerifier for MockPackageVerifier {
    fn verified_packages(&self) -> Vec<(String, String)> {
        vec![]
    }
    fn verify_package(&self, _package_id: &str, _version: &str) -> Result<(), OperationError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operation_error_display() {
        let err = OperationError::Registry("not found".into());
        let msg = err.to_string();
        assert!(msg.contains("Registry error"));
        assert!(msg.contains("not found"));
    }

    #[test]
    fn test_registry_summary_default() {
        let mut by_source = HashMap::new();
        by_source.insert("builtin".into(), 5);
        let summary = RegistrySummary {
            total_capabilities: 10,
            by_source,
            frozen: false,
        };
        assert_eq!(summary.total_capabilities, 10);
        assert_eq!(summary.by_source.get("builtin"), Some(&5));
        assert!(!summary.frozen);
    }

    #[test]
    fn test_runtime_summary_empty() {
        let summary = RuntimeSummary {
            loaded_instances: 0,
            total_memory_bytes: 0,
            total_fuel_consumed: 0,
            active_sessions: 0,
        };
        assert_eq!(summary.loaded_instances, 0);
    }

    #[test]
    fn test_invocation_metric_roundtrip() {
        let m = InvocationMetric {
            capability_id: "test.cap".into(),
            invocation_count: 42,
            error_count: 2,
            avg_latency_ms: 150.5,
            window_start_secs: 1000,
            window_end_secs: 2000,
        };
        let json = serde_json::to_string(&m).unwrap();
        let back: InvocationMetric = serde_json::from_str(&json).unwrap();
        assert_eq!(back.capability_id, "test.cap");
        assert_eq!(back.invocation_count, 42);
    }

    #[test]
    fn test_time_window_serde() {
        let w = TimeWindow { start_secs: 0, end_secs: 3600 };
        let json = serde_json::to_string(&w).unwrap();
        assert!(json.contains("3600"));
    }
}
