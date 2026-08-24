use crate::operations::{OperationError, PackageVerifier};
use crate::telemetry::audit::{AuditEntry, AuditLog};
use std::sync::Arc;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PackageAttestationStatus {
    pub package_id: String,
    pub version: String,
    pub schema_valid: bool,
    pub signature_valid: bool,
    pub semantic_valid: bool,
    pub last_verified: i64,
}

pub struct AttestationViewer {
    verifier: Arc<dyn PackageVerifier>,
    #[allow(dead_code)]
    audit_log: Arc<AuditLog>,
}

impl AttestationViewer {
    pub fn new(verifier: Arc<dyn PackageVerifier>, audit_log: Arc<AuditLog>) -> Self {
        Self {
            verifier,
            audit_log,
        }
    }

    pub fn list_packages(&self) -> Result<Vec<PackageAttestationStatus>, OperationError> {
        let packages = self.verifier.verified_packages();
        let statuses = packages
            .into_iter()
            .map(|(id, ver)| {
                let now = chrono::Utc::now().timestamp();
                match self.verifier.verify_package(&id, &ver) {
                    Ok(v) => PackageAttestationStatus {
                        package_id: id,
                        version: ver,
                        schema_valid: v.schema_valid,
                        signature_valid: v.signature_valid,
                        semantic_valid: v.semantic_valid,
                        last_verified: now,
                    },
                    Err(_) => PackageAttestationStatus {
                        package_id: id,
                        version: ver,
                        schema_valid: false,
                        signature_valid: false,
                        semantic_valid: false,
                        last_verified: now,
                    },
                }
            })
            .collect();
        Ok(statuses)
    }

    #[allow(dead_code)]
    pub fn re_verify(
        &self,
        package_id: &str,
        version: &str,
    ) -> Result<PackageAttestationStatus, OperationError> {
        let verification = self.verifier.verify_package(package_id, version)?;
        self.audit_log.record(AuditEntry {
            timestamp: chrono::Utc::now().timestamp(),
            request_id: String::new(),
            user_id: None,
            action: format!("attestation.reverify:{}@{}", package_id, version),
            result: "ok".into(),
            details: serde_json::json!({"package_id": package_id, "version": version}),
        });
        Ok(PackageAttestationStatus {
            package_id: package_id.into(),
            version: version.into(),
            schema_valid: verification.schema_valid,
            signature_valid: verification.signature_valid,
            semantic_valid: verification.semantic_valid,
            last_verified: chrono::Utc::now().timestamp(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::MockPackageVerifier;
    use crate::telemetry::audit::AuditLog;

    #[test]
    fn test_list_attestations_empty() {
        let audit = Arc::new(AuditLog::new(100));
        let verifier = Arc::new(MockPackageVerifier);
        let viewer = AttestationViewer::new(verifier, audit);
        let list = viewer.list_packages().unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn test_attestation_status_fields() {
        let status = PackageAttestationStatus {
            package_id: "test.pkg".into(),
            version: "1.0.0".into(),
            schema_valid: true,
            signature_valid: true,
            semantic_valid: true,
            last_verified: 1000,
        };
        assert_eq!(status.package_id, "test.pkg");
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("schema_valid"));
    }
}
