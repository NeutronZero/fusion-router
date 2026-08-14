# Sprint O2 — Typed Permissions & Capability Registry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add typed `Permission` enum to the ABI, refactor `CapabilityRegistry` into a trait with `InMemoryCapabilityRegistry` impl, and introduce `CapabilityDescriptor` — completing the Capability Discovery Layer.

**Architecture:** Three parts — (1) `Permission` enum in `fusion-plugin-api` with `Validate`, `Display`, `FromStr`; (2) macro/SDK updates to emit/receive typed `Permission` values; (3) `CapabilityRegistry` trait + `InMemoryCapabilityRegistry` + `CapabilityDescriptor` + `RegistryError` in `src/capability/registry.rs`, with runtime helpers in `src/capability/permission.rs`.

**Tech Stack:** Rust 2021 edition, `proc-macro2`/`quote`/`syn` for macros, `serde`/`serde_json` for serialization, `semver` for versioning

## Global Constraints

- `Permission` enum lives in `fusion-plugin-api` (ABI crate), not runtime
- `CapabilityContract.permissions` changes from `Vec<String>` to `Vec<Permission>` — all consumers must migrate
- `CapabilityRegistry` becomes a trait — concrete `InMemoryCapabilityRegistry` implements it
- `freeze()` is non-consuming (`&mut self`), does not consume `Box<Self>`
- `list()` returns contracts sorted by `CapabilityId` for deterministic ordering
- Macro crate emits typed `Permission` values (not strings)
- SDK builder `permission()` accepts `Permission` (not `impl Into<String>`)
- No new execution trait hierarchy introduced
- Zero warnings — `cargo check` + `cargo clippy --all-targets -- -D warnings` clean
- All existing tests pass

---

## File Structure

| File | Responsibility |
|------|---------------|
| **Modify:** `crates/fusion-plugin-api/src/lib.rs` | Add `Permission` enum, `PermissionError`, `Permission::validate()`, `Display`, `FromStr`; change `CapabilityContract.permissions` type |
| **Modify:** `crates/fusion-capability-macros/src/permission.rs` | Add `Environment` variant, add `to_permission_token_stream()` emitting typed `Permission` values |
| **Modify:** `crates/fusion-capability-macros/src/lib.rs` | Use typed permission tokens instead of strings |
| **Modify:** `crates/fusion-capability-sdk/src/builder.rs` | `permission()` takes `Permission`, field becomes `Vec<Permission>` |
| **Modify:** `crates/fusion-capability-sdk/src/prelude.rs` | Add `Permission` re-export |
| **Create:** `src/capability/registry.rs` | `CapabilityRegistry` trait, `RegistryError`, `InMemoryCapabilityRegistry`, `CapabilityDescriptor`, `CapabilitySource` |
| **Create:** `src/capability/permission.rs` | Runtime policy helpers, `PermissionError` display, convenience functions |
| **Modify:** `src/capability/mod.rs` | Re-export new modules, refactor existing concrete struct into trait |
| **Modify:** `src/plugin/manager.rs` | Use `InMemoryCapabilityRegistry` directly, trait boundaries |
| **Modify:** `src/planner/resolver/capability/resolver.rs` | Use `Arc<dyn CapabilityRegistry>` |
| **Modify:** `tests/unit/phase_invariants.rs` | Adapt to typed permissions and new registry API |
| **Modify:** `plugins/fusion-plugin-echo/src/lib.rs` (if applicable) | Update any permission strings |

---

### Task 1: Permission ABI Type in `fusion-plugin-api`

**Files:**
- Modify: `crates/fusion-plugin-api/src/lib.rs`
- Test: within plugin-api crate

**Interfaces:**
- Produces: `fusion_plugin_api::Permission` enum (all variants), `fusion_plugin_api::PermissionError`, `fusion_plugin_api::Permission::validate()`
- Produces: `Display`, `FromStr`, `Debug`, `Clone`, `PartialEq`, `Eq`, `PartialOrd`, `Ord`, `Hash`, `Serialize`, `Deserialize` for `Permission`
- Produces: `CapabilityContract.permissions` changed to `Vec<Permission>`

- [ ] **Step 1: Write failing tests for Permission**

Append to `crates/fusion-plugin-api/src/lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn permission_network_display() {
        assert_eq!(Permission::Network.to_string(), "Network");
    }

    #[test]
    fn permission_filesystem_display() {
        let p = Permission::Filesystem("/tmp".into());
        assert_eq!(p.to_string(), "Filesystem(/tmp)");
    }

    #[test]
    fn permission_http_display() {
        let p = Permission::Http("https://api.example.com".into());
        assert_eq!(p.to_string(), "Http(https://api.example.com)");
    }

    #[test]
    fn permission_from_str_network() {
        let p = Permission::from_str("Network").unwrap();
        assert_eq!(p, Permission::Network);
    }

    #[test]
    fn permission_from_str_filesystem() {
        let p = Permission::from_str("Filesystem(/tmp)").unwrap();
        assert_eq!(p, Permission::Filesystem("/tmp".into()));
    }

    #[test]
    fn permission_round_trips() {
        let cases = vec![
            Permission::Network,
            Permission::Filesystem("/data".into()),
            Permission::Http("https://example.com".into()),
            Permission::Secrets("API_KEY".into()),
            Permission::Environment("HOME".into()),
        ];
        for p in cases {
            let s = p.to_string();
            let back = Permission::from_str(&s).unwrap();
            assert_eq!(p, back, "round-trip failed for {s}");
        }
    }

    #[test]
    fn permission_validate_network_ok() {
        assert!(Permission::Network.validate().is_ok());
    }

    #[test]
    fn permission_validate_empty_filesystem_fails() {
        let p = Permission::Filesystem("".into());
        assert!(p.validate().is_err());
    }

    #[test]
    fn permission_validate_empty_http_fails() {
        let p = Permission::Http("".into());
        assert!(p.validate().is_err());
    }

    #[test]
    fn permission_validate_empty_secrets_fails() {
        let p = Permission::Secrets("".into());
        assert!(p.validate().is_err());
    }

    #[test]
    fn permission_validate_empty_environment_fails() {
        let p = Permission::Environment("".into());
        assert!(p.validate().is_err());
    }

    #[test]
    fn permission_json_round_trip() {
        let p = Permission::Filesystem("/tmp".into());
        let json = serde_json::to_string(&p).unwrap();
        let back: Permission = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn contract_permissions_typed() {
        let contract = CapabilityContract {
            id: CapabilityId::new("test.typed"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "typed permissions".into(),
            inputs_schema: serde_json::json!({}),
            outputs_schema: serde_json::json!({}),
            permissions: vec![Permission::Network, Permission::Http("https://example.com".into())],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 0,
            reliability_score: 1.0,
            supports_streaming: false,
        };
        assert_eq!(contract.permissions.len(), 2);
        assert!(matches!(contract.permissions[0], Permission::Network));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p fusion-plugin-api`
Expected: Compile error — `Permission` not defined, `CapabilityContract.permissions` type mismatch

- [ ] **Step 3: Add `Permission` enum with full trait support**

Replace the `CAPABILITY_ABI_VERSION` constant (update to `"0.2.0"` for ABI version bump) and add `Permission` with all variants after the `CapabilityId` block in `crates/fusion-plugin-api/src/lib.rs`:

```rust
/// Current ABI version for capability packages (ADR-018).
pub const CAPABILITY_ABI_VERSION: &str = "0.2.0";

/// Error type for `Permission::validate()`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PermissionError {
    #[error("permission argument must not be empty")]
    EmptyArgument,
}

/// Typed permission model for capability ABI contracts.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Permission {
    Network,
    Filesystem(String),
    Http(String),
    Secrets(String),
    Environment(String),
}

impl Permission {
    /// Validates that parameterized permissions have non-empty arguments.
    pub fn validate(&self) -> Result<(), PermissionError> {
        match self {
            Permission::Network => Ok(()),
            Permission::Filesystem(path) if path.is_empty() => Err(PermissionError::EmptyArgument),
            Permission::Http(endpoint) if endpoint.is_empty() => Err(PermissionError::EmptyArgument),
            Permission::Secrets(name) if name.is_empty() => Err(PermissionError::EmptyArgument),
            Permission::Environment(name) if name.is_empty() => Err(PermissionError::EmptyArgument),
            _ => Ok(()),
        }
    }
}

impl std::fmt::Display for Permission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Permission::Network => write!(f, "Network"),
            Permission::Filesystem(path) => write!(f, "Filesystem({path})"),
            Permission::Http(endpoint) => write!(f, "Http({endpoint})"),
            Permission::Secrets(name) => write!(f, "Secrets({name})"),
            Permission::Environment(name) => write!(f, "Environment({name})"),
        }
    }
}

impl std::str::FromStr for Permission {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(arg) = s.strip_prefix("Filesystem(").and_then(|s| s.strip_suffix(')')) {
            return Ok(Permission::Filesystem(arg.to_string()));
        }
        if let Some(arg) = s.strip_prefix("Http(").and_then(|s| s.strip_suffix(')')) {
            return Ok(Permission::Http(arg.to_string()));
        }
        if let Some(arg) = s.strip_prefix("Secrets(").and_then(|s| s.strip_suffix(')')) {
            return Ok(Permission::Secrets(arg.to_string()));
        }
        if let Some(arg) = s.strip_prefix("Environment(").and_then(|s| s.strip_suffix(')')) {
            return Ok(Permission::Environment(arg.to_string()));
        }
        if s == "Network" {
            return Ok(Permission::Network);
        }
        Err(format!("unknown permission variant: {s}"))
    }
}
```

Also change `CapabilityContract.permissions` field from `Vec<String>` to `Vec<Permission>`:

```rust
    pub permissions: Vec<Permission>,
```

- [ ] **Step 4: Add `thiserror` dependency**

Add to `crates/fusion-plugin-api/Cargo.toml`:

```toml
thiserror = "2"
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p fusion-plugin-api`
Expected: All permission tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/fusion-plugin-api/
git commit -m "feat(plugin-api): add typed Permission enum with validate, Display, FromStr"
```

---

### Task 2: Macro & SDK Typed Permission Support

**Files:**
- Modify: `crates/fusion-capability-macros/src/permission.rs`
- Modify: `crates/fusion-capability-macros/src/lib.rs`
- Modify: `crates/fusion-capability-sdk/src/builder.rs`
- Modify: `crates/fusion-capability-sdk/src/prelude.rs`
- Modify: `crates/fusion-capability-sdk/src/lib.rs`

**Interfaces:**
- Consumes: `fusion_plugin_api::Permission` from Task 1
- Produces: `PermissionAttr::to_permission_token_stream()` emitting typed `::fusion_plugin_api::Permission::*` tokens
- Produces: `CapabilityBuilder::permission()` accepting `Permission`
- Produces: `Permission` re-exported from SDK prelude

- [ ] **Step 1: Write failing tests for SDK builder with typed permissions**

Add to `crates/fusion-capability-sdk/src/builder.rs` tests:

```rust
    #[test]
    fn builds_with_typed_permissions() {
        use fusion_plugin_api::Permission;
        let contract = CapabilityBuilder::new("test.typed")
            .version("0.1.0")
            .permission(Permission::Network)
            .permission(Permission::Http("https://api.example.com".into()))
            .finish();
        assert_eq!(contract.permissions.len(), 2);
        assert_eq!(contract.permissions[0], Permission::Network);
    }
```

- [ ] **Step 2: Update `PermissionAttr` to emit typed values**

Replace `to_permission_string()` with `to_permission_token_stream()` in `crates/fusion-capability-macros/src/permission.rs`, and add `Environment` variant:

```rust
use proc_macro2::TokenStream;
use quote::quote;

impl PermissionAttr {
    /// Emits a typed `::fusion_plugin_api::Permission::*` token stream.
    pub fn to_permission_token_stream(&self) -> TokenStream {
        match self {
            PermissionAttr::Network => quote! { ::fusion_plugin_api::Permission::Network },
            PermissionAttr::Filesystem(path) => {
                quote! { ::fusion_plugin_api::Permission::Filesystem(#path.into()) }
            }
            PermissionAttr::Http(endpoint) => {
                quote! { ::fusion_plugin_api::Permission::Http(#endpoint.into()) }
            }
            PermissionAttr::Secrets(name) => {
                quote! { ::fusion_plugin_api::Permission::Secrets(#name.into()) }
            }
            PermissionAttr::Environment(name) => {
                quote! { ::fusion_plugin_api::Permission::Environment(#name.into()) }
            }
        }
    }
}

// Add Environment variant to the enum:
pub enum PermissionAttr {
    Network,
    Filesystem(String),
    Http(String),
    Secrets(String),
    Environment(String),
}

// Add Environment parsing in Parse impl:
            "Environment" => {
                let content;
                syn::parenthesized!(content in input);
                let name: LitStr = content.parse()?;
                Ok(PermissionAttr::Environment(name.value()))
            }
```

- [ ] **Step 3: Update macro codegen to use typed tokens**

In `crates/fusion-capability-macros/src/lib.rs`, replace:

```rust
    let permission_strings: Vec<String> = permissions.iter().map(|p| p.to_permission_string()).collect();
    ...
    permissions: vec![#(#permission_strings),*],
```

with:

```rust
    let permission_tokens: Vec<TokenStream> = permissions.iter().map(|p| p.to_permission_token_stream()).collect();
    ...
    permissions: vec![#(#permission_tokens),*],
```

Also add `use proc_macro2::TokenStream;` to the imports (it's already implicit but be explicit).

Also add an `Environment` parsing test to the existing `mod tests` block:

```rust
    #[test]
    fn parse_environment() {
        let attr: PermissionAttr = parse_quote!(Environment("HOME"));
        assert!(matches!(attr, PermissionAttr::Environment(p) if p == "HOME"));
    }
```

- [ ] **Step 4: Update SDK builder**

In `crates/fusion-capability-sdk/src/builder.rs`:

Change field type:
```rust
    permissions: Vec<String>,
```
to:
```rust
    permissions: Vec<Permission>,
```

And add the import alongside existing `fusion_plugin_api` imports:
```rust
use fusion_plugin_api::{CapabilityContract, CapabilityId, Permission};
```

Change method signature:
```rust
    pub fn permission(mut self, permission: Permission) -> Self {
        self.permissions.push(permission);
        self
    }
```

- [ ] **Step 5: Update SDK prelude**

Add `Permission` to `crates/fusion-capability-sdk/src/prelude.rs`:

```rust
pub use fusion_plugin_api::{
    CapabilityPlugin,
    CapabilityContract,
    CapabilityId,
    Permission,
};
```

- [ ] **Step 6: Update `__reexports` in `fusion-capability-sdk/src/lib.rs` if needed**

The macro already references `::fusion_plugin_api::Permission` directly, so no change needed.

- [ ] **Step 7: Run tests**

Run: `cargo test -p fusion-capability-macros -p fusion-capability-sdk`
Expected: All tests pass (existing string-based tests will need updates)

- [ ] **Step 8: Fix failing tests in builder**

Update the `builds_full_contract` test in builder.rs:

```rust
    #[test]
    fn builds_full_contract() {
        let contract = CapabilityBuilder::new("test.full")
            .version("1.0.0")
            .description("A full test capability")
            .permission(Permission::Network)
            .estimated_cost_usd(0.01)
            .estimated_latency_ms(50)
            .reliability_score(0.99)
            .supports_streaming(true)
            .finish();
        assert_eq!(contract.description, "A full test capability");
        assert_eq!(contract.permissions, vec![Permission::Network]);
        assert_eq!(contract.estimated_cost_usd, 0.01);
        assert!(contract.supports_streaming);
    }
```

- [ ] **Step 9: Run tests again**

Run: `cargo test -p fusion-capability-macros -p fusion-capability-sdk`
Expected: All tests pass

- [ ] **Step 10: Commit**

```bash
git add crates/fusion-capability-macros/ crates/fusion-capability-sdk/
git commit -m "feat(macros, sdk): emit and consume typed Permission values"
```

---

### Task 3: Registry Trait & InMemoryCapabilityRegistry

**Files:**
- Create: `src/capability/registry.rs`
- Modify: `src/capability/mod.rs`

**Interfaces:**
- Produces: `CapabilityRegistry` trait, `RegistryError` enum, `InMemoryCapabilityRegistry` impl, `CapabilityDescriptor` struct, `CapabilitySource` enum

- [ ] **Step 1: Replace `src/capability/mod.rs` with submodule declarations**

Replace the entire file (removes the old concrete struct and its test):

```rust
//! Capability Subsystem (`src/capability/mod.rs`)
//!
//! Provides the registry, descriptor, and permission types for the Capability Platform.

pub mod permission;
pub mod registry;

pub use registry::{
    CapabilityRegistry,
    CapabilityDescriptor,
    CapabilitySource,
    InMemoryCapabilityRegistry,
    RegistryError,
};
```

- [ ] **Step 2: Create `src/capability/registry.rs` with full implementation + inline tests**

```rust
use std::collections::HashMap;
use std::fmt;
use serde::{Deserialize, Serialize};
use fusion_plugin_api::{CapabilityContract, CapabilityId};

/// Errors that can occur during registry operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    DuplicateId(CapabilityId),
    Frozen,
    NotFound(CapabilityId),
    InvalidContract(String),
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegistryError::DuplicateId(id) => write!(f, "Duplicate capability ID: {id}"),
            RegistryError::Frozen => write!(f, "Registry is frozen"),
            RegistryError::NotFound(id) => write!(f, "Capability not found: {id}"),
            RegistryError::InvalidContract(msg) => write!(f, "Invalid contract: {msg}"),
        }
    }
}

impl std::error::Error for RegistryError {}

/// Source of a registered capability.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CapabilitySource {
    Builtin,
    Package,
    Development,
    Remote,
}

impl fmt::Display for CapabilitySource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CapabilitySource::Builtin => write!(f, "builtin"),
            CapabilitySource::Package => write!(f, "package"),
            CapabilitySource::Development => write!(f, "development"),
            CapabilitySource::Remote => write!(f, "remote"),
        }
    }
}

/// Discovery metadata wrapping a `CapabilityContract`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityDescriptor {
    pub contract: CapabilityContract,
    pub tags: Vec<String>,
    pub categories: Vec<String>,
    pub discoverable: bool,
    pub provider: Option<String>,
    pub source: CapabilitySource,
}

/// The capability registry trait — answers only "what capabilities are available?"
pub trait CapabilityRegistry: Send + Sync {
    fn register(&mut self, contract: CapabilityContract) -> Result<(), RegistryError>;
    fn get(&self, id: &CapabilityId) -> Option<&CapabilityContract>;
    fn contains(&self, id: &CapabilityId) -> bool;
    fn list(&self) -> Vec<&CapabilityContract>;
    fn freeze(&mut self);
    fn is_frozen(&self) -> bool;
}

/// In-memory implementation of `CapabilityRegistry`.
#[derive(Debug, Clone)]
pub struct InMemoryCapabilityRegistry {
    contracts: HashMap<CapabilityId, CapabilityContract>,
    frozen: bool,
}

impl InMemoryCapabilityRegistry {
    pub fn new() -> Self {
        Self {
            contracts: HashMap::new(),
            frozen: false,
        }
    }
}

impl Default for InMemoryCapabilityRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl CapabilityRegistry for InMemoryCapabilityRegistry {
    fn register(&mut self, contract: CapabilityContract) -> Result<(), RegistryError> {
        if self.frozen {
            return Err(RegistryError::Frozen);
        }
        if self.contracts.contains_key(&contract.id) {
            return Err(RegistryError::DuplicateId(contract.id.clone()));
        }
        self.contracts.insert(contract.id.clone(), contract);
        Ok(())
    }

    fn get(&self, id: &CapabilityId) -> Option<&CapabilityContract> {
        self.contracts.get(id)
    }

    fn contains(&self, id: &CapabilityId) -> bool {
        self.contracts.contains_key(id)
    }

    fn list(&self) -> Vec<&CapabilityContract> {
        let mut result: Vec<&CapabilityContract> = self.contracts.values().collect();
        result.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
        result
    }

    fn freeze(&mut self) {
        self.frozen = true;
    }

    fn is_frozen(&self) -> bool {
        self.frozen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_get() {
        let mut reg = InMemoryCapabilityRegistry::new();
        let contract = CapabilityContract {
            id: CapabilityId::new("test.trait"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "trait test".into(),
            inputs_schema: serde_json::json!({}),
            outputs_schema: serde_json::json!({}),
            permissions: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 0,
            reliability_score: 1.0,
            supports_streaming: false,
        };
        reg.register(contract.clone()).unwrap();
        assert!(reg.contains(&CapabilityId::new("test.trait")));
        assert_eq!(reg.get(&CapabilityId::new("test.trait")), Some(&contract));
    }

    #[test]
    fn freeze_blocks_registration() {
        let mut reg = InMemoryCapabilityRegistry::new();
        let contract = CapabilityContract {
            id: CapabilityId::new("test.freeze"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "freeze test".into(),
            inputs_schema: serde_json::json!({}),
            outputs_schema: serde_json::json!({}),
            permissions: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 0,
            reliability_score: 1.0,
            supports_streaming: false,
        };
        reg.register(contract).unwrap();
        reg.freeze();
        assert!(reg.is_frozen());
        let dup = CapabilityContract {
            id: CapabilityId::new("test.after_freeze"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "should fail".into(),
            inputs_schema: serde_json::json!({}),
            outputs_schema: serde_json::json!({}),
            permissions: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 0,
            reliability_score: 1.0,
            supports_streaming: false,
        };
        match reg.register(dup) {
            Err(RegistryError::Frozen) => {}
            _ => panic!("expected Frozen error"),
        }
    }

    #[test]
    fn duplicate_id_rejected() {
        let mut reg = InMemoryCapabilityRegistry::new();
        let contract = CapabilityContract {
            id: CapabilityId::new("test.dup"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "original".into(),
            inputs_schema: serde_json::json!({}),
            outputs_schema: serde_json::json!({}),
            permissions: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 0,
            reliability_score: 1.0,
            supports_streaming: false,
        };
        reg.register(contract.clone()).unwrap();
        match reg.register(contract) {
            Err(RegistryError::DuplicateId(_)) => {}
            _ => panic!("expected DuplicateId error"),
        }
    }

    #[test]
    fn list_sorted_by_id() {
        let mut reg = InMemoryCapabilityRegistry::new();
        let c1 = CapabilityContract {
            id: CapabilityId::new("z.last"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: String::new(),
            inputs_schema: serde_json::json!({}),
            outputs_schema: serde_json::json!({}),
            permissions: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 0,
            reliability_score: 1.0,
            supports_streaming: false,
        };
        let c2 = CapabilityContract {
            id: CapabilityId::new("a.first"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: String::new(),
            inputs_schema: serde_json::json!({}),
            outputs_schema: serde_json::json!({}),
            permissions: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 0,
            reliability_score: 1.0,
            supports_streaming: false,
        };
        reg.register(c1).unwrap();
        reg.register(c2).unwrap();
        let list = reg.list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id.as_str(), "a.first");
        assert_eq!(list[1].id.as_str(), "z.last");
    }

    #[test]
    fn registry_error_display() {
        let err = RegistryError::Frozen;
        let msg = err.to_string();
        assert!(!msg.is_empty());
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: All tests pass (plugin-api, macros, SDK, and new registry tests)

- [ ] **Step 4: Commit**

```bash
git add src/capability/
git commit -m "feat(capability): add CapabilityRegistry trait, InMemoryCapabilityRegistry, RegistryError, CapabilityDescriptor"
```

---

### Task 4: Runtime Permission Helpers

**Files:**
- Create: `src/capability/permission.rs`

**Interfaces:**
- Produces: Runtime helpers for permission validation, display, conversions

- [ ] **Step 1: Create `src/capability/permission.rs` with helpers and inline tests**

```rust
//! Runtime permission helpers for policy evaluation and convenience.
//!
//! The `Permission` type itself lives in `fusion-plugin-api` (the ABI crate).
//! This module provides runtime-specific utilities.

use fusion_plugin_api::{Permission, PermissionError};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_display_round_trips() {
        let cases = vec![
            Permission::Network,
            Permission::Filesystem("/data".into()),
            Permission::Http("https://example.com".into()),
            Permission::Secrets("API_KEY".into()),
            Permission::Environment("HOME".into()),
        ];
        for p in cases {
            let s = p.to_string();
            let back: Permission = s.parse().unwrap();
            assert_eq!(p, back);
        }
    }

    #[test]
    fn validate_allows_valid() {
        assert!(Permission::Network.validate().is_ok());
        assert!(Permission::Filesystem("/tmp".into()).validate().is_ok());
        assert!(Permission::Http("https://example.com".into()).validate().is_ok());
        assert!(Permission::Secrets("API_KEY".into()).validate().is_ok());
        assert!(Permission::Environment("HOME".into()).validate().is_ok());
    }

    #[test]
    fn validate_rejects_empty() {
        assert!(Permission::Filesystem("".into()).validate().is_err());
        assert!(Permission::Http("".into()).validate().is_err());
        assert!(Permission::Secrets("".into()).validate().is_err());
        assert!(Permission::Environment("".into()).validate().is_err());
    }
}
```

This module is intentionally thin — `Permission` and its validation live in the ABI crate. This file serves as a home for future runtime policy evaluation logic (e.g., permission resolution, intersection checks, sandbox policy mapping).

- [ ] **Step 2: Run compilation check**

Run: `cargo check`
Expected: Clean compilation

- [ ] **Step 3: Commit**

```bash
git add src/capability/permission.rs
git commit -m "feat(capability): add runtime permission helpers"
```

---

### Task 5: Consumer Migration — Update All Downstream Users

**Files:**
- Modify: `src/plugin/manager.rs`
- Modify: `src/planner/resolver/capability/resolver.rs`
- Modify: `tests/unit/phase_invariants.rs`
- Search: any other files referencing `CapabilityRegistry` struct or `Vec<String>` permissions

**Interfaces:**
- Consumes: `CapabilityRegistry` trait, `InMemoryCapabilityRegistry`, `Permission`
- Produces: Updated consumers compiling with new types

- [ ] **Step 1: Find all consumers**

Run: `rg "CapabilityRegistry" --type rust`
Expected: manager.rs, resolver.rs, phase_invariants.rs

Run: `rg "permissions: vec!|permissions:.*String" --type rust -l`
Expected: all files with `Vec<String>` permission values

- [ ] **Step 2: Migrate `PluginManager`**

In `src/plugin/manager.rs`:

Change field type:
```rust
use crate::capability::InMemoryCapabilityRegistry;

pub struct PluginManager {
    registry: PluginRegistry,
    capability_registry: InMemoryCapabilityRegistry,
    manifests: HashMap<String, PluginManifest>,
    ...
}
```

Update `new()`:
```rust
capability_registry: InMemoryCapabilityRegistry::new(),
```

Keep `register_capability_plugin()` as-is (it calls `self.capability_registry.register(contract)` which works via the trait).

Update `freeze_capability_registry()`:
```rust
pub fn freeze_capability_registry(&mut self) -> Arc<InMemoryCapabilityRegistry> {
    let mut empty = InMemoryCapabilityRegistry::new();
    std::mem::swap(&mut self.capability_registry, &mut empty);
    empty.freeze();
    Arc::new(empty)
}
```

Remove `use crate::capability::CapabilityRegistry;` import (replaced by `InMemoryCapabilityRegistry`).

- [ ] **Step 3: Migrate `CapabilityResolver`**

In `src/planner/resolver/capability/resolver.rs`:

Change field type:
```rust
use crate::capability::CapabilityRegistry;

pub struct CapabilityResolver {
    registry: Arc<dyn CapabilityRegistry>,
    cache: CapabilityPlannerCache,
    aliases: HashMap<CapabilityId, CapabilityId>,
}
```

Update `new()`:
```rust
pub fn new(registry: Arc<dyn CapabilityRegistry>) -> Self {
```

Update test helper:
```rust
fn build_test_registry() -> Arc<dyn CapabilityRegistry> {
    use crate::capability::InMemoryCapabilityRegistry;
    let mut reg = InMemoryCapabilityRegistry::new();
    reg.register(CapabilityContract {
        id: CapabilityId::new("echo.text"),
        version: semver::Version::parse("0.1.0").unwrap(),
        description: "Echo text".into(),
        inputs_schema: json!({}),
        outputs_schema: json!({}),
        permissions: vec![],
        estimated_cost_usd: 0.0,
        estimated_latency_ms: 10,
        reliability_score: 1.0,
        supports_streaming: false,
    }).unwrap();
    reg.register(CapabilityContract {
        id: CapabilityId::new("echo.uppercase"),
        version: semver::Version::parse("0.1.0").unwrap(),
        description: "Echo uppercase".into(),
        inputs_schema: json!({}),
        outputs_schema: json!({}),
        permissions: vec![],
        estimated_cost_usd: 0.0,
        estimated_latency_ms: 50,
        reliability_score: 1.0,
        supports_streaming: false,
    }).unwrap();
    reg.freeze();
    Arc::new(reg)
}
```

- [ ] **Step 4: Migrate `phase_invariants.rs`**

In `tests/unit/phase_invariants.rs`:

Change import:
```rust
use fusion_router::capability::InMemoryCapabilityRegistry;
```

Update `invariant_capability_registry_immutable_post_freeze`:
```rust
fn invariant_capability_registry_immutable_post_freeze() {
    let mut reg = InMemoryCapabilityRegistry::new();
    let contract = CapabilityContract {
        id: CapabilityId::new("test.invar"),
        version: semver::Version::parse("0.1.0").unwrap(),
        description: "Invariant test".into(),
        inputs_schema: json!({}),
        outputs_schema: json!({}),
        permissions: vec![],
        estimated_cost_usd: 0.0,
        estimated_latency_ms: 1,
        reliability_score: 1.0,
        supports_streaming: false,
    };
    reg.register(contract).unwrap();
    reg.freeze();
    assert!(reg.is_frozen());
    assert!(reg.contains(&CapabilityId::new("test.invar")));
}
```

Update `invariant_capability_resolver_does_not_execute_logic`:
```rust
fn invariant_capability_resolver_does_not_execute_logic() {
    use fusion_router::capability::CapabilityRegistry;
    let mut reg = InMemoryCapabilityRegistry::new();
    reg.register(CapabilityContract {
        id: CapabilityId::new("pure.symbol"),
        version: semver::Version::parse("0.1.0").unwrap(),
        description: "Symbol only".into(),
        inputs_schema: json!({}),
        outputs_schema: json!({}),
        permissions: vec![],
        estimated_cost_usd: 0.0,
        estimated_latency_ms: 1,
        reliability_score: 1.0,
        supports_streaming: false,
    }).unwrap();
    reg.freeze();
    let resolver = CapabilityResolver::new(Arc::new(reg));
    ...
}
```

- [ ] **Step 5: Update `PluginManager` tests**

In `src/plugin/manager.rs` tests — import `CapabilityId` from `fusion_plugin_api` (already done). The tests should compile as-is since `freeze_capability_registry` returns `Arc<InMemoryCapabilityRegistry>` but the test calls `.is_frozen()`, `.contains()`, `.list()` which are all on the trait.

- [ ] **Step 6: Fix any other `CapabilityContract` construction with permissions**

Search for `permissions: vec!` in the codebase and ensure all values are `Permission` not `String`.

- [ ] **Step 7: Compile-check**

Run: `cargo check`
Expected: Clean compilation, zero warnings

- [ ] **Step 8: Run all tests**

Run: `cargo test`
Expected: All tests pass (including existing capability registry tests, capability resolver tests, plugin manager tests, phase invariants)

- [ ] **Step 9: Commit**

```bash
git add src/plugin/manager.rs src/planner/resolver/capability/resolver.rs tests/unit/phase_invariants.rs
git commit -m "refactor: migrate consumers to typed Permission and trait-based CapabilityRegistry"
```

---

### Task 6: Integration Verification

**Files:**
- Verify: whole workspace

- [ ] **Step 1: Full test suite**

Run: `cargo test --all-features`
Expected: All tests pass

- [ ] **Step 2: Clippy check**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: Zero warnings

- [ ] **Step 3: Check no-default-features**

Run: `cargo check --no-default-features --lib`
Expected: Compiles without optional runtime features

- [ ] **Step 4: Check default-features library**

Run: `cargo check --lib`
Expected: Compiles with all default features

- [ ] **Step 5: Verify ABI constant**

Run: `rg "CAPABILITY_ABI_VERSION" crates/fusion-plugin-api/src/lib.rs`
Expected: Shows `"0.2.0"`

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "chore: Sprint O2 integration verification"
```
