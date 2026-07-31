use std::collections::HashMap;
use std::fmt;
use serde::{Deserialize, Serialize};
use fusion_plugin_api::{CapabilityContract, CapabilityId};

/// Errors that can occur during registry operations.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RegistryError {
    DuplicateId(CapabilityId),
    Frozen,
    NotFound(CapabilityId),
    InvalidContract(String),
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegistryError::DuplicateId(id) => write!(f, "Duplicate capability ID: {id}"),
            RegistryError::Frozen => write!(f, "Registry is frozen"),
            RegistryError::NotFound(id) => write!(f, "Capability not found: {id}"),
            RegistryError::InvalidContract(msg) => write!(f, "Invalid contract: {msg}"),
        }
    }
}

impl std::error::Error for RegistryError {}

/// Source of a registered capability.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CapabilitySource {
    Builtin,
    Package,
    Development,
    Remote,
}

impl fmt::Display for CapabilitySource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CapabilitySource::Builtin => write!(f, "builtin"),
            CapabilitySource::Package => write!(f, "package"),
            CapabilitySource::Development => write!(f, "development"),
            CapabilitySource::Remote => write!(f, "remote"),
        }
    }
}

/// Discovery metadata wrapping a `CapabilityContract`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityDescriptor {
    pub contract: CapabilityContract,
    pub tags: Vec<String>,
    pub categories: Vec<String>,
    pub discoverable: bool,
    pub provider: Option<String>,
    pub source: CapabilitySource,
}

/// The capability registry trait — answers only "what capabilities are available?"
pub trait CapabilityRegistry: Send + Sync {
    fn register(&mut self, contract: CapabilityContract) -> Result<(), RegistryError>;
    fn get(&self, id: &CapabilityId) -> Option<&CapabilityContract>;
    fn contains(&self, id: &CapabilityId) -> bool;
    fn list(&self) -> Vec<&CapabilityContract>;
    fn freeze(&mut self);
    fn is_frozen(&self) -> bool;
}

/// In-memory implementation of `CapabilityRegistry`.
#[derive(Debug, Clone)]
pub struct InMemoryCapabilityRegistry {
    contracts: HashMap<CapabilityId, CapabilityContract>,
    frozen: bool,
}

impl InMemoryCapabilityRegistry {
    pub fn new() -> Self {
        Self {
            contracts: HashMap::new(),
            frozen: false,
        }
    }
}

impl Default for InMemoryCapabilityRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl CapabilityRegistry for InMemoryCapabilityRegistry {
    fn register(&mut self, contract: CapabilityContract) -> Result<(), RegistryError> {
        if self.frozen {
            return Err(RegistryError::Frozen);
        }
        if self.contracts.contains_key(&contract.id) {
            return Err(RegistryError::DuplicateId(contract.id.clone()));
        }
        for perm in &contract.permissions {
            perm.validate().map_err(|e| {
                RegistryError::InvalidContract(format!("invalid permission: {e}"))
            })?;
        }
        self.contracts.insert(contract.id.clone(), contract);
        Ok(())
    }

    fn get(&self, id: &CapabilityId) -> Option<&CapabilityContract> {
        self.contracts.get(id)
    }

    fn contains(&self, id: &CapabilityId) -> bool {
        self.contracts.contains_key(id)
    }

    fn list(&self) -> Vec<&CapabilityContract> {
        let mut result: Vec<&CapabilityContract> = self.contracts.values().collect();
        result.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
        result
    }

    fn freeze(&mut self) {
        self.frozen = true;
    }

    fn is_frozen(&self) -> bool {
        self.frozen
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fusion_plugin_api::Permission;

    #[test]
    fn register_and_get() {
        let mut reg = InMemoryCapabilityRegistry::new();
        let contract = CapabilityContract {
            id: CapabilityId::new("test.trait"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "trait test".into(),
            inputs_schema: serde_json::json!({}),
            outputs_schema: serde_json::json!({}),
            permissions: vec![],
            dependencies: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 0,
            reliability_score: 1.0,
            supports_streaming: false,
            traits: vec![],
        };
        reg.register(contract.clone()).unwrap();
        assert!(reg.contains(&CapabilityId::new("test.trait")));
        assert_eq!(
            reg.get(&CapabilityId::new("test.trait"))
                .map(|c| c.id.as_str()),
            Some("test.trait")
        );
    }

    #[test]
    fn freeze_blocks_registration() {
        let mut reg = InMemoryCapabilityRegistry::new();
        let contract = CapabilityContract {
            id: CapabilityId::new("test.freeze"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "freeze test".into(),
            inputs_schema: serde_json::json!({}),
            outputs_schema: serde_json::json!({}),
            permissions: vec![],
            dependencies: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 0,
            reliability_score: 1.0,
            supports_streaming: false,
            traits: vec![],
        };
        reg.register(contract).unwrap();
        reg.freeze();
        assert!(reg.is_frozen());
        let dup = CapabilityContract {
            id: CapabilityId::new("test.after_freeze"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "should fail".into(),
            inputs_schema: serde_json::json!({}),
            outputs_schema: serde_json::json!({}),
            permissions: vec![],
            dependencies: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 0,
            reliability_score: 1.0,
            supports_streaming: false,
            traits: vec![],
        };
        match reg.register(dup) {
            Err(RegistryError::Frozen) => {}
            _ => panic!("expected Frozen error"),
        }
    }

    #[test]
    fn duplicate_id_rejected() {
        let mut reg = InMemoryCapabilityRegistry::new();
        let contract = CapabilityContract {
            id: CapabilityId::new("test.dup"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "original".into(),
            inputs_schema: serde_json::json!({}),
            outputs_schema: serde_json::json!({}),
            permissions: vec![],
            dependencies: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 0,
            reliability_score: 1.0,
            supports_streaming: false,
            traits: vec![],
        };
        reg.register(contract.clone()).unwrap();
        match reg.register(contract) {
            Err(RegistryError::DuplicateId(_)) => {}
            _ => panic!("expected DuplicateId error"),
        }
    }

    #[test]
    fn list_sorted_by_id() {
        let mut reg = InMemoryCapabilityRegistry::new();
        let c1 = CapabilityContract {
            id: CapabilityId::new("z.last"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: String::new(),
            inputs_schema: serde_json::json!({}),
            outputs_schema: serde_json::json!({}),
            permissions: vec![],
            dependencies: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 0,
            reliability_score: 1.0,
            supports_streaming: false,
            traits: vec![],
        };
        let c2 = CapabilityContract {
            id: CapabilityId::new("a.first"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: String::new(),
            inputs_schema: serde_json::json!({}),
            outputs_schema: serde_json::json!({}),
            permissions: vec![],
            dependencies: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 0,
            reliability_score: 1.0,
            supports_streaming: false,
            traits: vec![],
        };
        reg.register(c1).unwrap();
        reg.register(c2).unwrap();
        let list = reg.list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id.as_str(), "a.first");
        assert_eq!(list[1].id.as_str(), "z.last");
    }

    #[test]
    fn rejects_invalid_permissions() {
        let mut reg = InMemoryCapabilityRegistry::new();
        let contract = CapabilityContract {
            id: CapabilityId::new("test.invalid"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: String::new(),
            inputs_schema: serde_json::json!({}),
            outputs_schema: serde_json::json!({}),
            permissions: vec![Permission::Filesystem("".into())],
            dependencies: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 0,
            reliability_score: 1.0,
            supports_streaming: false,
            traits: vec![],
        };
        match reg.register(contract) {
            Err(RegistryError::InvalidContract(_)) => {}
            _ => panic!("expected InvalidContract error"),
        }
    }

    #[test]
    fn registry_error_display() {
        let err = RegistryError::Frozen;
        let msg = err.to_string();
        assert!(!msg.is_empty());
    }
}
