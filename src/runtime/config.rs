use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SandboxConfig {
    pub memory_limit_bytes: u64,
    pub fuel_amount: u64,
    pub timeout_ms: Option<u64>,
    pub max_response_bytes: usize,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            memory_limit_bytes: 64 * 1024 * 1024,
            fuel_amount: 1_000_000,
            timeout_ms: None,
            max_response_bytes: 64 * 1024 * 1024,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values() {
        let config = SandboxConfig::default();
        assert_eq!(config.memory_limit_bytes, 64 * 1024 * 1024);
        assert_eq!(config.fuel_amount, 1_000_000);
        assert_eq!(config.timeout_ms, None);
    }

    #[test]
    fn json_round_trip() {
        let config = SandboxConfig {
            memory_limit_bytes: 128 * 1024 * 1024,
            fuel_amount: 500_000,
            timeout_ms: Some(5000),
            max_response_bytes: 8 * 1024 * 1024,
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: SandboxConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.memory_limit_bytes, 128 * 1024 * 1024);
        assert_eq!(deserialized.fuel_amount, 500_000);
        assert_eq!(deserialized.timeout_ms, Some(5000));
    }

    #[test]
    fn custom_values() {
        let config = SandboxConfig {
            memory_limit_bytes: 32 * 1024 * 1024,
            fuel_amount: 100,
            timeout_ms: Some(1000),
            max_response_bytes: 4 * 1024 * 1024,
        };
        assert_eq!(config.memory_limit_bytes, 32 * 1024 * 1024);
        assert_eq!(config.fuel_amount, 100);
        assert_eq!(config.timeout_ms, Some(1000));
    }

    #[test]
    fn serialize_deserialize_yaml() {
        let config = SandboxConfig::default();
        let yaml = serde_yaml::to_string(&config).unwrap();
        let deserialized: SandboxConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(config.memory_limit_bytes, deserialized.memory_limit_bytes);
        assert_eq!(config.fuel_amount, deserialized.fuel_amount);
        assert_eq!(config.timeout_ms, deserialized.timeout_ms);
    }

    #[test]
    fn debug_format() {
        let config = SandboxConfig::default();
        let debug = format!("{config:?}");
        assert!(debug.contains("memory_limit_bytes"));
        assert!(debug.contains("fuel_amount"));
    }
}
