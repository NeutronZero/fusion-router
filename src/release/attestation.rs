use serde::{Deserialize, Serialize};
use crate::release::assessment::ReleaseAssessment;
use crate::release::gate::GateError;

pub const ATTESTATION_SCHEMA_VERSION: &str = "fusion.router.release.attestation.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostInfo {
    pub fusion_version: String,
    pub os: String,
    pub arch: String,
}

impl Default for HostInfo {
    fn default() -> Self {
        Self {
            fusion_version: env!("CARGO_PKG_VERSION").to_string(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseAttestation {
    pub schema_version: String,
    pub assessment: ReleaseAssessment,
    pub host_info: HostInfo,
}

impl ReleaseAttestation {
    pub fn new(assessment: ReleaseAssessment) -> Self {
        Self {
            schema_version: ATTESTATION_SCHEMA_VERSION.to_string(),
            assessment,
            host_info: HostInfo::default(),
        }
    }
}

pub struct AttestationBuilder;

impl AttestationBuilder {
    /// Sole authority for canonical JSON serialization prior to signing.
    pub fn to_canonical_bytes(attestation: &ReleaseAttestation) -> Result<Vec<u8>, GateError> {
        serde_json::to_vec(attestation)
            .map_err(|e| GateError::ExecutionFailed(format!("canonical serialization error: {e}")))
    }

    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<ReleaseAttestation, GateError> {
        serde_json::from_slice(bytes)
            .map_err(|e| GateError::ExecutionFailed(format!("deserialization error: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release::evaluator::{PolicyEvaluation, PolicySummary, ReleaseDecision};
    use crate::release::policy::ReleaseEnvironment;

    #[test]
    fn test_canonical_serialization_deterministic() {
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
        let attestation = ReleaseAttestation::new(assessment);

        let bytes1 = AttestationBuilder::to_canonical_bytes(&attestation).unwrap();
        let bytes2 = AttestationBuilder::to_canonical_bytes(&attestation).unwrap();

        assert_eq!(bytes1, bytes2);
        let restored = AttestationBuilder::from_canonical_bytes(&bytes1).unwrap();
        assert_eq!(restored.schema_version, ATTESTATION_SCHEMA_VERSION);
    }
}
