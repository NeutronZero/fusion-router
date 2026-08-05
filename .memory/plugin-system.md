# FusionRouter Plugin System

## Overview

The plugin system provides extension points for providers, strategies, compiler passes, tools, and capabilities. Plugins can be native Rust crates or WASM modules.

**Location:** `src/plugin/`, `crates/fusion-plugin-api/`, `src/wasm/` (feature-gated)

## Extension Points (ADR-010)

| Extension Point | Trait | Registration |
|-----------------|-------|-------------|
| Providers | `Provider` | `ProviderRegistry` |
| Strategies | `Strategy` | Strategy loader |
| Compiler Passes | `CompilerPass` | fixed `build_compiler` list (ADR-034) |
| Tools | `Tool` | `ToolRegistry` |
| Capabilities | `CapabilityPlugin` | `CapabilityRegistry` |

## Plugin Manager (`src/plugin/manager.rs`)

- Scans plugin directories for TOML manifests
- Loads and validates plugin metadata
- Registrations happen at startup only
- WASM loading is feature-gated (`wasm-plugins`)

### Plugin Manifest (`src/plugin/manifest.rs`)

TOML-based manifest format describing:
- Plugin metadata (name, version, author)
- Declared capabilities
- Dependencies
- Required permissions

## WASM Runtime (`src/wasm/`)

**Feature gate:** `wasm-plugins`

| Component | File | Purpose |
|-----------|------|---------|
| WASM Runtime | `src/wasm/runtime.rs` | Wasmtime 47-based execution |
| Fuel Metering | `src/wasm/mod.rs` | Execution fuel budgeting |
| FFI Bridge | `src/wasm/mod.rs` | 5-function host interface for WASM plugins |

### WASM Host Interface Functions

1. `emit_event` — Emit runtime event
2. `log` — Log message
3. `fetch_secret` — Access named secret
4. `http_request` — Outbound HTTP request
5. `record_metric` — Record custom metric

## Capability Plugins

### Native Plugins

Rust crates using `fusion-plugin-api`:
- Implement `Plugin`, `CapabilityPlugin`, `CapabilityExecutor` traits
- Can use `#[capability]` macro from `fusion-capability-macros`
- Built with `CapabilityBuilder` from `fusion-capability-sdk`

### WASM Plugins

Distributed as `.fusionpkg` packages (ADR-018):
- Gzipped tarball: `manifest.toml`, `module.wasm`, `attestation.json`
- Typed permission scoping via `CapabilityContract`
- WASI memory/sandbox invariants

## Plugin ABI (ADR-022)

- `CAPABILITY_ABI_VERSION = "0.2.0"` for version negotiation
- `PluginMetadata` for compatibility checks (name, version, api_version, min_compiler_version)
- Separation of metadata (`CapabilityContract`) from execution (`CapabilityExecutor`)

## Key Invariants

- Plugin discovery happens at startup only
- No hot-reload of plugins
- WASM plugins are sandboxed via Wasmtime
- ABI version negotiation prevents incompatible plugins
- Capability registration freezes after startup

## Related ADRs

- ADR-010: 4 extension points, TOML manifest, WASM runtime
- ADR-022: Plugin ABI, version negotiation, metadata/execution separation
- ADR-018 (docs/adrs/): Capability Binary Interface (`.fusionpkg`)
- ADR-019 (docs/adrs/): Capability Host Interface (host services)
