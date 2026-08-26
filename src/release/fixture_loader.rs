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

    /// Resolve the single manifest file describing an artifact: `path` itself
    /// when it is a file, otherwise the first `*.{ext}` inside it.
    /// A missing file is a hard error — gates must never certify against
    /// nothing (gate integrity).
    pub fn resolve_manifest_file(
        &self,
        path: &Path,
        ext: &str,
        kind_label: &str,
    ) -> Result<PathBuf, GateError> {
        let files = if path.is_dir() {
            self.find_files(path, ext)?
        } else {
            vec![path.to_path_buf()]
        };
        files.into_iter().next().ok_or_else(|| {
            GateError::ExecutionFailed(format!(
                "no {kind_label} manifest (*.{ext}) found under {}",
                path.display()
            ))
        })
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
///
/// Gate integrity: a manifest entry whose version fails to parse or whose
/// outcome string is unrecognized is a hard error listing the offending
/// entry — unknown outcomes must NOT silently default to Pass.
pub fn discover_fixtures(
    manifest: &FixtureManifest,
    kind: FixtureKind,
) -> Result<Vec<FixtureEntry>, GateError> {
    let entries = match kind {
        FixtureKind::Configs => &manifest.configs,
        FixtureKind::Snapshots => &manifest.snapshots,
        FixtureKind::Plugins => &manifest.plugins,
        FixtureKind::Strategies => &manifest.strategies,
        FixtureKind::Providers => &manifest.providers,
        FixtureKind::Connectors => &manifest.connectors,
    };
    let mut results = Vec::new();
    let mut problems: Vec<String> = Vec::new();
    for entry in entries {
        let label = entry
            .id
            .clone()
            .unwrap_or_else(|| format!("path {}", entry.path));
        let version = match semver::Version::parse(&entry.version) {
            Ok(v) => v,
            Err(e) => {
                problems.push(format!(
                    "entry '{label}': invalid version {:?} ({e})",
                    entry.version
                ));
                continue;
            }
        };
        let expected = match entry.expected.as_ref() {
            None => ExpectedOutcome::Pass,
            Some(cfg) => match cfg.outcome.as_str() {
                "pass" => ExpectedOutcome::Pass,
                "warning" => ExpectedOutcome::Warning,
                "fail" => ExpectedOutcome::Fail,
                other => {
                    problems.push(format!(
                        "entry '{label}': unrecognized expected outcome {other:?} (allowed: pass | warning | fail)"
                    ));
                    continue;
                }
            },
        };
        results.push(FixtureEntry {
            id: entry.id.clone(),
            version,
            path: PathBuf::from(&entry.path),
            expected,
        });
    }
    if !problems.is_empty() {
        return Err(GateError::ExecutionFailed(format!(
            "invalid {} manifest entries: {}",
            kind_label(kind),
            problems.join("; ")
        )));
    }
    Ok(results)
}

fn kind_label(kind: FixtureKind) -> &'static str {
    match kind {
        FixtureKind::Configs => "configs",
        FixtureKind::Snapshots => "snapshots",
        FixtureKind::Plugins => "plugins",
        FixtureKind::Strategies => "strategies",
        FixtureKind::Providers => "providers",
        FixtureKind::Connectors => "connectors",
    }
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
        let entries = discover_fixtures(&manifest, FixtureKind::Configs).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].version, semver::Version::new(0, 10, 0));
        assert_eq!(entries[0].expected, ExpectedOutcome::Pass);
        assert_eq!(entries[1].version, semver::Version::new(0, 9, 0));
        assert_eq!(entries[1].expected, ExpectedOutcome::Pass);
    }

    #[test]
    fn test_discover_fixtures_unknown_outcome_is_hard_error() {
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
        let result = discover_fixtures(&manifest, FixtureKind::Configs);
        match result {
            Ok(entries) => {
                panic!(
                    "unknown outcome must not default to Pass, got {} entries",
                    entries.len()
                )
            }
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("unrecognized expected outcome"),
                    "error must name the problem: {msg}"
                );
                assert!(
                    msg.contains("v0.10"),
                    "error must name the offending entry: {msg}"
                );
            }
        }
    }

    #[test]
    fn test_discover_fixtures_bad_semver_is_hard_error() {
        let manifest = FixtureManifest {
            configs: vec![ManifestEntry {
                id: Some("broken".into()),
                version: "not-a-semver".into(),
                path: "configs/broken".into(),
                expected: None,
            }],
            ..Default::default()
        };
        let result = discover_fixtures(&manifest, FixtureKind::Configs);
        match result {
            Ok(entries) => {
                panic!(
                    "invalid semver must fail the load, got {} entries",
                    entries.len()
                )
            }
            Err(e) => {
                let msg = e.to_string();
                assert!(msg.contains("invalid version"), "{msg}");
                assert!(
                    msg.contains("broken"),
                    "must name the offending entry: {msg}"
                );
            }
        }
    }

    #[test]
    fn test_discover_fixtures_collects_all_problems() {
        let manifest = FixtureManifest {
            configs: vec![
                ManifestEntry {
                    id: Some("bad-version".into()),
                    version: "1.0".into(),
                    path: "configs/a".into(),
                    expected: None,
                },
                ManifestEntry {
                    id: Some("bad-outcome".into()),
                    version: "1.0.0".into(),
                    path: "configs/b".into(),
                    expected: Some(ExpectedOutcomeConfig {
                        outcome: "maybe".into(),
                    }),
                },
            ],
            ..Default::default()
        };
        let err = discover_fixtures(&manifest, FixtureKind::Configs).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("bad-version") && msg.contains("bad-outcome"),
            "{msg}"
        );
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
