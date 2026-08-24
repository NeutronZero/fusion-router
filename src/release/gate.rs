use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GateId {
    Sdk1,
    Replay1,
    Upgrade1,
    Determinism1,
    Plugin1,
    Strategy1,
    Provider1,
    Connector1,
}

impl GateId {
    pub fn as_str(&self) -> &'static str {
        match self {
            GateId::Sdk1 => "SDK-1",
            GateId::Replay1 => "RPL-1",
            GateId::Upgrade1 => "UPG-1",
            GateId::Determinism1 => "DET-1",
            GateId::Plugin1 => "PLG-1",
            GateId::Strategy1 => "STR-1",
            GateId::Provider1 => "PRV-1",
            GateId::Connector1 => "CON-1",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "SDK-1" => Some(GateId::Sdk1),
            "RPL-1" => Some(GateId::Replay1),
            "UPG-1" => Some(GateId::Upgrade1),
            "DET-1" => Some(GateId::Determinism1),
            "PLG-1" => Some(GateId::Plugin1),
            "STR-1" => Some(GateId::Strategy1),
            "PRV-1" => Some(GateId::Provider1),
            "CON-1" => Some(GateId::Connector1),
            _ => None,
        }
    }
}

impl std::str::FromStr for GateId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_str(s).ok_or_else(|| format!("invalid GateId: {}", s))
    }
}

impl fmt::Display for GateId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl Serialize for GateId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for GateId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        GateId::from_str(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("invalid GateId: {}", s)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GateCategory {
    Compatibility,
    Replay,
    Determinism,
    Upgrade,
    Certification,
}

#[derive(Debug, Clone)]
pub struct GateContext {
    pub workspace_root: PathBuf,
    pub baseline_version: Option<semver::Version>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateCheck {
    pub name: String,
    pub passed: bool,
    pub message: String,
}

pub(crate) mod duration_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(dur: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
        let secs = dur.as_secs_f64();
        secs.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
        let secs = f64::deserialize(deserializer)?;
        Ok(Duration::from_secs_f64(secs))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateResult {
    pub gate_id: GateId,
    pub passed: bool,
    pub summary: String,
    pub details: Vec<GateCheck>,
    #[serde(with = "duration_serde")]
    pub duration: Duration,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum GateError {
    #[error("Gate execution failed: {0}")]
    ExecutionFailed(String),
    #[error("Required tool not available: {0}")]
    ToolNotAvailable(String),
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
}

#[derive(Debug, Clone)]
pub enum GateExecution {
    Success(GateResult),
    ExecutionError(GateError),
}

impl GateExecution {
    pub fn passed(&self) -> bool {
        match self {
            GateExecution::Success(result) => result.passed,
            GateExecution::ExecutionError(_) => false,
        }
    }

    pub fn is_error(&self) -> bool {
        matches!(self, GateExecution::ExecutionError(_))
    }
}

#[derive(Debug, Clone)]
pub struct GateMetadata {
    pub id: GateId,
    pub category: GateCategory,
    pub required: bool,
    pub introduced: semver::Version,
}

#[async_trait]
pub trait ReleaseGate: Send + Sync {
    fn id(&self) -> GateId;
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn metadata(&self) -> &GateMetadata;
    async fn run(&self, context: &GateContext) -> GateExecution;
}

#[cfg(test)]
pub(crate) struct MockGate {
    id: GateId,
    name: String,
    description: String,
    metadata: GateMetadata,
    result: GateExecution,
}

#[cfg(test)]
impl MockGate {
    pub fn new(
        id: GateId,
        name: &str,
        description: &str,
        metadata: GateMetadata,
        result: GateExecution,
    ) -> Self {
        Self {
            id,
            name: name.to_string(),
            description: description.to_string(),
            metadata,
            result,
        }
    }
}

#[cfg(test)]
#[async_trait]
impl ReleaseGate for MockGate {
    fn id(&self) -> GateId {
        self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn metadata(&self) -> &GateMetadata {
        &self.metadata
    }

    async fn run(&self, _context: &GateContext) -> GateExecution {
        self.result.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gate_id_display_and_parse() {
        assert_eq!(GateId::Sdk1.to_string(), "SDK-1");
        assert_eq!(GateId::Replay1.to_string(), "RPL-1");
        assert_eq!(GateId::Upgrade1.to_string(), "UPG-1");
        assert_eq!(GateId::Determinism1.to_string(), "DET-1");

        assert_eq!(GateId::from_str("SDK-1"), Some(GateId::Sdk1));
        assert_eq!(GateId::from_str("RPL-1"), Some(GateId::Replay1));
        assert_eq!(GateId::from_str("UPG-1"), Some(GateId::Upgrade1));
        assert_eq!(GateId::from_str("DET-1"), Some(GateId::Determinism1));
        assert_eq!(GateId::from_str("UNKNOWN"), None);
    }

    #[test]
    fn test_gate_id_serde() {
        for id in [
            GateId::Sdk1,
            GateId::Replay1,
            GateId::Upgrade1,
            GateId::Determinism1,
        ] {
            let json = serde_json::to_string(&id).unwrap();
            let deserialized: GateId = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, id);
        }
    }

    #[test]
    fn test_gate_metadata_has_required_fields() {
        let meta = GateMetadata {
            id: GateId::Sdk1,
            category: GateCategory::Compatibility,
            required: true,
            introduced: semver::Version::new(0, 10, 0),
        };
        assert!(meta.required);
        assert_eq!(meta.category, GateCategory::Compatibility);
        assert_eq!(meta.id, GateId::Sdk1);
    }

    #[test]
    fn test_gate_result_passed_serde() {
        let result = GateResult {
            gate_id: GateId::Sdk1,
            passed: true,
            summary: "All checks passed".into(),
            details: vec![GateCheck {
                name: "SDK compatibility".into(),
                passed: true,
                message: "SDK is compatible".into(),
            }],
            duration: Duration::from_secs(42),
        };

        let json = serde_json::to_string(&result).unwrap();
        let deserialized: GateResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.gate_id, result.gate_id);
        assert_eq!(deserialized.passed, result.passed);
        assert_eq!(deserialized.summary, result.summary);
        assert_eq!(deserialized.details.len(), result.details.len());
        assert_eq!(deserialized.details[0].name, result.details[0].name);
        assert_eq!(deserialized.details[0].passed, result.details[0].passed);
        assert_eq!(deserialized.details[0].message, result.details[0].message);
        assert_eq!(
            deserialized.duration.as_secs_f64(),
            result.duration.as_secs_f64()
        );
    }

    #[test]
    fn test_gate_execution_success() {
        let result = GateResult {
            gate_id: GateId::Sdk1,
            passed: true,
            summary: "OK".into(),
            details: vec![],
            duration: Duration::from_secs(1),
        };
        let execution = GateExecution::Success(result);
        assert!(execution.passed());
        assert!(!execution.is_error());
    }

    #[test]
    fn test_gate_execution_error() {
        let error = GateError::ExecutionFailed("something broke".into());
        let execution = GateExecution::ExecutionError(error);
        assert!(!execution.passed());
        assert!(execution.is_error());
    }

    #[tokio::test]
    async fn test_mock_gate() {
        let meta = GateMetadata {
            id: GateId::Sdk1,
            category: GateCategory::Compatibility,
            required: true,
            introduced: semver::Version::new(0, 10, 0),
        };
        let result = GateResult {
            gate_id: GateId::Sdk1,
            passed: true,
            summary: "OK".into(),
            details: vec![],
            duration: Duration::from_secs(1),
        };
        let gate = MockGate::new(
            GateId::Sdk1,
            "SDK Compat",
            "Checks SDK compatibility",
            meta,
            GateExecution::Success(result.clone()),
        );
        assert_eq!(gate.id(), GateId::Sdk1);
        assert_eq!(gate.name(), "SDK Compat");

        let context = GateContext {
            workspace_root: PathBuf::from("/tmp"),
            baseline_version: None,
        };
        let execution = gate.run(&context).await;
        assert!(execution.passed());
    }
}
