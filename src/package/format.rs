use std::io::{Read, Write};
use std::path::Path;
use serde::{Deserialize, Serialize};
use fusion_plugin_api::{CapabilityContract, CapabilityId};

use crate::package::PackageError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    pub version: semver::Version,
    #[serde(default)]
    pub capabilities: Vec<CapabilityContract>,
    #[serde(default)]
    pub dependencies: Vec<CapabilityDependency>,
    #[serde(default)]
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityDependency {
    pub id: CapabilityId,
    pub version_req: semver::VersionReq,
}

#[derive(Debug)]
pub struct PackageArchive {
    pub manifest: Vec<u8>,
    pub wasm: Vec<u8>,
    pub attestation: Vec<u8>,
}

pub fn extract_package(path: &Path) -> Result<PackageArchive, PackageError> {
    let file = std::fs::File::open(path)?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);

    let mut manifest: Option<Vec<u8>> = None;
    let mut wasm: Option<Vec<u8>> = None;
    let mut attestation: Option<Vec<u8>> = None;

    for entry in archive.entries()? {
        let mut entry = entry.map_err(|e| PackageError::InvalidArchive(e.to_string()))?;
        let path = entry.path()
            .map_err(|e| PackageError::InvalidArchive(e.to_string()))?
            .to_string_lossy()
            .to_string();

        let mut buf = Vec::new();
        entry.read_to_end(&mut buf)?;

        match path.as_str() {
            "manifest.toml" => manifest = Some(buf),
            "module.wasm" => wasm = Some(buf),
            "attestation.json" => attestation = Some(buf),
            _ => {}
        }
    }

    let manifest = manifest.ok_or_else(|| PackageError::MissingFile("manifest.toml".into()))?;
    let wasm = wasm.ok_or_else(|| PackageError::MissingFile("module.wasm".into()))?;
    let attestation = attestation.ok_or_else(|| PackageError::MissingFile("attestation.json".into()))?;

    Ok(PackageArchive { manifest, wasm, attestation })
}

pub fn parse_manifest(bytes: &[u8]) -> Result<Manifest, PackageError> {
    let s = std::str::from_utf8(bytes)
        .map_err(|e| PackageError::ManifestParse(e.to_string()))?;
    let manifest: Manifest = toml::from_str(s)
        .map_err(|e| PackageError::ManifestParse(e.to_string()))?;
    if manifest.capabilities.is_empty() {
        return Err(PackageError::ManifestParse(
            "manifest must declare at least one capability".into(),
        ));
    }
    Ok(manifest)
}

#[cfg(test)]
pub fn build_test_fusionpkg(
    name: &str,
    version: &str,
    capabilities: &[CapabilityContract],
    permissions: &[String],
) -> Vec<u8> {
    let manifest = Manifest {
        name: name.to_string(),
        version: semver::Version::parse(version).unwrap(),
        capabilities: capabilities.to_vec(),
        dependencies: vec![],
        permissions: permissions.to_vec(),
    };
    let manifest_toml = toml::to_string(&manifest).unwrap();
    let wasm_bytes = wat::parse_str("(module)").unwrap();
    use crate::release::signing::Signer;
    let signer = crate::release::signing::MockSigner::new("test-key");
    let assessment = crate::release::assessment::ReleaseAssessment::new(
        crate::release::policy::ReleaseEnvironment::Development,
        crate::release::evaluator::PolicyEvaluation {
            environment: crate::release::policy::ReleaseEnvironment::Development,
            decision: crate::release::evaluator::ReleaseDecision::Approved,
            summary: Default::default(),
            required_failures: vec![],
            waived_failures: vec![],
            advisory_failures: vec![],
            passed_gates: vec![],
        },
        vec![],
    );
    let attestation = crate::release::attestation::ReleaseAttestation::new(assessment);
    let canonical = crate::release::attestation::AttestationBuilder::to_canonical_bytes(&attestation).unwrap();
    let signature = signer.sign(&canonical).unwrap();
    let envelope = crate::release::envelope::AttestationEnvelope::new(
        crate::release::signing::SignedAttestation {
            attestation,
            signature,
        },
    );
    let attestation_json = serde_json::to_vec(&envelope).unwrap();

    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_path("manifest.toml").unwrap();
    header.set_size(manifest_toml.len() as u64);
    header.set_cksum();
    builder.append(&header, manifest_toml.as_bytes()).unwrap();

    let mut header = tar::Header::new_gnu();
    header.set_path("module.wasm").unwrap();
    header.set_size(wasm_bytes.len() as u64);
    header.set_cksum();
    builder.append(&header, &wasm_bytes[..]).unwrap();

    let mut header = tar::Header::new_gnu();
    header.set_path("attestation.json").unwrap();
    header.set_size(attestation_json.len() as u64);
    header.set_cksum();
    builder.append(&header, &attestation_json[..]).unwrap();

    let uncompressed = builder.into_inner().unwrap();
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&uncompressed).unwrap();
    encoder.finish().unwrap()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn test_extract_valid_package() {
        let pkg_bytes = build_test_fusionpkg("test.cap", "0.1.0", &[], &[]);
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &pkg_bytes).unwrap();
        let archive = extract_package(tmp.path()).unwrap();
        assert!(String::from_utf8_lossy(&archive.manifest).contains("name"));
        assert!(archive.wasm.len() > 0);
        assert!(archive.attestation.len() > 0);
    }

    #[test]
    fn test_extract_missing_manifest() {
        let wasm_bytes = wat::parse_str("(module)").unwrap();
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_path("module.wasm").unwrap();
        header.set_size(wasm_bytes.len() as u64);
        header.set_cksum();
        builder.append(&header, &wasm_bytes[..]).unwrap();
        let attestation = b"{}";
        let mut header2 = tar::Header::new_gnu();
        header2.set_path("attestation.json").unwrap();
        header2.set_size(attestation.len() as u64);
        header2.set_cksum();
        builder.append(&header2, &attestation[..]).unwrap();
        let uncompressed = builder.into_inner().unwrap();
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&uncompressed).unwrap();
        let pkg_bytes = encoder.finish().unwrap();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &pkg_bytes).unwrap();
        match extract_package(tmp.path()) {
            Err(PackageError::MissingFile(_)) => {}
            _ => panic!("expected MissingFile error for missing manifest.toml"),
        }
    }

    #[test]
    fn test_extract_invalid_archive() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"not-a-gzip-file").unwrap();
        match extract_package(tmp.path()) {
            Err(PackageError::InvalidArchive(_)) => {}
            _ => panic!("expected InvalidArchive error"),
        }
    }

    #[test]
    fn test_parse_manifest_valid() {
        let toml_str = r#"
name = "test-capability"
version = "0.1.0"

[[capabilities]]
id = "test.cap"
version = "0.1.0"
description = "A test capability"
inputs_schema = {}
outputs_schema = {}
permissions = ["Network"]
dependencies = []
estimated_cost_usd = 0.001
estimated_latency_ms = 50
reliability_score = 0.99
supports_streaming = false
"#;
        let manifest = parse_manifest(toml_str.as_bytes()).unwrap();
        assert_eq!(manifest.name, "test-capability");
        assert_eq!(manifest.version.to_string(), "0.1.0");
        assert_eq!(manifest.capabilities.len(), 1);
        assert_eq!(manifest.capabilities[0].id.to_string(), "test.cap");
    }

    #[test]
    fn test_parse_manifest_missing_field() {
        let toml_str = "name = \"broken\"\n";
        match parse_manifest(toml_str.as_bytes()) {
            Err(PackageError::ManifestParse(_)) => {}
            _ => panic!("expected ManifestParse error"),
        }
    }
}
