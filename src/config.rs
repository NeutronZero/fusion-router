use std::collections::HashMap;
use serde::Deserialize;

use crate::types::{Policy, PolicyAction, PolicyCondition, Quota, ProviderLimit};

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub resources: ResourceConfig,
    #[serde(default)]
    pub policies: Vec<PolicyConfig>,
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,
    #[serde(default)]
    pub strategies: StrategyConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

fn default_host() -> String { "0.0.0.0".to_string() }
fn default_port() -> u16 { 8080 }

#[derive(Debug, Clone, Deserialize)]
pub struct ResourceConfig {
    pub max_daily_cost: f64,
    pub max_daily_tokens: u64,
    #[serde(default = "default_concurrent")]
    pub max_concurrent: u32,
    #[serde(default)]
    pub provider_limits: HashMap<String, ProviderLimitConfig>,
}

fn default_concurrent() -> u32 { 5 }

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderLimitConfig {
    pub max_daily_cost: f64,
    pub max_rpm: u32,
    pub max_tpm: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PolicyConfig {
    pub name: String,
    #[serde(default)]
    pub priority: u32,
    #[serde(default)]
    pub conditions: Vec<PolicyConditionConfig>,
    #[serde(default)]
    pub actions: Vec<PolicyActionConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PolicyConditionConfig {
    pub field: String,
    pub operator: String,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PolicyActionConfig {
    pub action_type: String,
    #[serde(default)]
    pub params: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderConfig {
    pub base_url: Option<String>,
    pub api_key_env: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StrategyConfig {
    #[serde(default = "default_consensus_count")]
    pub consensus_count: u32,
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self { consensus_count: default_consensus_count() }
    }
}

fn default_consensus_count() -> u32 { 3 }

impl AppConfig {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: AppConfig = serde_yaml::from_str(&content)?;
        Ok(config)
    }

    pub fn to_quota(&self) -> Quota {
        Quota {
            max_daily_cost: self.resources.max_daily_cost,
            max_daily_tokens: self.resources.max_daily_tokens,
            max_concurrent: self.resources.max_concurrent,
            provider_limits: self.resources.provider_limits.iter().map(|(k, v)| {
                (k.clone(), ProviderLimit {
                    max_daily_cost: v.max_daily_cost,
                    max_rpm: v.max_rpm,
                    max_tpm: v.max_tpm,
                })
            }).collect(),
        }
    }

    pub fn to_policies(&self) -> Vec<Policy> {
        self.policies.iter().map(|p| Policy {
            name: p.name.clone(),
            priority: p.priority,
            conditions: p.conditions.iter().map(|c| PolicyCondition {
                field: c.field.clone(),
                operator: c.operator.clone(),
                value: c.value.clone(),
            }).collect(),
            actions: p.actions.iter().map(|a| PolicyAction {
                action_type: a.action_type.clone(),
                params: a.params.clone(),
            }).collect(),
        }).collect()
    }
}
