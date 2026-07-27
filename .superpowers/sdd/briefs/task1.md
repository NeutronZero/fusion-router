# Task 1: FeatureFlag, FeatureDefinition, FeatureRegistry

**Files:**
- Create: `src/feature_gate/mod.rs`
- Test: inline `#[cfg(test)] mod tests`

**Interfaces:**
- `FeatureFlag` enum (Streaming, Replay, ConnectorHealth, SemanticCache, WasmPlugins) with serde kebab-case
- `Stability` enum (Experimental, Stable, Deprecated)
- `FeatureDefinition` struct with id, introduced, removed, stability, default_enabled, description
- `FeatureConfig` struct with enabled field
- `FeatureState` struct with id, enabled, overridden, definition
- `FeatureRegistry` with new(), apply_config(), is_enabled(), is_effectively_enabled(), list()

**Key design:** `lookup_map: HashMap<String, FeatureFlag>` is built once during `new()` by iterating `definitions` and calling `serde_json::to_value(&def.id)`. No manual `match` needed.

**Tests:**
- test_feature_flag_serde_round_trip: serialize/deserialize FeatureFlag
- test_feature_registry_defaults: default_enabled=true works
- test_apply_config_disables_feature: override to false
- test_apply_config_unknown_feature_is_ignored: unknown keys don't panic
- test_list_returns_all_features_with_state: list() returns correct entries
- test_lookup_from_definition_works: derived lookup map resolves config names

**Global constraints:** FeatureDefinition uses `&'static` lifetime. Definitions are always static compile-time arrays.
