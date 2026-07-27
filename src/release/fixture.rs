use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct FixtureManifest {
    #[serde(default)]
    pub configs: Vec<ManifestEntry>,
    #[serde(default)]
    pub snapshots: Vec<ManifestEntry>,
    #[serde(default)]
    pub plugins: Vec<ManifestEntry>,
    #[serde(default)]
    pub strategies: Vec<ManifestEntry>,
    #[serde(default)]
    pub providers: Vec<ManifestEntry>,
    #[serde(default)]
    pub connectors: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ManifestEntry {
    #[serde(default)]
    pub id: Option<String>,
    pub version: String,
    pub path: String,
    #[serde(default)]
    pub expected: Option<ExpectedOutcomeConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExpectedOutcomeConfig {
    #[serde(default)]
    pub outcome: String,
}

#[derive(Debug, Clone)]
pub struct FixtureEntry {
    pub id: Option<String>,
    pub version: semver::Version,
    pub path: PathBuf,
    pub expected: ExpectedOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpectedOutcome {
    Pass,
    Warning,
    Fail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureKind {
    Configs,
    Snapshots,
    Plugins,
    Strategies,
    Providers,
    Connectors,
}
