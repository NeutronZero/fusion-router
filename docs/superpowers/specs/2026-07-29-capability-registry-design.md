# Sprint O2 — Typed Permissions & Capability Registry Design

* **Status:** Draft
* **Date:** 2026-07-29
* **Subsystem:** Capability Platform / Discovery Layer

---

## Context

Sprint O1 established the developer SDK with `#[capability]` macro and builders. Permissions are currently `Vec<String>` — string-based and untyped. The existing `CapabilityRegistry` in `src/capability/mod.rs` is a concrete struct, not a trait, limiting polymorphism. Sprint O2 introduces a typed `Permission` model in the ABI and a trait-based registry.

---

## Architecture

```
fusion-plugin-api (ABI)
────────────────────────
CapabilityContract
Permission          ← NEW
CapabilityId
Plugin Traits

↓

fusion-capability-macros
  → emits Permission::Network directly

↓

fusion-capability-sdk
  → re-exports Permission, builder accepts Permission

↓

src/capability/
  permission.rs     ← runtime policy helpers
  registry.rs       ← CapabilityRegistry trait + InMemory impl + Descriptor
  mod.rs            ← re-exports
```

---

## 1. `Permission` Enum in `fusion-plugin-api`

```rust
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Permission {
    Network,
    Filesystem(String),
    Http(String),
    Secrets(String),
    Environment(String),
}
```

### Traits & conversions

- `Display` — round-trippable string representation for logging and manifests
- `FromStr` — parse from display format (migration path)
- `validate(&self) -> Result<(), PermissionError>` — lightweight validation (empty endpoint, empty name, etc.)

### ABI change

- `CapabilityContract.permissions` changes from `Vec<String>` to `Vec<Permission>`
- ADR-018 ABI version updated to reflect the typed permission model

---

## 2. `CapabilityRegistry` Trait

```rust
pub trait CapabilityRegistry: Send + Sync {
    fn register(&mut self, contract: CapabilityContract) -> Result<(), RegistryError>;
    fn get(&self, id: &CapabilityId) -> Option<&CapabilityContract>;
    fn contains(&self, id: &CapabilityId) -> bool;
    fn list(&self) -> Vec<&CapabilityContract>;
    fn freeze(&mut self);
    fn is_frozen(&self) -> bool;
}
```

### RegistryError

```rust
pub enum RegistryError {
    DuplicateId(CapabilityId),
    Frozen,
    NotFound(CapabilityId),
    InvalidContract(String),
}
```

### InMemoryCapabilityRegistry

Concrete implementation replacing the current concrete `CapabilityRegistry` struct. Uses `HashMap<CapabilityId, CapabilityContract>` internally. Supports freeze via a boolean flag (non-consuming).

**Deterministic ordering:** `list()` returns contracts sorted by `CapabilityId` for deterministic CLI output, tests, and serialization.

**Freeze invariant:** After `freeze()` completes successfully, no subsequent `register()` operation may mutate registry state.

---

## 3. `CapabilityDescriptor` & `CapabilitySource`

Enhanced discovery metadata wrapping `CapabilityContract`:

```rust
pub struct CapabilityDescriptor {
    pub contract: CapabilityContract,
    pub tags: Vec<String>,
    pub categories: Vec<String>,
    pub discoverable: bool,
    pub provider: Option<String>,
    pub source: CapabilitySource,
}

pub enum CapabilitySource {
    Builtin,
    Package,
    Development,
    Remote,
}
```

---

## 4. File Map

| File | Responsibility |
|------|---------------|
| **Modify:** `crates/fusion-plugin-api/src/lib.rs` | Add `Permission` enum, change `CapabilityContract.permissions` type |
| **Modify:** `crates/fusion-capability-macros/src/permission.rs` | `PermissionAttr` → `Permission` value conversion |
| **Modify:** `crates/fusion-capability-macros/src/lib.rs` | Generated code emits `Permission::Network` instead of `"Network".to_string()` |
| **Modify:** `crates/fusion-capability-sdk/src/prelude.rs` | Add `Permission` re-export |
| **Modify:** `crates/fusion-capability-sdk/src/builder.rs` | `permission()` accepts `Permission` |
| **Create:** `src/capability/registry.rs` | `CapabilityRegistry` trait, `RegistryError`, `InMemoryCapabilityRegistry`, `CapabilityDescriptor`, `CapabilitySource` |
| **Create:** `src/capability/permission.rs` | Runtime policy helpers (validation, display) |
| **Modify:** `src/capability/mod.rs` | Re-export new modules, refactor existing concrete registry |

---

## 5. Validation Pipeline

Validation occurs at three stages:

1. **Builders:** validate permissions before producing a `CapabilityContract`
2. **Registry:** validates during `register()`, rejecting invalid contracts
3. **Runtime:** assumes registered contracts are already valid (no re-validation hot path)

This creates a single, tiered validation pipeline where errors are caught early.

---

## 6. Future Considerations

`RegistryError::InvalidContract(String)` is sufficient for O2. In future sprints, this may be split into structured causes (e.g., invalid permissions, malformed metadata, duplicate capability declarations). The enum should remain `#[non_exhaustive]` to allow this evolution.

---

## 7. Testing

- Unit tests for `Permission` serialization round-trips
- Unit tests for `Permission::validate()` edge cases (empty strings)
- Unit tests for `InMemoryCapabilityRegistry` (register, freeze, duplicate detection)
- Unit tests for `RegistryError` display
- Update existing `CapabilityRegistry` test to use typed permissions

---

## 8. Success Criteria

1. `Permission` is a typed enum in `fusion-plugin-api` with full trait support
2. `CapabilityContract.permissions` is `Vec<Permission>`
3. `CapabilityRegistry` is a trait with `InMemoryCapabilityRegistry` impl
4. Registry uses non-consuming `freeze()`
5. Macros emit typed `Permission` values
6. SDK builder accepts `Permission` values
7. All existing tests pass with typed permissions
