use crate::release::fixture::*;
use crate::release::gate::GateError;
use std::path::{Path, PathBuf};

/// Low-level fixture I/O shared by all backends and test helpers.
/// Backends use this for manifest loading + file traversal, then construct their own domain types.
pub struct FixtureLoader {
    pub fixture_root: PathBuf,
}

impl FixtureLoader {
    pub fn new(fixture_root: PathBuf) -> Self {
        Self { fixture_root }
    }

    /// Resolve a path relative to the fixture root.
    pub fn resolve(&self, rel: &Path) -> PathBuf {
        self.fixture_root.join(rel)
    }

    /// Read a file to string, wrapping errors as GateError.
    pub fn read_to_string(&self, path: &Path) -> Result<String, GateError> {
        std::fs::read_to_string(path)
            .map_err(|e| GateError::ExecutionFailed(format!("read {}: {e}", path.display())))
    }

    /// Find files with a given extension in a directory (non-recursive).
    pub fn find_files(&self, dir: &Path, ext: &str) -> Result<Vec<PathBuf>, GateError> {
        let mut results = Vec::new();
        if dir.is_dir() {
            for entry in std::fs::read_dir(dir).map_err(|e| {
                GateError::ExecutionFailed(format!("read dir {}: {e}", dir.display()))
            })? {
                let entry = entry.map_err(|e| GateError::ExecutionFailed(e.to_string()))?;
                if entry.path().extension().is_some_and(|e| e == ext) {
                    results.push(entry.path());
                }
            }
        }
        results.sort();
        Ok(results)
    }
}

/// Parse and load a fixture manifest from the standard location.
pub fn load_fixture_manifest(loader: &FixtureLoader) -> Result<FixtureManifest, GateError> {
    let path = loader.resolve(Path::new("tests/fixtures/manifest.yaml"));
    let content = loader.read_to_string(&path)?;
    serde_yaml::from_str(&content)
        .map_err(|e| GateError::ExecutionFailed(format!("parse manifest: {e}")))
}

/// Discover fixture entries preserving **manifest declaration order**.
/// Only sorts when no manifest is given and directory scanning is used (future).
pub fn discover_fixtures(manifest: &FixtureManifest, kind: FixtureKind) -> Vec<FixtureEntry> {
    let entries = match kind {
        FixtureKind::Configs => &manifest.configs,
        FixtureKind::Snapshots => &manifest.snapshots,
        FixtureKind::Plugins => &manifest.plugins,
        FixtureKind::Strategies => &manifest.strategies,
        FixtureKind::Providers => &manifest.providers,
        FixtureKind::Connectors => &manifest.connectors,
    };
    entries
        .iter()
        .filter_map(|entry| {
            let version = semver::Version::parse(&entry.version).ok()?;
            let expected = entry
                .expected
                .as_ref()
                .and_then(|e| match e.outcome.as_str() {
                    "pass" => Some(ExpectedOutcome::Pass),
                    "warning" => Some(ExpectedOutcome::Warning),
                    "fail" => Some(ExpectedOutcome::Fail),
                    _ => None,
                })
                .unwrap_or(ExpectedOutcome::Pass);
            Some(FixtureEntry {
                id: entry.id.clone(),
                version,
                path: PathBuf::from(&entry.path),
                expected,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_manifest_success() {
        let dir =
            std::env::temp_dir().join(format!("fusion_m2_fixture_loader_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(dir.join("tests/fixtures"));
        let yaml = r#"
configs:
  - version: "0.9.0"
    path: configs/v0.9
    expected:
      outcome: pass
"#;
        std::fs::write(dir.join("tests/fixtures/manifest.yaml"), yaml).unwrap();
        let loader = FixtureLoader::new(dir.clone());
        let manifest = load_fixture_manifest(&loader).unwrap();
        assert_eq!(manifest.configs.len(), 1);
        assert_eq!(manifest.snapshots.len(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_manifest_missing_file() {
        let loader = FixtureLoader::new(PathBuf::from("/nonexistent"));
        let result = load_fixture_manifest(&loader);
        assert!(result.is_err());
    }

    #[test]
    fn test_discover_fixtures_preserves_manifest_order() {
        let manifest = FixtureManifest {
            configs: vec![
                ManifestEntry {
                    id: Some("v0.10".into()),
                    version: "0.10.0".into(),
                    path: "configs/v0.10".into(),
                    expected: Some(ExpectedOutcomeConfig {
                        outcome: "pass".into(),
                    }),
                },
                ManifestEntry {
                    id: Some("v0.9".into()),
                    version: "0.9.0".into(),
                    path: "configs/v0.9".into(),
                    expected: None,
                },
            ],
            ..Default::default()
        };
        let entries = discover_fixtures(&manifest, FixtureKind::Configs);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].version, semver::Version::new(0, 10, 0));
        assert_eq!(entries[0].expected, ExpectedOutcome::Pass);
        assert_eq!(entries[1].version, semver::Version::new(0, 9, 0));
        assert_eq!(entries[1].expected, ExpectedOutcome::Pass);
    }

    #[test]
    fn test_discover_fixtures_unknown_outcome_defaults_to_pass() {
        let manifest = FixtureManifest {
            configs: vec![ManifestEntry {
                id: Some("v0.10".into()),
                version: "0.10.0".into(),
                path: "configs/v0.10".into(),
                expected: Some(ExpectedOutcomeConfig {
                    outcome: "unknown".into(),
                }),
            }],
            ..Default::default()
        };
        let entries = discover_fixtures(&manifest, FixtureKind::Configs);
        assert_eq!(entries[0].expected, ExpectedOutcome::Pass);
    }

    #[test]
    fn test_fixture_loader_find_files() {
        let dir =
            std::env::temp_dir().join(format!("fusion_m2_find_files_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("a.yaml"), "a").unwrap();
        std::fs::write(dir.join("b.yml"), "b").unwrap();
        std::fs::write(dir.join("c.txt"), "c").unwrap();
        let loader = FixtureLoader::new(PathBuf::from("."));
        let yaml_files = loader.find_files(&dir, "yaml").unwrap();
        assert!(yaml_files.iter().any(|p| p.ends_with("a.yaml")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
