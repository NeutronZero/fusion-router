use std::path::PathBuf;
use std::fs;

use fusion_plugin_api::CapabilityId;

use crate::package::PackageError;
use crate::package::registry::PackageRegistry;

pub struct FilesystemPackageRegistry {
    root: PathBuf,
}

impl FilesystemPackageRegistry {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn package_path(&self, id: &CapabilityId, version: &semver::Version) -> PathBuf {
        self.root.join(format!("{}/{}.fusionpkg", id.as_str(), version))
    }
}

impl PackageRegistry for FilesystemPackageRegistry {
    fn store(&self, id: &CapabilityId, version: &semver::Version, pkg: &[u8]) -> Result<(), PackageError> {
        let dir = self.root.join(id.as_str());
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.fusionpkg", version));
        fs::write(&path, pkg)?;
        Ok(())
    }

    fn load(&self, id: &CapabilityId, version: &semver::Version) -> Result<Vec<u8>, PackageError> {
        let path = self.package_path(id, version);
        Ok(fs::read(&path)?)
    }

    fn list_versions(&self, id: &CapabilityId) -> Result<Vec<semver::Version>, PackageError> {
        let dir = self.root.join(id.as_str());
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut versions: Vec<semver::Version> = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("fusionpkg") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if let Ok(v) = semver::Version::parse(stem) {
                        versions.push(v);
                    }
                }
            }
        }
        versions.sort();
        Ok(versions)
    }

    fn contains(&self, id: &CapabilityId, version: &semver::Version) -> Result<bool, PackageError> {
        Ok(self.package_path(id, version).exists())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let reg = FilesystemPackageRegistry::new(dir.path());
        let id = CapabilityId::new("test.store");
        let version = semver::Version::new(0, 1, 0);
        let pkg_data = b"fake-package-bytes";

        reg.store(&id, &version, pkg_data).unwrap();
        assert!(reg.contains(&id, &version).unwrap());

        let loaded = reg.load(&id, &version).unwrap();
        assert_eq!(loaded, pkg_data);
    }

    #[test]
    fn test_list_versions_sorted() {
        let dir = tempfile::tempdir().unwrap();
        let reg = FilesystemPackageRegistry::new(dir.path());
        let id = CapabilityId::new("test.list");

        reg.store(&id, &semver::Version::new(0, 2, 0), b"v2").unwrap();
        reg.store(&id, &semver::Version::new(0, 1, 0), b"v1").unwrap();
        reg.store(&id, &semver::Version::new(0, 3, 0), b"v3").unwrap();

        let versions = reg.list_versions(&id).unwrap();
        assert_eq!(versions.len(), 3);
        assert_eq!(versions[0].to_string(), "0.1.0");
        assert_eq!(versions[1].to_string(), "0.2.0");
        assert_eq!(versions[2].to_string(), "0.3.0");
    }

    #[test]
    fn test_contains_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let reg = FilesystemPackageRegistry::new(dir.path());
        let id = CapabilityId::new("test.unknown");
        assert!(!reg.contains(&id, &semver::Version::new(9, 9, 9)).unwrap());
    }

    #[test]
    fn test_load_missing_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let reg = FilesystemPackageRegistry::new(dir.path());
        let id = CapabilityId::new("test.missing");
        match reg.load(&id, &semver::Version::new(0, 0, 1)) {
            Err(PackageError::Io(_)) => {}
            _ => panic!("expected Io error for missing package"),
        }
    }
}
