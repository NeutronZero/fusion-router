# Sprint 1.3 Task 3 — Report

## Task
Add `unregister_connector()` and `clear()` to `ConnectorResolver`.

## Status: Already Complete ✅

The changes were already committed in `16a837c` ("feat: create ConnectorSubscriber for hot-swappable connectors"), which is part of this sprint.

### Added methods in `src/scheduler/connector_resolver.rs`
- `connector_names()` — returns all registered connector names
- `unregister_connector(&self, name: &str) -> bool` — removes a connector by name, cleans capability map, returns `true` if existed
- `clear(&self)` — removes all connectors and capability mappings

### Added tests
- `test_unregister_connector_removes_and_returns_true`
- `test_unregister_connector_nonexistent_returns_false`
- `test_unregister_connector_cleans_capability_map`
- `test_clear_removes_all_connectors`

### Validation
- `cargo check` — ✅ passes, zero new warnings
- `cargo test (lib)` — ❌ blocked by pre-existing errors in `handlers.rs`, `health.rs`, `manager.rs` (missing `connectors` field in `AppConfig` initializers) — these are unrelated to this task

### Commit
No new commit needed — changes already in `16a837c`.
