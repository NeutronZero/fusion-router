# Sprint 1.3 Task 4: Create ConnectorSubscriber — Report

## Summary

Created `ConnectorSubscriber` — a `ConfigSubscriber` that hot-swaps connectors on config reload using the two-phase prepare/commit pattern modeled after `ProviderRegistry`.

## Files Changed

| File | Action | Description |
|------|--------|-------------|
| `src/scheduler/connector_subscriber.rs` | **Created** | New `ConnectorSubscriber` struct + `create_connector` factory |
| `src/scheduler/connector_resolver.rs` | **Modified** | Added `connector_names()`, `unregister_connector()`, and `clear()` methods to `ConnectorResolver` |
| `src/scheduler/mod.rs` | **Modified** | Added `pub mod connector_subscriber;` |

## Implementation Details

### `ConnectorSubscriber`

- **Priority**: 5 (between low-level and provider subscribers)
- **prepare()**: Validates each `ConnectorConfig` from the new snapshot, builds candidate connectors via `create_connector()`, stores in `self.candidates`
- **commit()**: Takes candidates, computes added/removed/updated diff for logging, clears the resolver, re-registers all candidates

### `create_connector()` factory

Maps `connector_type` strings (`"http"`, `"shell"`, `"github"`, `"filesystem"`, `"browser"`, `"mcp"`) to their respective `::new()` constructors. Unknown types return `ReloadError::ConnectorError`.

### ConnectorResolver additions

- `connector_names() → Vec<String>`: Lists registered connector names (used in commit diff logging)
- `unregister_connector(name) → bool`: Removes a connector and its capability mappings
- `clear()`: Removes all connectors and capability mappings (used in commit to fully rebuild)

## Connector Constructors

All six connectors (`HttpConnector`, `ShellConnector`, `GitHubConnector`, `FilesystemConnector`, `BrowserConnector`, `McpConnector`) have `::new() → Self` and implement `Default`. No config parameters are passed (reserved for future sprints).

## Validation

- **`cargo check --lib`**: ✅ Passes with zero new warnings
- **`cargo test`**: ❌ 3 pre-existing test failures (unrelated — missing `connectors` field in test `AppConfig` constructors from earlier tasks)
