# FusionRouter Operator & Operations Guide

## Platform Operations & Health Management

### 1. Deployment Modes
- **Single Instance:** Run `fusion-server` binary listening on `http://127.0.0.1:8080`.
- **Embedded Database:** Persists `fusion_data.db` with automated SQLite migrations.

### 2. Platform Health & Recovery Engine
Evaluate 9 health domains via `/api/v1/diagnostics`:
- `Platform`, `Compiler`, `Runtime`, `Providers`, `Storage`, `Security`, `Studio`, `Plugins`, `Configuration`.

### 3. Automated Safe Recovery
When provider or storage issues arise, trigger safe recovery via:
- `ReconnectProvider`: Re-tests active provider endpoints.
- `RetryDiscovery`: Re-scans local model ports.
- `RevalidateConfig`: Validates database schema snapshot.
