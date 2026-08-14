#![cfg(feature = "wasm-plugins")]

use std::io::Write;
use std::sync::{Arc, RwLock};

use fusion_plugin_api::{CapabilityContract, CapabilityId, Permission};
use fusion_router::capability::{CapabilityRegistry, InMemoryCapabilityRegistry};
use fusion_router::package::{
    FilesystemPackageRegistry, PackageLoader, PackageRegistry, PackageVerifier,
    RuntimeModuleCache, format::Manifest,
};

fn build_signed_fusionpkg(
    name: &str,
    version: &str,
    capabilities: &[CapabilityContract],
    permissions: &[String],
) -> Vec<u8> {
    use flate2::write::GzEncoder;
    use flate2::Compression;

    let manifest = Manifest {
        name: name.to_string(),
        version: semver::Version::parse(version).unwrap(),
        capabilities: capabilities.to_vec(),
        dependencies: vec![],
        permissions: permissions.to_vec(),
    };
    let manifest_toml = toml::to_string(&manifest).unwrap();
    let wasm_bytes = wat::parse_str("(module)").unwrap();

    let signer = fusion_router::release::signing::MockSigner::new("integration-key");
    use fusion_router::release::Signer;
    let assessment = fusion_router::release::assessment::ReleaseAssessment::new(
        fusion_router::release::policy::ReleaseEnvironment::Development,
        fusion_router::release::evaluator::PolicyEvaluation {
            environment: fusion_router::release::policy::ReleaseEnvironment::Development,
            decision: fusion_router::release::evaluator::ReleaseDecision::Approved,
            summary: Default::default(),
            required_failures: vec![],
            waived_failures: vec![],
            advisory_failures: vec![],
            passed_gates: vec![],
        },
        vec![],
    );
    let attestation = fusion_router::release::attestation::ReleaseAttestation::new(assessment);
    let canonical = fusion_router::release::attestation::AttestationBuilder::to_canonical_bytes(&attestation).unwrap();
    let signature = signer.sign(&canonical).unwrap();
    let signed = fusion_router::release::signing::SignedAttestation {
        attestation,
        signature,
    };
    let envelope = fusion_router::release::envelope::AttestationEnvelope::new(signed);
    let attestation_json = serde_json::to_vec(&envelope).unwrap();

    let mut builder = tar::Builder::new(Vec::new());
    let mut h = tar::Header::new_gnu();
    h.set_path("manifest.toml").unwrap();
    h.set_size(manifest_toml.len() as u64); h.set_cksum();
    builder.append(&h, manifest_toml.as_bytes()).unwrap();

    let mut h = tar::Header::new_gnu();
    h.set_path("module.wasm").unwrap();
    h.set_size(wasm_bytes.len() as u64); h.set_cksum();
    builder.append(&h, &wasm_bytes[..]).unwrap();

    let mut h = tar::Header::new_gnu();
    h.set_path("attestation.json").unwrap();
    h.set_size(attestation_json.len() as u64); h.set_cksum();
    builder.append(&h, &attestation_json[..]).unwrap();

    let uncompressed = builder.into_inner().unwrap();
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&uncompressed).unwrap();
    encoder.finish().unwrap()
}

#[test]
fn test_full_verify_load_resolve_cycle() {
    let contract = CapabilityContract {
        id: CapabilityId::new("test.echo"),
        version: semver::Version::parse("0.1.0").unwrap(),
        description: "echo capability for integration test".into(),
        inputs_schema: serde_json::json!({"type": "object"}),
        outputs_schema: serde_json::json!({"type": "object"}),
        permissions: vec![Permission::Network],
        dependencies: vec![],
        estimated_cost: fusion_core::NanoUSD::from_nanos(1_000_000),
        estimated_latency_ms: 10,
        reliability_score: 0.99,
        supports_streaming: false,
        traits: vec![],
    };
    let pkg_bytes = build_signed_fusionpkg(
        "test.echo", "0.1.0", &[contract], &["Network".into()],
    );

    let pkg_dir = tempfile::tempdir().unwrap();
    let pkg_path = pkg_dir.path().join("test.echo-0.1.0.fusionpkg");
    std::fs::write(&pkg_path, &pkg_bytes).unwrap();

    let signer = Arc::new(fusion_router::release::signing::MockSigner::new("integration-key"));
    let verifier = PackageVerifier::new(signer);
    let verified = verifier.verify(&pkg_path).unwrap();
    assert_eq!(verified.manifest().name, "test.echo");

    let registry = Arc::new(RwLock::new(InMemoryCapabilityRegistry::new()));
    let cache = Arc::new(RuntimeModuleCache::new());
    let loader = PackageLoader::new(registry.clone(), cache);
    let cap_id = loader.load(verified).unwrap();
    assert_eq!(cap_id.to_string(), "test.echo");

    let reg = registry.read().unwrap();
    assert!(reg.contains(&CapabilityId::new("test.echo")));
    let stored = reg.get(&CapabilityId::new("test.echo")).unwrap();
    assert_eq!(stored.description, "echo capability for integration test");
    drop(reg);

    let reg_dir = tempfile::tempdir().unwrap();
    let pkg_reg = FilesystemPackageRegistry::new(reg_dir.path());
    pkg_reg.store(&cap_id, &semver::Version::new(0, 1, 0), &pkg_bytes).unwrap();
    assert!(pkg_reg.contains(&cap_id, &semver::Version::new(0, 1, 0)).unwrap());
    let loaded_pkg = pkg_reg.load(&cap_id, &semver::Version::new(0, 1, 0)).unwrap();
    assert_eq!(loaded_pkg, pkg_bytes);
}

#[test]
fn test_verify_rejects_missing_attestation() {
    let mut builder = tar::Builder::new(Vec::new());
    let manifest = r#"name = "bad" version = "0.1.0""#;
    let wasm = wat::parse_str("(module)").unwrap();
    let mut h = tar::Header::new_gnu();
    h.set_path("manifest.toml").unwrap(); h.set_size(manifest.len() as u64); h.set_cksum();
    builder.append(&h, manifest.as_bytes()).unwrap();
    let mut h = tar::Header::new_gnu();
    h.set_path("module.wasm").unwrap(); h.set_size(wasm.len() as u64); h.set_cksum();
    builder.append(&h, &wasm[..]).unwrap();
    let uncompressed = builder.into_inner().unwrap();
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&uncompressed).unwrap();
    let bad_pkg = encoder.finish().unwrap();

    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), &bad_pkg).unwrap();
    let signer = Arc::new(fusion_router::release::signing::MockSigner::new("k"));
    let verifier = PackageVerifier::new(signer);
    let result = verifier.verify(tmp.path());
    assert!(result.is_err(), "expected error for missing attestation");
}

#[test]
fn test_registry_list_versions_returns_sorted() {
    let dir = tempfile::tempdir().unwrap();
    let reg = FilesystemPackageRegistry::new(dir.path());
    let id = CapabilityId::new("test.multi");

    reg.store(&id, &semver::Version::new(0, 3, 0), b"v3").unwrap();
    reg.store(&id, &semver::Version::new(0, 1, 0), b"v1").unwrap();
    reg.store(&id, &semver::Version::new(0, 2, 0), b"v2").unwrap();

    let versions = reg.list_versions(&id).unwrap();
    assert_eq!(versions, vec![
        semver::Version::new(0, 1, 0),
        semver::Version::new(0, 2, 0),
        semver::Version::new(0, 3, 0),
    ]);
}
