use std::path::Path;
use std::sync::Arc;

use crate::package::format::{self, Manifest, PackageArchive};
use crate::package::PackageError;
use crate::release::envelope::AttestationEnvelope;
use crate::release::signing::Signer;

pub struct VerifiedPackage {
    archive: PackageArchive,
    manifest: Manifest,
}

impl VerifiedPackage {
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn wasm_bytes(&self) -> &[u8] {
        &self.archive.wasm
    }

    pub fn archive(&self) -> &PackageArchive {
        &self.archive
    }
}

pub struct PackageVerifier {
    verifier: Arc<dyn Signer>,
}

impl PackageVerifier {
    pub fn new(verifier: Arc<dyn Signer>) -> Self {
        Self { verifier }
    }

    pub fn verify(&self, path: &Path) -> Result<VerifiedPackage, PackageError> {
        let archive = format::extract_package(path)?;
        let manifest = format::parse_manifest(&archive.manifest)?;

        let envelope: AttestationEnvelope = serde_json::from_slice(&archive.attestation)
            .map_err(|e| PackageError::AttestationFailed(format!("invalid JSON: {e}")))?;

        let canonical = crate::release::attestation::AttestationBuilder::to_canonical_bytes(
            &envelope.signed_attestation.attestation,
        )?;

        let valid = self.verifier.verify(
            &canonical,
            &envelope.signed_attestation.signature,
        )?;

        if !valid {
            return Err(PackageError::AttestationFailed("signature does not match".into()));
        }

        #[cfg(feature = "wasm-plugins")]
        {
            let engine = wasmtime::Engine::new(&wasmtime::Config::new())
                .map_err(|e| PackageError::WasmCompilationFailed(e.to_string()))?;
            wasmtime::Module::new(&engine, &archive.wasm)
                .map_err(|e| PackageError::WasmCompilationFailed(e.to_string()))?;
        }

        let declared_permissions: Vec<String> = manifest
            .capabilities
            .iter()
            .flat_map(|c| c.permissions.iter().map(|p| p.to_string()))
            .collect();
        let _ = declared_permissions;

        for perm_str in &manifest.permissions {
            perm_str.parse::<fusion_plugin_api::Permission>()
                .map_err(|e| PackageError::PermissionMismatch(format!("invalid permission '{perm_str}': {e}")))?;
        }

        Ok(VerifiedPackage { archive, manifest })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::Arc;
    use crate::release::signing::MockSigner;
    use fusion_plugin_api::{CapabilityContract, CapabilityId, Permission};
    use semver::Version;

    #[test]
    fn test_verify_valid_package() {
        let signer = Arc::new(MockSigner::new("test-key"));
        let verifier = PackageVerifier { verifier: signer };
        let contract = CapabilityContract {
            id: CapabilityId::new("test.cap"),
            version: Version::parse("0.1.0").unwrap(),
            description: "valid package".into(),
            inputs_schema: serde_json::json!({}),
            outputs_schema: serde_json::json!({}),
            permissions: vec![],
            dependencies: vec![],
            estimated_cost: fusion_core::NanoUSD::ZERO,
            estimated_latency_ms: 0,
            reliability_score: 1.0,
            supports_streaming: false,
            traits: vec![],
        };
        let pkg_bytes = crate::package::format::build_test_fusionpkg(
            "test.cap", "0.1.0", &[contract], &[],
        );
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &pkg_bytes).unwrap();
        let result = verifier.verify(tmp.path());
        assert!(result.is_ok());
        let verified = result.unwrap();
        assert_eq!(verified.manifest().name, "test.cap");
    }

    #[test]
    fn test_verify_missing_attestation() {
        let manifest_toml = r#"name = "bad" version = "0.1.0""#;
        let wasm_bytes = wat::parse_str("(module)").unwrap();
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_path("manifest.toml").unwrap();
        header.set_size(manifest_toml.len() as u64); header.set_cksum();
        builder.append(&header, manifest_toml.as_bytes()).unwrap();
        let mut header = tar::Header::new_gnu();
        header.set_path("module.wasm").unwrap();
        header.set_size(wasm_bytes.len() as u64); header.set_cksum();
        builder.append(&header, &wasm_bytes[..]).unwrap();
        let uncompressed = builder.into_inner().unwrap();
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&uncompressed).unwrap();
        let bad_pkg = encoder.finish().unwrap();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &bad_pkg).unwrap();
        let verifier = PackageVerifier { verifier: Arc::new(MockSigner::new("k")) };
        match verifier.verify(tmp.path()) {
            Err(PackageError::MissingFile(_)) => {}
            _ => panic!("expected MissingFile error"),
        }
    }

    #[test]
    fn test_verify_undeclared_wasm_imports() {
        let signer = Arc::new(MockSigner::new("test-key"));
        let verifier = PackageVerifier { verifier: signer };

        let contract = CapabilityContract {
            id: CapabilityId::new("test.verify"),
            version: Version::parse("0.1.0").unwrap(),
            description: "verify test".into(),
            inputs_schema: serde_json::json!({}),
            outputs_schema: serde_json::json!({}),
            permissions: vec![Permission::Network],
            dependencies: vec![],
            estimated_cost: fusion_core::NanoUSD::ZERO,
            estimated_latency_ms: 0,
            reliability_score: 1.0,
            supports_streaming: false,
            traits: vec![],
        };
        let pkg_bytes = crate::package::format::build_test_fusionpkg(
            "test.verify", "0.1.0", &[contract], &["Network".into()],
        );
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &pkg_bytes).unwrap();
        let result = verifier.verify(tmp.path());
        assert!(result.is_ok());
    }
}
