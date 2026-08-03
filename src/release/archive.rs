use std::path::PathBuf;
use crate::release::envelope::AttestationEnvelope;
use crate::release::gate::GateError;

pub trait ArchiveBackend: Send + Sync {
    fn store(&self, envelope: &AttestationEnvelope) -> Result<PathBuf, GateError>;
    fn load(&self, assessment_id: &str) -> Result<AttestationEnvelope, GateError>;
    fn exists(&self, assessment_id: &str) -> bool;
    fn list(&self) -> Result<Vec<String>, GateError>;
}

pub struct FilesystemArchiveBackend {
    archive_dir: PathBuf,
}

impl FilesystemArchiveBackend {
    pub fn new(archive_dir: PathBuf) -> Self {
        Self { archive_dir }
    }

    fn validate_id(id: &str) -> Result<(), GateError> {
        if id.contains('/') || id.contains('\\') || id.contains("..") {
            return Err(GateError::ExecutionFailed("invalid assessment ID: path traversal detected".to_string()));
        }
        Ok(())
    }
}

impl ArchiveBackend for FilesystemArchiveBackend {
    fn store(&self, envelope: &AttestationEnvelope) -> Result<PathBuf, GateError> {
        let assessment_id = &envelope.signed_attestation.attestation.assessment.assessment_id;
        Self::validate_id(assessment_id)?;
        let file_name = format!("{assessment_id}.json");
        let target_path = self.archive_dir.join(file_name);

        if target_path.exists() {
            return Err(GateError::ExecutionFailed(format!(
                "append-only archive error: assessment {} already exists at {}",
                assessment_id,
                target_path.display()
            )));
        }

        if let Some(parent) = target_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let json = serde_json::to_string_pretty(envelope)
            .map_err(|e| GateError::ExecutionFailed(format!("serialize envelope error: {e}")))?;

        std::fs::write(&target_path, json)
            .map_err(|e| GateError::ExecutionFailed(format!("write envelope to {}: {e}", target_path.display())))?;

        Ok(target_path)
    }

    fn load(&self, assessment_id: &str) -> Result<AttestationEnvelope, GateError> {
        Self::validate_id(assessment_id)?;
        let file_name = format!("{assessment_id}.json");
        let target_path = if assessment_id.ends_with(".json") {
            self.archive_dir.join(assessment_id)
        } else {
            self.archive_dir.join(file_name)
        };

        if !target_path.exists() {
            return Err(GateError::ExecutionFailed(format!(
                "attestation not found at {}",
                target_path.display()
            )));
        }

        let content = std::fs::read_to_string(&target_path)
            .map_err(|e| GateError::ExecutionFailed(format!("read attestation {}: {e}", target_path.display())))?;

        serde_json::from_str(&content)
            .map_err(|e| GateError::ExecutionFailed(format!("parse attestation {}: {e}", target_path.display())))
    }

    fn exists(&self, assessment_id: &str) -> bool {
        if Self::validate_id(assessment_id).is_err() {
            return false;
        }
        let file_name = format!("{assessment_id}.json");
        let target_path = if assessment_id.ends_with(".json") {
            self.archive_dir.join(assessment_id)
        } else {
            self.archive_dir.join(file_name)
        };
        target_path.exists()
    }

    fn list(&self) -> Result<Vec<String>, GateError> {
        if !self.archive_dir.exists() {
            return Ok(vec![]);
        }

        let entries = std::fs::read_dir(&self.archive_dir)
            .map_err(|e| GateError::ExecutionFailed(format!("read_dir {}: {e}", self.archive_dir.display())))?;

        let mut results = Vec::new();
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.ends_with(".json") {
                    results.push(name.trim_end_matches(".json").to_string());
                }
            }
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    use crate::release::assessment::ReleaseAssessment;
    use crate::release::attestation::ReleaseAttestation;
    use crate::release::evaluator::{PolicyEvaluation, PolicySummary, ReleaseDecision};
    use crate::release::policy::ReleaseEnvironment;
    use crate::release::signing::{MockSigner, Signer};

    #[test]
    fn test_filesystem_archive_store_load_and_append_only() {
        let temp_path = std::env::temp_dir().join(format!("fusion_test_{}", Uuid::new_v4()));
        let archive = FilesystemArchiveBackend::new(temp_path.clone());

        let eval = PolicyEvaluation {
            environment: ReleaseEnvironment::Production,
            decision: ReleaseDecision::Approved,
            summary: PolicySummary::default(),
            required_failures: vec![],
            waived_failures: vec![],
            advisory_failures: vec![],
            passed_gates: vec![],
        };
        let assessment = ReleaseAssessment::new(ReleaseEnvironment::Production, eval, vec![]);
        let id = assessment.assessment_id.clone();
        let attestation = ReleaseAttestation::new(assessment);
        let signer = MockSigner::default();
        let canonical_bytes = crate::release::attestation::AttestationBuilder::to_canonical_bytes(&attestation).unwrap();
        let sig = signer.sign(&canonical_bytes).unwrap();
        let signed = crate::release::signing::SignedAttestation { attestation, signature: sig };
        let envelope = AttestationEnvelope::new(signed);

        assert!(!archive.exists(&id));
        let path = archive.store(&envelope).unwrap();
        assert!(path.exists());
        assert!(archive.exists(&id));

        // Test append-only overwrite rejection
        let overwrite_err = archive.store(&envelope);
        assert!(overwrite_err.is_err());

        // Test load and list
        let loaded = archive.load(&id).unwrap();
        assert_eq!(loaded.signed_attestation.attestation.assessment.assessment_id, id);
        let list = archive.list().unwrap();
        assert!(list.contains(&id));

        let _ = std::fs::remove_dir_all(temp_path);
    }

    #[test]
    fn test_filesystem_archive_path_traversal() {
        let temp_path = std::env::temp_dir().join(format!("fusion_test_{}", Uuid::new_v4()));
        let archive = FilesystemArchiveBackend::new(temp_path.clone());

        let eval = PolicyEvaluation {
            environment: ReleaseEnvironment::Production,
            decision: ReleaseDecision::Approved,
            summary: PolicySummary::default(),
            required_failures: vec![],
            waived_failures: vec![],
            advisory_failures: vec![],
            passed_gates: vec![],
        };
        let mut assessment = ReleaseAssessment::new(ReleaseEnvironment::Production, eval, vec![]);

        // Attempt path traversal
        assessment.assessment_id = "../../../etc/passwd".to_string();

        let id = assessment.assessment_id.clone();
        let attestation = ReleaseAttestation::new(assessment);
        let signer = MockSigner::default();
        let canonical_bytes = crate::release::attestation::AttestationBuilder::to_canonical_bytes(&attestation).unwrap();
        let sig = signer.sign(&canonical_bytes).unwrap();
        let signed = crate::release::signing::SignedAttestation { attestation, signature: sig };
        let envelope = AttestationEnvelope::new(signed);

        assert!(!archive.exists(&id));

        let store_err = archive.store(&envelope);
        assert!(store_err.is_err());
        assert!(store_err.unwrap_err().to_string().contains("path traversal"));

        let load_err = archive.load(&id);
        assert!(load_err.is_err());
        assert!(load_err.unwrap_err().to_string().contains("path traversal"));

        let _ = std::fs::remove_dir_all(temp_path);
    }
}
