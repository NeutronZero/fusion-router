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
