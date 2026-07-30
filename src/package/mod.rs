pub mod format;
pub mod verifier;
pub mod loader;
pub mod registry;
pub mod filesystem_registry;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PackageError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid archive: {0}")]
    InvalidArchive(String),

    #[error("Manifest parse error: {0}")]
    ManifestParse(String),

    #[error("Missing required file: {0}")]
    MissingFile(String),

    #[error("Attestation verification failed: {0}")]
    AttestationFailed(String),

    #[error("WASM compilation failed: {0}")]
    WasmCompilationFailed(String),

    #[error("Permission inconsistency: {0}")]
    PermissionMismatch(String),

    #[error("Registry error: {0}")]
    Registry(String),

    #[error("Cache error: {0}")]
    Cache(String),
}

impl From<crate::release::gate::GateError> for PackageError {
    fn from(e: crate::release::gate::GateError) -> Self {
        PackageError::AttestationFailed(e.to_string())
    }
}

impl From<crate::runtime::RuntimeError> for PackageError {
    fn from(e: crate::runtime::RuntimeError) -> Self {
        PackageError::WasmCompilationFailed(e.to_string())
    }
}

pub use format::{extract_package, parse_manifest, CapabilityDependency, Manifest, PackageArchive};
pub use verifier::{PackageVerifier, VerifiedPackage};
pub use loader::PackageLoader;
pub use registry::PackageRegistry;
pub use filesystem_registry::FilesystemPackageRegistry;

#[cfg(feature = "wasm-plugins")]
pub use crate::runtime::RuntimeModuleCache;
