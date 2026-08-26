use crate::release::attestation::{AttestationBuilder, ATTESTATION_SCHEMA_VERSION};
use crate::release::envelope::{AttestationEnvelope, ENVELOPE_VERSION};
use crate::release::gate::GateError;
use crate::release::signing::Signer;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationReport {
    pub schema_valid: bool,
    pub canonical_valid: bool,
    pub signature_valid: bool,
    pub semantic_valid: bool,
    pub summary: String,
}

pub struct AttestationVerifier;

impl AttestationVerifier {
    /// Four-Phase Verification Pipeline:
    /// 1. Schema Validation
    /// 2. Canonical Serialization
    /// 3. Cryptographic Verification
    /// 4. Semantic Consistency Check
    pub fn verify(
        envelope: &AttestationEnvelope,
        signer: &dyn Signer,
    ) -> Result<VerificationReport, GateError> {
        // Phase 1: Schema Validation
        let schema_valid = envelope.envelope_version == ENVELOPE_VERSION
            && envelope.signed_attestation.attestation.schema_version == ATTESTATION_SCHEMA_VERSION;

        if !schema_valid {
            return Ok(VerificationReport {
                schema_valid: false,
                canonical_valid: false,
                signature_valid: false,
                semantic_valid: false,
                summary: "Phase 1 Failed: Schema version mismatch".into(),
            });
        }

        // Phase 2: Canonical Serialization
        let canonical_bytes = match AttestationBuilder::to_canonical_bytes(
            &envelope.signed_attestation.attestation,
        ) {
            Ok(bytes) => bytes,
            Err(e) => {
                return Ok(VerificationReport {
                    schema_valid: true,
                    canonical_valid: false,
                    signature_valid: false,
                    semantic_valid: false,
                    summary: format!("Phase 2 Failed: Canonical serialization: {e}"),
                });
            }
        };

        // Phase 3: Cryptographic Verification
        let signature_valid = signer
            .verify(&canonical_bytes, &envelope.signed_attestation.signature)
            .unwrap_or_default();

        if !signature_valid {
            return Ok(VerificationReport {
                schema_valid: true,
                canonical_valid: true,
                signature_valid: false,
                semantic_valid: false,
                summary: "Phase 3 Failed: Cryptographic signature verification failed".into(),
            });
        }

        // Phase 4: Semantic Consistency Check
        let eval = &envelope
            .signed_attestation
            .attestation
            .assessment
            .policy_evaluation;
        let semantic_valid = match eval.decision {
            crate::release::evaluator::ReleaseDecision::Approved => {
                eval.required_failures.is_empty()
            }
            crate::release::evaluator::ReleaseDecision::ApprovedWithWaivers => {
                !eval.waived_failures.is_empty()
            }
            crate::release::evaluator::ReleaseDecision::Blocked => {
                !eval.required_failures.is_empty()
            }
        };

        let summary = if semantic_valid {
            "Attestation 4-phase verification succeeded".into()
        } else {
            "Phase 4 Warning: Semantic decision inconsistency".into()
        };

        Ok(VerificationReport {
            schema_valid: true,
            canonical_valid: true,
            signature_valid: true,
            semantic_valid,
            summary,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release::assessment::ReleaseAssessment;
    use crate::release::attestation::ReleaseAttestation;
    use crate::release::evaluator::{PolicyEvaluation, PolicySummary, ReleaseDecision};
    use crate::release::policy::ReleaseEnvironment;
    use crate::release::signing::MockSigner;

    #[test]
    fn test_verifier_4_phase_success() {
        let eval = PolicyEvaluation {
            environment: ReleaseEnvironment::Production,
            decision: ReleaseDecision::Approved,
            summary: PolicySummary::default(),
            reason: None,
            required_failures: vec![],
            waived_failures: vec![],
            advisory_failures: vec![],
            passed_gates: vec![],
        };
        let assessment = ReleaseAssessment::new(ReleaseEnvironment::Production, eval, vec![]);
        let attestation = ReleaseAttestation::new(assessment);
        let signer = MockSigner::default();

        let canonical_bytes = AttestationBuilder::to_canonical_bytes(&attestation).unwrap();
        let sig = signer.sign(&canonical_bytes).unwrap();
        let signed = crate::release::signing::SignedAttestation {
            attestation,
            signature: sig,
        };
        let envelope = AttestationEnvelope::new(signed);

        let report = AttestationVerifier::verify(&envelope, &signer).unwrap();
        assert!(report.schema_valid);
        assert!(report.canonical_valid);
        assert!(report.signature_valid);
        assert!(report.semantic_valid);
    }
}
