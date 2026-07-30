use fusion_plugin_api::CapabilityId;
use crate::package::PackageError;

pub trait PackageRegistry: Send + Sync {
    fn store(&self, id: &CapabilityId, version: &semver::Version, pkg: &[u8]) -> Result<(), PackageError>;
    fn load(&self, id: &CapabilityId, version: &semver::Version) -> Result<Vec<u8>, PackageError>;
    fn list_versions(&self, id: &CapabilityId) -> Result<Vec<semver::Version>, PackageError>;
    fn contains(&self, id: &CapabilityId, version: &semver::Version) -> Result<bool, PackageError>;
}
