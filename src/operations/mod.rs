pub mod attestation_viewer;
pub mod dashboard;
pub mod handlers;
pub mod policy_admin;
pub mod runtime_inspector;

use fusion_plugin_api::CapabilityId;
use parking_lot::RwLock;
use std::collections::HashMap;

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
        Self {
            modules: RwLock::new(HashMap::new()),
        }
    }

    pub fn len(&self) -> usize {
        self.modules.read().len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.modules.read().is_empty()
    }

    pub fn insert(&self, key: CacheKey) {
        self.modules.write().insert(key, ());
    }

    #[allow(dead_code)]
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

#[derive(Debug, Clone)]
pub struct PackageVerification {
    pub schema_valid: bool,
    pub signature_valid: bool,
    pub semantic_valid: bool,
}

pub trait PackageVerifier: Send + Sync {
    fn verified_packages(&self) -> Vec<(String, String)>;
    fn verify_package(
        &self,
        package_id: &str,
        version: &str,
    ) -> Result<PackageVerification, OperationError>;
}

/// Archive-backed verifier: loads the attestation envelope for a package and
/// runs the 4-phase verification pipeline with a real HMAC-SHA256 signer.
/// With no signing key configured, verification is refused (never fabricated).
pub struct ArchivePackageVerifier {
    archive: crate::release::archive::FilesystemArchiveBackend,
    signer: Option<std::sync::Arc<dyn crate::release::signing::Signer>>,
}

impl ArchivePackageVerifier {
    pub fn new(
        archive: crate::release::archive::FilesystemArchiveBackend,
        signer: Option<std::sync::Arc<dyn crate::release::signing::Signer>>,
    ) -> Self {
        Self { archive, signer }
    }
}

impl PackageVerifier for ArchivePackageVerifier {
    fn verified_packages(&self) -> Vec<(String, String)> {
        use crate::release::archive::ArchiveBackend;
        self.archive
            .list()
            .map(|ids| ids.into_iter().map(|id| (id, "unknown".into())).collect())
            .unwrap_or_default()
    }

    fn verify_package(
        &self,
        package_id: &str,
        _version: &str,
    ) -> Result<PackageVerification, OperationError> {
        use crate::release::archive::ArchiveBackend;
        let Some(signer) = &self.signer else {
            return Err(OperationError::Registry(
                "attestation verification unavailable: FUSION_SIGNING_KEY not set".into(),
            ));
        };
        let envelope = self
            .archive
            .load(package_id)
            .map_err(|e| OperationError::Registry(e.to_string()))?;
        let report =
            crate::release::verifier::AttestationVerifier::verify(&envelope, signer.as_ref())
                .map_err(|e| OperationError::Registry(e.to_string()))?;
        let verification = PackageVerification {
            schema_valid: report.schema_valid,
            signature_valid: report.signature_valid,
            semantic_valid: report.semantic_valid,
        };
        if !(verification.schema_valid
            && verification.signature_valid
            && verification.semantic_valid)
        {
            return Err(OperationError::Registry(format!(
                "attestation {package_id} failed verification: {}",
                report.summary
            )));
        }
        Ok(verification)
    }
}

pub struct MockPackageVerifier;

impl PackageVerifier for MockPackageVerifier {
    fn verified_packages(&self) -> Vec<(String, String)> {
        vec![]
    }
    fn verify_package(
        &self,
        _package_id: &str,
        _version: &str,
    ) -> Result<PackageVerification, OperationError> {
        Ok(PackageVerification {
            schema_valid: true,
            signature_valid: true,
            semantic_valid: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release::signing::HmacSha256Signer;
    use std::sync::Arc;

    #[test]
    fn test_archive_package_verifier_refuses_without_key() {
        let archive = crate::release::archive::FilesystemArchiveBackend::new(
            std::env::temp_dir().join(format!("fusion_ops_nokey_{}", std::process::id())),
        );
        let verifier = ArchivePackageVerifier::new(archive, None);
        let result = verifier.verify_package("some-pkg", "1.0.0");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("FUSION_SIGNING_KEY"));
    }

    #[test]
    fn test_archive_package_verifier_rejects_missing_attestation() {
        let archive = crate::release::archive::FilesystemArchiveBackend::new(
            std::env::temp_dir().join(format!("fusion_ops_missing_{}", std::process::id())),
        );
        let signer = Arc::new(HmacSha256Signer::new("ops", b"secret-key"));
        let verifier = ArchivePackageVerifier::new(archive, Some(signer));
        let result = verifier.verify_package("does-not-exist", "1.0.0");
        assert!(result.is_err());
    }

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
        let w = TimeWindow {
            start_secs: 0,
            end_secs: 3600,
        };
        let json = serde_json::to_string(&w).unwrap();
        assert!(json.contains("3600"));
    }
}
