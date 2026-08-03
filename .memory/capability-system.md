# FusionRouter Capability System

## Overview

The capability system provides a unified abstraction for extending FusionRouter with reusable, versioned, and permission-scoped execution units called **capabilities**. It replaces the older ad-hoc plugin/tool/connector distinctions with a single, unified execution model.

**Location:** `src/capability/`, `src/planner/resolver/capability/`, `crates/fusion-capability-sdk/`, `crates/fusion-plugin-api/`

## Architecture

### Capability Registry (`src/capability/registry.rs`)

- Mutable during initialization, **frozen** after startup (ADR-021)
- Immutable `CapabilityRegistry` at runtime
- Indexes capabilities by `CapabilityId`
- Entry point for capability resolution

### Capability Resolver (`src/planner/resolver/capability/resolver.rs`)

- Resolves abstract capability references to concrete `CapabilityInstance` objects
- Called during compiler's **Capability Resolution** pass
- Uses `CapabilityGraph` for dependency DAG traversal
- LRU cache for resolved capability plans

### Capability Graph (`src/planner/resolver/capability/graph.rs`)

- Dependency DAG between capabilities
- Ensures correct ordering of capability execution
- Detects circular dependencies

### Permissions (`src/capability/permission.rs`)

| Permission | Description |
|------------|-------------|
| `Network` | Network access |
| `Filesystem(path)` | Scoped filesystem access |
| `Http(endpoint)` | Specific HTTP endpoint access |
| `Secrets(name)` | Named secret access |
| `Environment(name)` | Named environment variable access |

## SDK (`crates/fusion-capability-sdk/`)

| Component | File | Purpose |
|-----------|------|---------|
| CapabilityBuilder | `crates/fusion-capability-sdk/src/builder.rs` | Builder pattern for capability construction |
| CapabilityManifestBuilder | `crates/fusion-capability-sdk/src/manifest.rs` | Builder for plugin manifests |
| SchemaBuilder | `crates/fusion-capability-sdk/src/schema.rs` | JSON Schema builder for inputs/outputs |
| Prelude | `crates/fusion-capability-sdk/src/prelude.rs` | Common re-exports |
| Macros | `crates/fusion-capability-macros/` | `#[capability]` attribute macro |

The `#[capability(id, description, version)]` attribute macro auto-generates `Plugin` and `CapabilityPlugin` trait implementations from annotated structs.

## Plugin API (`crates/fusion-plugin-api/`)

Minimal public SDK for building capability plugins.

### Key Types

| Type | Description |
|------|-------------|
| `CapabilityId(String)` | Strongly-typed identifier (e.g., `echo.text`) |
| `Permission` enum | 5 permission variants with scoping |
| `PluginMetadata` | Version compatibility: name, api_version, min_compiler_version, capabilities |
| `CapabilityContract` | Declarative ABI: id, version, description, JSON Schema inputs/outputs, permissions, dependencies, cost/latency/reliability, streaming support |
| `CapabilityInstance` | Bound runtime execution object |
| `ExecutionResult` | Standardized output (outputs `Value`, metrics `HashMap<String,f64>`) |
| `ExecutionError` | Structured error: connector, capability, reason, retryable flag |

### Key Traits

| Trait | Method | Purpose |
|-------|--------|---------|
| `Plugin` | `fn metadata() -> PluginMetadata` | Plugin identity |
| `CapabilityPlugin` | `fn capabilities() -> Vec<CapabilityContract>` | Capability declarations |
| `CapabilityExecutor` | `async fn execute(instance, input) -> Result<ExecutionResult, ExecutionError>` | Execution logic |

### ABI Version

Current: `CAPABILITY_ABI_VERSION = "0.2.0"`

## Plugin Package Format (ADR-018, ADR-019)

Binary plugins distributed as `.fusionpkg`:
- Gzipped tarball containing `manifest.toml`, `module.wasm`, `attestation.json`
- WASM runtime via Wasmtime (feature-gated: `wasm-plugins`)
- Fuel metering for WASM execution
- 5-function FFI bridge for WASM-host interaction

## Key Invariants

- Capability resolution is late-bound (at compilation time, not planning time)
- Registry is frozen after startup — no runtime registration
- Capability execution is unified via `CapabilityExecutor`
- ABI version negotiation ensures compatibility
- Permissions are declared in contract, enforced at execution
- Policy (deny/allow lists) is enforced on **every** resolution path — required, version-constrained, optional, and transitive dependencies (incl. the final resolved instance set) — and any violation fails resolution (H13 / ADR-034, v0.13.1). `apply_policy` checks both the requested id (after alias resolution) and the resolved contract id; with an allow list present, both must be listed.
- Caching caveat: the planner cache (`CapabilityPlannerCache`) keys on the full `RequirementSet` (which includes `policy`), so policy changes never reuse a cached resolution.

## Related ADRs

- ADR-021: Capability Platform (registry freeze, unified execution)
- ADR-022: Plugin ABI (version negotiation, metadata/execution separation)
- ADR-023: Capability Resolution (dedicated resolver, dependency graph)
- ADR-028: Capability Contract Evolution (SemVer, aliasing, deprecation)
- ADR-018 (docs/adrs/): Capability Binary Interface (`.fusionpkg` format)
- ADR-019 (docs/adrs/): Capability Host Interface (host services trait)
