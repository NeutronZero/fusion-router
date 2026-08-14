# ADR-022: Plugin SDK, Version Negotiation & Separation of Concerns

- **Status**: Proposed
- **Date**: July 2026
- **Context**: FusionRouter v0.10.0 Plugin Architecture
- **Deciders**: FusionRouter Core Architecture Team

---

## Context

To make FusionRouter extensible by third-party ecosystem developers, we must establish a stable C-ABI / Rust trait SDK (`fusion-plugin-api`), enforce plugin version negotiation, and separate metadata declarations from runtime execution code.

---

## Decisions

### 1. Version Negotiation

Every plugin must declare explicit metadata detailing API and compiler version compatibility:

```rust
pub struct PluginMetadata {
    pub name: String,
    pub version: semver::Version,
    pub api_version: semver::Version,
    pub min_compiler_version: semver::Version,
    pub capabilities: Vec<CapabilityId>,
}
```

During plugin discovery, `PluginManager` invokes the `CompatibilityChecker`. If a plugin requires an incompatible API or compiler version, loading is safely rejected with diagnostic telemetry before registry freeze.

### 2. Strict Separation of Metadata vs. Execution

Plugins separate metadata declaration from runtime execution logic:

- **`CapabilityContract` / `CapabilityDescriptor` (Metadata)**: Exposes input schemas, output schemas, cost model, latency, permissions, side-effects, and streaming support. Consumed exclusively by the **Planner** and **Capability Resolver**.
- **`CapabilityExecutor` (Runtime)**: Contains actual execution code or WASM bytecode. Consumed exclusively by the **Scheduler / Runtime**.

```text
       Plugin
       ┌──┴────────────────────────┐
       ▼                           ▼
CapabilityDescriptor     CapabilityExecutor
 (Planner Sees This)    (Scheduler Sees This)
```

### 3. Execution Isolation Strategy

- **v0.10.0**: Rust C-ABI trait plugins (`libloading`), WASM plugins (`wasmtime`), in-tree static plugins.
- **v0.11.0**: Out-of-process gRPC / IPC / Remote plugins.
- **v1.0.0**: Distributed capability marketplace.

---

## Consequences

- Clear separation of concerns between Planner (lightweight metadata lookups) and Scheduler (heavyweight execution).
- Prevents version drift breakage when engine compiler versions advance.
