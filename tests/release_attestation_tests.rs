use fusion_router::release::archive::{ArchiveBackend, FilesystemArchiveBackend};
use fusion_router::release::assessment::ReleaseAssessment;
use fusion_router::release::attestation::{AttestationBuilder, ReleaseAttestation};
use fusion_router::release::bootstrap::build_default_runner;
use fusion_router::release::envelope::AttestationEnvelope;
use fusion_router::release::evaluator::{EvaluationContext, PolicyEvaluator};
use fusion_router::release::gate::GateContext;
use fusion_router::release::policy::{PolicyDefinition, ReleaseEnvironment};
use fusion_router::release::signing::{MockSigner, SignedAttestation, Signer};
use fusion_router::release::verifier::AttestationVerifier;
use fusion_router::release::waiver::WaiverSet;
use uuid::Uuid;

#[tokio::test]
async fn test_end_to_end_attestation_flow() {
    let workspace_root = std::path::PathBuf::from(".");
    let runner = build_default_runner(workspace_root.clone(), "HEAD");
    let gate_ctx = GateContext {
        workspace_root: workspace_root.clone(),
        baseline_version: None,
    };

    let gate_results = runner.run_all(&gate_ctx).await;
    let eval_ctx = EvaluationContext::new(
        ReleaseEnvironment::Production,
        PolicyDefinition::default_policy(),
        WaiverSet::default(),
    );
    let policy_eval = PolicyEvaluator::evaluate(&eval_ctx, &gate_results);

    let assessment = ReleaseAssessment::new(ReleaseEnvironment::Production, policy_eval, vec![]);
    let attestation = ReleaseAttestation::new(assessment);
    let canonical_bytes = AttestationBuilder::to_canonical_bytes(&attestation).unwrap();

    let signer = MockSigner::default();
    let signature = signer.sign(&canonical_bytes).unwrap();
    let signed = SignedAttestation {
        attestation,
        signature,
    };
    let envelope = AttestationEnvelope::new(signed);

    let temp_path = std::env::temp_dir().join(format!("fusion_test_e2e_{}", Uuid::new_v4()));
    let archive = FilesystemArchiveBackend::new(temp_path.clone());

    let assessment_id = &envelope
        .signed_attestation
        .attestation
        .assessment
        .assessment_id;
    assert!(!archive.exists(assessment_id));

    let stored_path = archive.store(&envelope).unwrap();
    assert!(stored_path.exists());
    assert!(archive.exists(assessment_id));

    // Overwrite rejection invariant
    let overwrite_err = archive.store(&envelope);
    assert!(overwrite_err.is_err());

    let loaded_envelope = archive.load(assessment_id).unwrap();
    let report = AttestationVerifier::verify(&loaded_envelope, &signer).unwrap();

    assert!(report.schema_valid);
    assert!(report.canonical_valid);
    assert!(report.signature_valid);
    assert!(report.semantic_valid);

    let _ = std::fs::remove_dir_all(temp_path);
}

#[tokio::test]
async fn test_attestation_tampered_payload_rejected() {
    let workspace_root = std::path::PathBuf::from(".");
    let runner = build_default_runner(workspace_root.clone(), "HEAD");
    let gate_ctx = GateContext {
        workspace_root: workspace_root.clone(),
        baseline_version: None,
    };

    let gate_results = runner.run_all(&gate_ctx).await;
    let eval_ctx = EvaluationContext::new(
        ReleaseEnvironment::Production,
        PolicyDefinition::default_policy(),
        WaiverSet::default(),
    );
    let policy_eval = PolicyEvaluator::evaluate(&eval_ctx, &gate_results);

    let assessment = ReleaseAssessment::new(ReleaseEnvironment::Production, policy_eval, vec![]);
    let attestation = ReleaseAttestation::new(assessment);
    let canonical_bytes = AttestationBuilder::to_canonical_bytes(&attestation).unwrap();

    let signer = MockSigner::default();
    let signature = signer.sign(&canonical_bytes).unwrap();

    // Tamper with attestation schema version
    let mut tampered_attestation = attestation;
    tampered_attestation.schema_version = "invalid-schema-v999".into();
    let signed = SignedAttestation {
        attestation: tampered_attestation,
        signature,
    };
    let envelope = AttestationEnvelope::new(signed);

    let report = AttestationVerifier::verify(&envelope, &signer).unwrap();
    assert!(!report.schema_valid);
    assert!(!report.signature_valid);
}
