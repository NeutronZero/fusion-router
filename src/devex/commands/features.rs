use crate::feature_gate::{FeatureFlag, FeatureRegistry};

pub fn list_features(registry: &FeatureRegistry) -> String {
    let states = registry.list();
    if states.is_empty() {
        return "No features registered.".to_string();
    }
    let mut output = String::new();
    output.push_str("Registered features:\n");
    for state in states {
        let status = if state.enabled { "ENABLED" } else { "DISABLED" };
        let override_marker = if state.overridden { " (overridden)" } else { "" };
        output.push_str(&format!(
            "  [{}] {}{} - v{} - {}\n",
            status,
            flag_name(&state.definition.id),
            override_marker,
            state.definition.introduced,
            state.definition.description,
        ));
    }
    output
}

fn flag_name(flag: &FeatureFlag) -> String {
    serde_json::to_value(flag)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| format!("{:?}", flag))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature_gate::{FeatureDefinition, FeatureFlag, FeatureRegistry, Stability};

    static TEST_DEFS: &[FeatureDefinition] = &[FeatureDefinition {
        id: FeatureFlag::Streaming,
        introduced: "1.0.0",
        removed: None,
        stability: Stability::Stable,
        default_enabled: true,
        description: "Enable streaming responses",
    }];

    #[test]
    fn test_list_features_empty() {
        let registry = FeatureRegistry::new(&[]);
        let output = list_features(&registry);
        assert_eq!(output, "No features registered.");
    }

    #[test]
    fn test_list_features_with_entries() {
        let registry = FeatureRegistry::new(TEST_DEFS);
        let output = list_features(&registry);
        assert!(output.contains("Registered features:"));
        assert!(output.contains("ENABLED"));
        assert!(output.contains("Enable streaming responses"));
    }
}
