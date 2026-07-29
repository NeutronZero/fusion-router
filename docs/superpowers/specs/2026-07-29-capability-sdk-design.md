# Sprint O1 — Capability SDK Design

* **Status:** Draft
* **Date:** 2026-07-29
* **Subsystem:** Capability Platform / Developer SDK

---

## Context

The v0.12 Capability Platform requires a developer SDK that allows plugin authors to declare capabilities ergonomically while preserving the stable ABI established by `fusion-plugin-api`. The existing plugin API provides core contract types (`CapabilityContract`, `CapabilityId`, `Plugin`, `CapabilityPlugin`, `CapabilityExecutor`) but requires substantial boilerplate for metadata declaration, schema generation, and registration.

---

## Architecture

Three-crate stack with strict layering:

```
Plugin Author
     │
     ▼
fusion-capability-sdk (DX Layer)
  CapabilityBuilder, CapabilityManifestBuilder, SchemaBuilder, prelude
     │
     ▼
fusion-capability-macros (Code Generation)
  #[capability], #[permission(...)]
     │
     ▼
fusion-plugin-api (Stable ABI)
  CapabilityContract, CapabilityId, Plugin, CapabilityPlugin, CapabilityExecutor
     │
     ▼
FusionRouter Runtime
```

Each crate has one responsibility:
- `fusion-plugin-api` — minimal public ABI, no macros, no SDK concerns
- `fusion-capability-macros` — purely code generation, no runtime logic
- `fusion-capability-sdk` — developer ergonomics, re-exports, builders

---

## crate: `fusion-capability-macros`

### `#[capability]` attribute macro

Placed on a struct that implements capability logic. Generates implementations of the **existing** `Plugin` and `CapabilityPlugin` traits from `fusion-plugin-api`.

```rust
#[capability(
    id = "echo.text",
    description = "Echoes input text back",
    version = "0.1.0"
)]
#[permission(Network)]
struct EchoCapability;
```

### Generated code

The macro expands to:

```rust
impl Plugin for EchoCapability {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata { ... }
    }
}

impl CapabilityPlugin for EchoCapability {
    fn capabilities(&self) -> Vec<CapabilityContract> {
        vec![CapabilityContract { ... }]
    }
}
```

Each `CapabilityContract` is generated from the annotated attributes, including:
- `id` → `CapabilityId`
- `version` → `semver::Version`
- `description`
- `inputs_schema` / `outputs_schema` — derived from annotated types or inline `schema = "..."` paths
- `permissions` — from `#[permission(...)]` helper attrs → typed `Permission` values
- ADR-018 ABI version embedded in generated metadata
- Generated schema hash for verification

### `#[permission(...)]` helper attribute

Scalable syntax mapping to the typed permission model (Sprint O2):

```rust
#[permission(Network)]

#[permission(Filesystem("/tmp"))]

#[permission(Http("https://api.example.com"))]

#[permission(Secrets("OPENAI_API_KEY"))]
```

Multiple `#[permission]` attrs are merged into the capability's permission list.

### Responsibilities (macro crate only)

- Parsing attributes
- Generating metadata and trait impls
- Compile-time validation (duplicate IDs, invalid semver, malformed permissions, missing required fields)
- **Not:** manifest parsing, packaging, registry logic, filesystem access

---

## crate: `fusion-capability-sdk`

### CapabilityBuilder

Fluent, immutable builder for constructing `CapabilityContract` at runtime (without macros):

```rust
let contract = CapabilityBuilder::new("echo.text")
    .description("Echoes input text back")
    .version("0.1.0")
    .permission(Permission::Network)
    .finish(); // returns CapabilityContract, immutable after finish
```

### CapabilityManifestBuilder

Constructs ADR-018 manifest stubs for packaging (forward-looking, minimal in O1):

```rust
let manifest = CapabilityManifestBuilder::new(contract)
    .abi_version("0.1.0")
    .build();
```

### SchemaBuilder

Abstraction around JSON Schema derivation:

```rust
let schema = SchemaBuilder::derive::<MyInputType>().finish();
```

Forward-looking — in O1 may start as a thin wrapper around `schemars` or a manual JSON builder.

### Prelude

Intentional small prelude:

```rust
pub use fusion_capability_macros::capability;
pub use fusion_plugin_api::{
    CapabilityPlugin,
    CapabilityContract,
    CapabilityId,
    Permission,
};
pub use crate::{
    CapabilityBuilder,
    CapabilityManifestBuilder,
};
```

---

## File map

| File | Purpose |
|------|---------|
| `crates/fusion-capability-macros/Cargo.toml` | `[lib] proc-macro = true` crate config |
| `crates/fusion-capability-macros/src/lib.rs` | `#[capability]` proc macro |
| `crates/fusion-capability-macros/src/permission.rs` | Permission attribute parsing |
| `crates/fusion-capability-sdk/Cargo.toml` | SDK crate config, deps on macros + plugin-api |
| `crates/fusion-capability-sdk/src/lib.rs` | Re-exports, prelude, crate root |
| `crates/fusion-capability-sdk/src/prelude.rs` | `pub use` prelude module |
| `crates/fusion-capability-sdk/src/builder.rs` | `CapabilityBuilder` |
| `crates/fusion-capability-sdk/src/manifest.rs` | `CapabilityManifestBuilder` |
| `crates/fusion-capability-sdk/src/schema.rs` | `SchemaBuilder` |

---

## Testing

### Unit tests (crate-level)

- Builder API produces correct `CapabilityContract` values
- `CapabilityManifestBuilder` constructs valid stubs
- `SchemaBuilder` derives valid JSON Schema

### Compile-fail tests (`fusion-capability-macros`)

Using `trybuild`:

- Duplicate capability IDs
- Invalid semantic version
- Malformed permission attribute
- Missing required metadata
- Unsupported field types

### Verification

```
cargo test -p fusion-capability-macros
cargo test -p fusion-capability-sdk
cargo test -p fusion-capability-macros -p fusion-capability-sdk
```

---

## Success criteria

1. `#[capability]` generates valid implementations of existing `Plugin` / `CapabilityPlugin` traits
2. No new execution trait hierarchy introduced
3. Plugin authors only depend on `fusion-capability-sdk`
4. `fusion-plugin-api` remains the stable ABI crate
5. Macro expansion validates metadata at compile time
6. Builders produce immutable contracts
7. SDK exports a clean `prelude`

---

## Forward compatibility

```
O1: #[capability] → CapabilityContract → Registry
O2: Typed Permission model
O3: SandboxRuntime, Wasmtime
O3.5: CapabilityHostServices (ADR-019)
O4: CapabilityManifestBuilder → .fusionpkg (ADR-018)
```
