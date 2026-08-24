use crate::release::gate::{GateError, GateId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseEnvironment {
    Production,
    Staging,
    Development,
    #[serde(untagged)]
    Custom(String),
}

impl ReleaseEnvironment {
    pub fn as_str(&self) -> &str {
        match self {
            ReleaseEnvironment::Production => "production",
            ReleaseEnvironment::Staging => "staging",
            ReleaseEnvironment::Development => "development",
            ReleaseEnvironment::Custom(s) => s.as_str(),
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "production" | "prod" => ReleaseEnvironment::Production,
            "staging" | "stage" => ReleaseEnvironment::Staging,
            "development" | "dev" => ReleaseEnvironment::Development,
            custom => ReleaseEnvironment::Custom(custom.to_string()),
        }
    }
}

impl std::str::FromStr for ReleaseEnvironment {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from_str(s))
    }
}

impl fmt::Display for ReleaseEnvironment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct EnvironmentPolicy {
    #[serde(default)]
    pub require: Vec<GateId>,
    #[serde(default)]
    pub advisory: Vec<GateId>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PolicyDefinition {
    pub name: String,
    #[serde(default)]
    pub environments: HashMap<String, EnvironmentPolicy>,
}

impl PolicyDefinition {
    pub fn get_environment_policy(&self, env: &ReleaseEnvironment) -> Option<&EnvironmentPolicy> {
        self.environments.get(env.as_str())
    }

    pub fn default_policy() -> Self {
        let mut environments = HashMap::new();
        environments.insert(
            "production".to_string(),
            EnvironmentPolicy {
                require: vec![
                    GateId::Sdk1,
                    GateId::Replay1,
                    GateId::Upgrade1,
                    GateId::Determinism1,
                    GateId::Plugin1,
                ],
                advisory: vec![GateId::Strategy1, GateId::Provider1, GateId::Connector1],
            },
        );
        environments.insert(
            "staging".to_string(),
            EnvironmentPolicy {
                require: vec![GateId::Sdk1, GateId::Upgrade1, GateId::Plugin1],
                advisory: vec![
                    GateId::Replay1,
                    GateId::Determinism1,
                    GateId::Strategy1,
                    GateId::Provider1,
                    GateId::Connector1,
                ],
            },
        );
        environments.insert(
            "development".to_string(),
            EnvironmentPolicy {
                require: vec![GateId::Sdk1],
                advisory: vec![
                    GateId::Replay1,
                    GateId::Upgrade1,
                    GateId::Determinism1,
                    GateId::Plugin1,
                    GateId::Strategy1,
                    GateId::Provider1,
                    GateId::Connector1,
                ],
            },
        );

        Self {
            name: "standard-default-policy".to_string(),
            environments,
        }
    }
}

#[allow(dead_code)]
pub fn load_policy_from_yaml(path: &Path) -> Result<PolicyDefinition, GateError> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        GateError::ExecutionFailed(format!("read policy file {}: {e}", path.display()))
    })?;
    serde_yaml::from_str(&content).map_err(|e| {
        GateError::ExecutionFailed(format!("parse policy file {}: {e}", path.display()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_release_environment_parsing() {
        assert_eq!(
            ReleaseEnvironment::from_str("production"),
            ReleaseEnvironment::Production
        );
        assert_eq!(
            ReleaseEnvironment::from_str("prod"),
            ReleaseEnvironment::Production
        );
        assert_eq!(
            ReleaseEnvironment::from_str("staging"),
            ReleaseEnvironment::Staging
        );
        assert_eq!(
            ReleaseEnvironment::from_str("development"),
            ReleaseEnvironment::Development
        );
        assert_eq!(
            ReleaseEnvironment::from_str("canary"),
            ReleaseEnvironment::Custom("canary".into())
        );
    }

    #[test]
    fn test_default_policy_structure() {
        let policy = PolicyDefinition::default_policy();
        let prod = policy
            .get_environment_policy(&ReleaseEnvironment::Production)
            .unwrap();
        assert!(prod.require.contains(&GateId::Sdk1));
        assert!(prod.require.contains(&GateId::Plugin1));
        assert!(prod.advisory.contains(&GateId::Provider1));
    }
}
