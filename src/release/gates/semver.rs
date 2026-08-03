use async_trait::async_trait;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::release::gate::{
    GateCategory, GateCheck, GateContext, GateError, GateExecution, GateId, GateMetadata,
    GateResult, ReleaseGate,
};

#[async_trait]
pub trait SemVerBackend: Send + Sync {
    fn name(&self) -> &str;
    async fn check_release(
        &self,
        crate_path: &Path,
        baseline_ref: &str,
    ) -> Result<Vec<GateCheck>, GateError>;
}

pub struct CargoSemVerChecksBackend;

#[async_trait]
impl SemVerBackend for CargoSemVerChecksBackend {
    fn name(&self) -> &str {
        "cargo-semver-checks"
    }

    async fn check_release(
        &self,
        crate_path: &Path,
        baseline_ref: &str,
    ) -> Result<Vec<GateCheck>, GateError> {
        let output = tokio::process::Command::new("cargo")
            .arg("semver-checks")
            .arg("check-release")
            .arg("--manifest-path")
            .arg(crate_path.join("Cargo.toml"))
            .arg("--baseline-ref")
            .arg(baseline_ref)
            .arg("--format")
            .arg("json")
            .output()
            .await
            .map_err(|e| {
                GateError::ToolNotAvailable(format!(
                    "cargo semver-checks not found: {}",
                    e
                ))
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();

        if !output.status.success() {
            if !stdout.trim().is_empty() {
                return parse_semver_checks_output(&stdout);
            }
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GateError::ExecutionFailed(format!(
                "cargo semver-checks failed: {}",
                stderr
            )));
        }

        parse_semver_checks_output(&stdout)
    }
}

#[derive(Debug, Deserialize)]
struct SemVerCheckEntry {
    #[serde(default)]
    name: String,
    message: String,
    severity: String,
}

pub fn parse_semver_checks_output(output: &str) -> Result<Vec<GateCheck>, GateError> {
    let entries: Vec<SemVerCheckEntry> = serde_json::from_str(output).map_err(|e| {
        GateError::ExecutionFailed(format!("Failed to parse semver-checks output: {}", e))
    })?;

    Ok(entries
        .into_iter()
        .map(|entry| {
            let passed = matches!(entry.severity.as_str(), "pass" | "info" | "warn");
            let name = if entry.name.is_empty() {
                entry.message.clone()
            } else {
                entry.name
            };
            GateCheck {
                name,
                passed,
                message: entry.message,
            }
        })
        .collect())
}

pub struct SemVerGate {
    backend: Box<dyn SemVerBackend>,
    baseline_ref: String,
    crate_path: PathBuf,
    metadata: GateMetadata,
}

impl SemVerGate {
    pub fn new(baseline_ref: &str, crate_path: PathBuf) -> Self {
        Self {
            backend: Box::new(CargoSemVerChecksBackend),
            baseline_ref: baseline_ref.to_string(),
            crate_path,
            metadata: GateMetadata {
                id: GateId::Sdk1,
                category: GateCategory::Compatibility,
                required: true,
                introduced: semver::Version::new(0, 10, 0),
            },
        }
    }

    pub fn with_backend(
        backend: Box<dyn SemVerBackend>,
        baseline_ref: &str,
        crate_path: PathBuf,
    ) -> Self {
        Self {
            backend,
            baseline_ref: baseline_ref.to_string(),
            crate_path,
            metadata: GateMetadata {
                id: GateId::Sdk1,
                category: GateCategory::Compatibility,
                required: true,
                introduced: semver::Version::new(0, 10, 0),
            },
        }
    }
}

#[async_trait]
impl ReleaseGate for SemVerGate {
    fn id(&self) -> GateId {
        GateId::Sdk1
    }

    fn name(&self) -> &str {
        "SemVer Compatibility Gate"
    }

    fn description(&self) -> &str {
        "Checks SemVer compatibility via cargo semver-checks"
    }

    fn metadata(&self) -> &GateMetadata {
        &self.metadata
    }

    async fn run(&self, _context: &GateContext) -> GateExecution {
        let start = Instant::now();
        match self
            .backend
            .check_release(&self.crate_path, &self.baseline_ref)
            .await
        {
            Ok(checks) => {
                let passed = checks.iter().all(|c| c.passed);
                let failed_count = checks.iter().filter(|c| !c.passed).count();
                let summary = if passed {
                    format!("All {} checks passed", checks.len())
                } else {
                    format!("{} check(s) failed", failed_count)
                };
                GateExecution::Success(GateResult {
                    gate_id: GateId::Sdk1,
                    passed,
                    summary,
                    details: checks,
                    duration: start.elapsed(),
                })
            }
            Err(e) => GateExecution::ExecutionError(e),
        }
    }
}

#[cfg(test)]
pub struct MockBackend {
    pub should_pass: bool,
}

#[cfg(test)]
#[async_trait]
impl SemVerBackend for MockBackend {
        fn name(&self) -> &str {
            "mock"
        }

        async fn check_release(
            &self,
            _crate_path: &Path,
            _baseline_ref: &str,
        ) -> Result<Vec<GateCheck>, GateError> {
            if self.should_pass {
                Ok(vec![GateCheck {
                    name: "compatibility".into(),
                    passed: true,
                    message: "All compatible".into(),
                }])
            } else {
                Ok(vec![GateCheck {
                    name: "compatibility".into(),
                    passed: false,
                    message: "Breaking change detected".into(),
                }])
            }
        }
    }

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_semver_gate_metadata() {
        let gate = SemVerGate::new("v0.9.0", PathBuf::from("/tmp"));
        let meta = gate.metadata();
        assert_eq!(meta.id, GateId::Sdk1);
        assert_eq!(meta.category, GateCategory::Compatibility);
        assert!(meta.required);
    }

    #[tokio::test]
    async fn test_mock_backend_returns_pass() {
        let gate = SemVerGate::with_backend(
            Box::new(MockBackend { should_pass: true }),
            "v0.9.0",
            PathBuf::from("/tmp"),
        );
        let context = GateContext {
            workspace_root: PathBuf::from("/tmp"),
            baseline_version: None,
        };
        let execution = gate.run(&context).await;
        assert!(execution.passed());
        match &execution {
            GateExecution::Success(result) => {
                assert!(result.passed);
                assert_eq!(result.details.len(), 1);
                assert!(result.details[0].passed);
            }
            _ => panic!("Expected Success"),
        }
    }

    #[tokio::test]
    async fn test_mock_backend_returns_fail() {
        let gate = SemVerGate::with_backend(
            Box::new(MockBackend { should_pass: false }),
            "v0.9.0",
            PathBuf::from("/tmp"),
        );
        let context = GateContext {
            workspace_root: PathBuf::from("/tmp"),
            baseline_version: None,
        };
        let execution = gate.run(&context).await;
        assert!(!execution.passed());
        match &execution {
            GateExecution::Success(result) => {
                assert!(!result.passed);
                assert_eq!(result.details.len(), 1);
                assert!(!result.details[0].passed);
            }
            _ => panic!("Expected Success"),
        }
    }

    #[test]
    fn test_parse_semver_output_all_pass() {
        let json = r#"[
            {"severity": "pass", "message": "All items compatible"}
        ]"#;
        let checks = parse_semver_checks_output(json).unwrap();
        assert_eq!(checks.len(), 1);
        assert!(checks[0].passed);
    }

    #[test]
    fn test_parse_semver_output_with_errors() {
        let json = r#"[
            {"severity": "pass", "message": "Public API unchanged"},
            {"severity": "error", "message": "Function `foo` was removed"}
        ]"#;
        let checks = parse_semver_checks_output(json).unwrap();
        assert_eq!(checks.len(), 2);
        assert!(checks[0].passed);
        assert!(!checks[1].passed);
    }

    #[test]
    fn test_parse_semver_output_invalid_json() {
        let result = parse_semver_checks_output("not json");
        assert!(result.is_err());
    }
}
