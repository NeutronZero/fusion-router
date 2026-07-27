use std::path::PathBuf;
use crate::release::gate::{GateCheck, GateError};

#[derive(Debug, Clone)]
pub struct CertificationContext {
    pub fixture_root: PathBuf,
    pub sdk_version: semver::Version,
    pub workspace_root: PathBuf,
}

impl CertificationContext {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            fixture_root: workspace_root.clone(),
            sdk_version: semver::Version::new(0, 10, 0),
            workspace_root,
        }
    }
}

pub trait CertificationArtifact: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &semver::Version;
    fn schema_checks(&self, ctx: &CertificationContext) -> Result<Vec<GateCheck>, GateError>;
    fn contract_checks(&self, ctx: &CertificationContext) -> Result<Vec<GateCheck>, GateError>;
}
