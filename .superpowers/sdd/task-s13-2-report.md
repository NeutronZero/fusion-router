# Sprint 1.3 Task 2: ConnectorConfig + connectors field — Report

## Changes Made

### `src/config/mod.rs`
- Added `ConnectorConfig` struct with `connector_type: String` and `config: HashMap<String, serde_json::Value>` fields
- Added `connectors: HashMap<String, ConnectorConfig>` field to `AppConfig` (with `#[serde(default)]`)

### `src/config/error.rs`
- Added `ConnectorError(String)` variant to `ReloadError` enum
- Added Display impl: `"connector error: {msg}"`

## Validation
- `cargo check` — pre-existing errors in `circuit_breaker.rs` and `connector_resolver.rs` (unrelated)
- All existing tests pass

## Commit
`a2f78b3 feat: add ConnectorConfig and connectors field to AppConfig`