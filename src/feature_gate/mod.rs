pub mod config_subscriber;

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FeatureFlag {
    Streaming,
    Replay,
    ConnectorHealth,
    SemanticCache,
    WasmPlugins,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stability {
    Experimental,
    Stable,
    Deprecated,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeatureDefinition {
    pub id: FeatureFlag,
    pub introduced: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed: Option<&'static str>,
    pub stability: Stability,
    pub default_enabled: bool,
    pub description: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct FeatureState {
    pub id: FeatureFlag,
    pub enabled: bool,
    pub overridden: bool,
    pub definition: &'static FeatureDefinition,
}

pub struct FeatureRegistry {
    definitions: &'static [FeatureDefinition],
    states: HashMap<FeatureFlag, bool>,
    overridden: HashSet<FeatureFlag>,
    lookup_map: HashMap<String, FeatureFlag>,
}

impl FeatureRegistry {
    pub fn new(definitions: &'static [FeatureDefinition]) -> Self {
        let states: HashMap<FeatureFlag, bool> = definitions
            .iter()
            .map(|d| (d.id, d.default_enabled))
            .collect();

        let lookup_map: HashMap<String, FeatureFlag> = definitions
            .iter()
            .map(|d| {
                let key = serde_json::to_value(d.id)
                    .ok()
                    .and_then(|v| v.as_str().map(String::from))
                    .unwrap_or_default();
                (key, d.id)
            })
            .collect();

        Self {
            definitions,
            states,
            overridden: HashSet::new(),
            lookup_map,
        }
    }

    pub fn apply_config(&mut self, config: &HashMap<String, FeatureConfig>) {
        for (key, cfg) in config {
            if let Some(flag) = self.lookup_map.get(key) {
                self.states.insert(*flag, cfg.enabled);
                self.overridden.insert(*flag);
            }
        }
    }

    pub fn is_enabled(&self, flag: FeatureFlag) -> bool {
        self.states.get(&flag).copied().unwrap_or(false)
    }

    pub fn is_effectively_enabled(&self, flag: FeatureFlag) -> bool {
        if !self.compile_time_enabled(flag) {
            return false;
        }
        self.is_enabled(flag)
    }

    fn compile_time_enabled(&self, flag: FeatureFlag) -> bool {
        match flag {
            FeatureFlag::SemanticCache => cfg!(feature = "semantic-cache"),
            FeatureFlag::WasmPlugins => cfg!(feature = "wasm-plugins"),
            _ => true,
        }
    }

    pub fn list(&self) -> Vec<FeatureState> {
        self.definitions
            .iter()
            .map(|def| {
                let enabled = self.states.get(&def.id).copied().unwrap_or(false);
                let overridden = self.overridden.contains(&def.id);
                FeatureState {
                    id: def.id,
                    enabled,
                    overridden,
                    definition: def,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_DEFINITIONS: &[FeatureDefinition] = &[
        FeatureDefinition {
            id: FeatureFlag::Streaming,
            introduced: "0.1.0",
            removed: None,
            stability: Stability::Stable,
            default_enabled: true,
            description: "Enable streaming responses",
        },
        FeatureDefinition {
            id: FeatureFlag::Replay,
            introduced: "0.5.0",
            removed: None,
            stability: Stability::Experimental,
            default_enabled: false,
            description: "Enable request replay",
        },
        FeatureDefinition {
            id: FeatureFlag::ConnectorHealth,
            introduced: "0.8.0",
            removed: None,
            stability: Stability::Stable,
            default_enabled: true,
            description: "Enable connector health checks",
        },
        FeatureDefinition {
            id: FeatureFlag::SemanticCache,
            introduced: "0.9.0",
            removed: None,
            stability: Stability::Experimental,
            default_enabled: false,
            description: "Enable semantic caching",
        },
        FeatureDefinition {
            id: FeatureFlag::WasmPlugins,
            introduced: "0.10.0",
            removed: None,
            stability: Stability::Deprecated,
            default_enabled: false,
            description: "Enable WASM plugin support",
        },
    ];

    #[test]
    fn test_feature_flag_serde_round_trip() {
        let flag = FeatureFlag::Streaming;
        let json = serde_json::to_string(&flag).unwrap();
        assert_eq!(json, "\"streaming\"");
        let deserialized: FeatureFlag = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, flag);

        let flag = FeatureFlag::ConnectorHealth;
        let json = serde_json::to_string(&flag).unwrap();
        assert_eq!(json, "\"connector-health\"");
        let deserialized: FeatureFlag = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, flag);

        let flag = FeatureFlag::WasmPlugins;
        let json = serde_json::to_string(&flag).unwrap();
        assert_eq!(json, "\"wasm-plugins\"");
        let deserialized: FeatureFlag = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, flag);
    }

    #[test]
    fn test_feature_registry_defaults() {
        let registry = FeatureRegistry::new(TEST_DEFINITIONS);
        assert!(registry.is_enabled(FeatureFlag::Streaming));
        assert!(!registry.is_enabled(FeatureFlag::Replay));
        assert!(registry.is_enabled(FeatureFlag::ConnectorHealth));
        assert!(!registry.is_enabled(FeatureFlag::SemanticCache));
        assert!(!registry.is_enabled(FeatureFlag::WasmPlugins));
    }

    #[test]
    fn test_apply_config_disables_feature() {
        let mut registry = FeatureRegistry::new(TEST_DEFINITIONS);
        assert!(registry.is_enabled(FeatureFlag::Streaming));

        let mut config: HashMap<String, FeatureConfig> = HashMap::new();
        config.insert("streaming".to_string(), FeatureConfig { enabled: false });
        registry.apply_config(&config);

        assert!(!registry.is_enabled(FeatureFlag::Streaming));
    }

    #[test]
    fn test_apply_config_unknown_feature_is_ignored() {
        let mut registry = FeatureRegistry::new(TEST_DEFINITIONS);
        let mut config: HashMap<String, FeatureConfig> = HashMap::new();
        config.insert(
            "unknown-feature".to_string(),
            FeatureConfig { enabled: true },
        );
        registry.apply_config(&config);
    }

    #[test]
    fn test_list_returns_all_features_with_state() {
        let registry = FeatureRegistry::new(TEST_DEFINITIONS);
        let list = registry.list();
        assert_eq!(list.len(), 5);

        let streaming = list
            .iter()
            .find(|s| s.id == FeatureFlag::Streaming)
            .unwrap();
        assert!(streaming.enabled);
        assert!(!streaming.overridden);
        assert_eq!(
            streaming.definition.description,
            "Enable streaming responses"
        );

        let replay = list.iter().find(|s| s.id == FeatureFlag::Replay).unwrap();
        assert!(!replay.enabled);
        assert!(!replay.overridden);
    }

    #[test]
    fn test_lookup_from_definition_works() {
        let mut registry = FeatureRegistry::new(TEST_DEFINITIONS);

        let mut config: HashMap<String, FeatureConfig> = HashMap::new();
        config.insert(
            "connector-health".to_string(),
            FeatureConfig { enabled: false },
        );
        registry.apply_config(&config);

        assert!(!registry.is_enabled(FeatureFlag::ConnectorHealth));

        let list = registry.list();
        let ch = list
            .iter()
            .find(|s| s.id == FeatureFlag::ConnectorHealth)
            .unwrap();
        assert!(ch.overridden);
    }

    #[test]
    fn test_is_effectively_enabled_delegates() {
        let mut registry = FeatureRegistry::new(TEST_DEFINITIONS);
        assert!(registry.is_effectively_enabled(FeatureFlag::Streaming));

        let mut config: HashMap<String, FeatureConfig> = HashMap::new();
        config.insert("streaming".to_string(), FeatureConfig { enabled: false });
        registry.apply_config(&config);
        assert!(!registry.is_effectively_enabled(FeatureFlag::Streaming));
    }

    #[test]
    fn test_apply_config_enables_feature() {
        let mut registry = FeatureRegistry::new(TEST_DEFINITIONS);
        assert!(!registry.is_enabled(FeatureFlag::Replay));

        let mut config: HashMap<String, FeatureConfig> = HashMap::new();
        config.insert("replay".to_string(), FeatureConfig { enabled: true });
        registry.apply_config(&config);

        assert!(registry.is_enabled(FeatureFlag::Replay));
    }
}
