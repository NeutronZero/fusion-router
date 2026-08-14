# Sprint O1 — Capability SDK Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the `fusion-capability-sdk` and `fusion-capability-macros` crates providing `#[capability]` macro, builders, and prelude — all building on `fusion-plugin-api` with no new execution trait hierarchy.

**Architecture:** Three-crate stack — `fusion-plugin-api` (stable ABI, unchanged except one constant), `fusion-capability-macros` (proc-macro crate for `#[capability]` and `#[permission]`), `fusion-capability-sdk` (re-exports, builders, prelude). SDK depends on macros; macros depend on plugin-api.

**Tech Stack:** Rust 2021 edition, `proc-macro2`, `quote`, `syn`, `trybuild` (dev), `schemars` (optional, forward-looking)

## Global Constraints

- No new execution trait hierarchy — `#[capability]` generates impls of existing `Plugin` / `CapabilityPlugin` traits
- `fusion-plugin-api` remains the stable ABI crate, may only gain constants, not new public traits
- SDK re-exports from macros — plugin authors use `fusion_capability_sdk::prelude::*`
- All builders produce immutable contracts after `finish()`
- Macro crate is purely code generation — no runtime logic, no filesystem access, no packaging
- Prelude is intentionally small (macro, traits, contract types, builder types)
- `CAPABILITY_ABI_VERSION` defined as a shared constant in `fusion-plugin-api`

---

## File Structure

| File | Responsibility |
|------|---------------|
| **Create:** `crates/fusion-capability-macros/Cargo.toml` | proc-macro crate config (`[lib] proc-macro = true`) |
| **Create:** `crates/fusion-capability-macros/src/lib.rs` | `#[capability]` proc macro entry point |
| **Create:** `crates/fusion-capability-macros/src/permission.rs` | `#[permission]` attribute parsing |
| **Create:** `crates/fusion-capability-sdk/Cargo.toml` | SDK crate config |
| **Create:** `crates/fusion-capability-sdk/src/lib.rs` | SDK re-exports |
| **Create:** `crates/fusion-capability-sdk/src/prelude.rs` | `pub use` prelude module |
| **Create:** `crates/fusion-capability-sdk/src/builder.rs` | `CapabilityBuilder` |
| **Create:** `crates/fusion-capability-sdk/src/manifest.rs` | `CapabilityManifestBuilder` |
| **Create:** `crates/fusion-capability-sdk/src/schema.rs` | `SchemaBuilder` |
| **Modify:** `Cargo.toml` | Add workspace members |
| **Modify:** `crates/fusion-plugin-api/src/lib.rs` | Add `CAPABILITY_ABI_VERSION` constant |

---

### Task 1: Crate Scaffolding & ABI Constant

**Files:**
- Create: `crates/fusion-capability-macros/Cargo.toml`
- Create: `crates/fusion-capability-sdk/Cargo.toml`
- Create: `crates/fusion-capability-macros/src/lib.rs` (minimal stub)
- Create: `crates/fusion-capability-sdk/src/lib.rs` (minimal stub)
- Modify: `Cargo.toml` (workspace members)
- Modify: `crates/fusion-plugin-api/src/lib.rs` (add constant)

**Interfaces:**
- Consumes: existing `fusion-plugin-api` crate
- Produces: `crates/fusion-capability-macros/` and `crates/fusion-capability-sdk/` directory scaffolding; `fusion_plugin_api::CAPABILITY_ABI_VERSION` constant

- [ ] **Step 1: Create `crates/fusion-capability-macros/Cargo.toml`**

```toml
[package]
name = "fusion-capability-macros"
version = "0.1.0"
edition = "2021"
license = "MIT OR Apache-2.0"
description = "Procedural macros for FusionRouter capability declarations"

[lib]
proc-macro = true

[dependencies]
proc-macro2 = "1"
quote = "1"
syn = { version = "2", features = ["full", "extra-traits"] }
semver = { version = "1.0", features = ["serde"] }
fusion-plugin-api = { path = "../fusion-plugin-api" }

[dev-dependencies]
trybuild = "1"
```

- [ ] **Step 2: Create `crates/fusion-capability-sdk/Cargo.toml`**

```toml
[package]
name = "fusion-capability-sdk"
version = "0.1.0"
edition = "2021"
license = "MIT OR Apache-2.0"
description = "Developer SDK for building FusionRouter capabilities"

[dependencies]
fusion-capability-macros = { path = "../fusion-capability-macros" }
fusion-plugin-api = { path = "../fusion-plugin-api" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
semver = { version = "1.0", features = ["serde"] }
schemars = { version = "0.8", optional = true }

[dev-dependencies]
serde_json = "1"
```

- [ ] **Step 3: Add workspace members in root `Cargo.toml`**

Edit `Cargo.toml` workspace `members` array to include:
```toml
members = [
    "crates/fusion-plugin-api",
    "crates/fusion-capability-macros",
    "crates/fusion-capability-sdk",
    "plugins/example-provider",
    "plugins/fusion-plugin-echo",
]
```

- [ ] **Step 4: Add `CAPABILITY_ABI_VERSION` constant to `fusion-plugin-api`**

In `crates/fusion-plugin-api/src/lib.rs`, add:
```rust
/// Current ABI version for capability packages (ADR-018).
pub const CAPABILITY_ABI_VERSION: &str = "0.1.0";
```

- [ ] **Step 5: Create minimal `fusion-capability-macros/src/lib.rs` stub**

```rust
use proc_macro::TokenStream;

#[proc_macro_attribute]
pub fn capability(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}
```

- [ ] **Step 6: Create minimal `fusion-capability-sdk/src/lib.rs` stub**

```rust
pub mod prelude;
```

- [ ] **Step 7: Create minimal `prelude.rs`**

```rust
pub use fusion_capability_macros::capability;
```

- [ ] **Step 8: Verify workspace compilation**

Run: `cargo check`
Expected: Clean compilation, no warnings

---

### Task 2: `#[permission]` Attribute Parsing

**Files:**
- Create: `crates/fusion-capability-macros/src/permission.rs`
- Modify: `crates/fusion-capability-macros/src/lib.rs`

**Interfaces:**
- Consumes: syn types
- Produces: `PermissionAttr::parse(input: &[syn::Meta]) -> Vec<PermissionAttr>` — parsed permission variants from `#[permission(...)]` attributes

- [ ] **Step 1: Create `permission.rs` with permission attribute parsing**

```rust
use syn::{parse::{Parse, ParseStream}, Token, LitStr, Path};

/// Represents a single `#[permission(...)]` attribute value.
/// Maps to the typed `Permission` enum planned for Sprint O2.
#[derive(Debug, Clone)]
pub enum PermissionAttr {
    Network,
    Filesystem(String),
    Http(String),
    Secrets(String),
}

impl PermissionAttr {
    /// Returns the permission variant name (e.g. "Network", "Filesystem").
    pub fn variant_name(&self) -> &'static str {
        match self {
            PermissionAttr::Network => "Network",
            PermissionAttr::Filesystem(_) => "Filesystem",
            PermissionAttr::Http(_) => "Http",
            PermissionAttr::Secrets(_) => "Secrets",
        }
    }

    /// Returns the string representation used in `CapabilityContract.permissions`.
    /// Single-arg variants include the value: `"Http(https://...)"`.
    pub fn to_permission_string(&self) -> String {
        match self {
            PermissionAttr::Network => "Network".to_string(),
            PermissionAttr::Filesystem(path) => format!("Filesystem({path})"),
            PermissionAttr::Http(url) => format!("Http({url})"),
            PermissionAttr::Secrets(name) => format!("Secrets({name})"),
        }
    }
}

impl Parse for PermissionAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let path: Path = input.parse()?;
        let ident = path.get_ident()
            .ok_or_else(|| input.error("expected identifier"))?;
        let name = ident.to_string();

        match name.as_str() {
            "Network" => {
                if input.peek(Token![,]) || input.is_empty() {
                    Ok(PermissionAttr::Network)
                } else {
                    Err(input.error("Network takes no arguments"))
                }
            }
            "Filesystem" => {
                let content;
                syn::parenthesized!(content in input);
                let path: LitStr = content.parse()?;
                Ok(PermissionAttr::Filesystem(path.value()))
            }
            "Http" => {
                let content;
                syn::parenthesized!(content in input);
                let url: LitStr = content.parse()?;
                Ok(PermissionAttr::Http(url.value()))
            }
            "Secrets" => {
                let content;
                syn::parenthesized!(content in input);
                let name: LitStr = content.parse()?;
                Ok(PermissionAttr::Secrets(name.value()))
            }
            _ => Err(syn::Error::new_spanned(&path, format!("unknown permission variant: {name}")))
        }
    }
}

/// Parses `#[permission(...)]` from struct attributes.
pub fn parse_permission_attrs(attrs: &[syn::Attribute]) -> Vec<PermissionAttr> {
    attrs.iter()
        .filter(|a| a.path().is_ident("permission"))
        .filter_map(|a| a.parse_args::<PermissionAttr>().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn parse_network() {
        let attr: PermissionAttr = parse_quote!(Network);
        assert!(matches!(attr, PermissionAttr::Network));
    }

    #[test]
    fn parse_filesystem() {
        let attr: PermissionAttr = parse_quote!(Filesystem("/tmp"));
        assert!(matches!(attr, PermissionAttr::Filesystem(p) if p == "/tmp"));
    }

    #[test]
    fn parse_http() {
        let attr: PermissionAttr = parse_quote!(Http("https://api.example.com"));
        assert!(matches!(attr, PermissionAttr::Http(u) if u == "https://api.example.com"));
    }

    #[test]
    fn parse_secrets() {
        let attr: PermissionAttr = parse_quote!(Secrets("OPENAI_API_KEY"));
        assert!(matches!(attr, PermissionAttr::Secrets(k) if k == "OPENAI_API_KEY"));
    }
}
```

- [ ] **Step 2: Add `mod permission;` to `fusion-capability-macros/src/lib.rs`**

```rust
mod permission;
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p fusion-capability-macros`
Expected: All unit tests pass

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: add #[permission] attribute parsing"
```

---

### Task 3: `#[capability]` Proc Macro

**Files:**
- Modify: `crates/fusion-capability-macros/src/lib.rs`

**Interfaces:**
- Produces: `#[capability(...)]` proc-macro attribute that, when applied to a struct, generates `Plugin` and `CapabilityPlugin` impls using the existing `fusion-plugin-api` traits

- [ ] **Step 1: Add `semver` as a direct dependency of the macro crate and add `#[doc(hidden)]` re-export modules**

Add `semver` to `crates/fusion-capability-macros/Cargo.toml`:
```toml
semver = { version = "1.0", features = ["serde"] }
```

Then in `crates/fusion-capability-macros/src/lib.rs`, add a re-export module so the SDK can proxy `semver` types to generated code:
```rust
/// Re-exports for use in generated code.
/// Not part of the public API.
#[doc(hidden)]
pub mod __reexports {
    pub use semver;
}
```

In `crates/fusion-capability-sdk/src/lib.rs`, add re-exports so user crates that depend on the SDK can resolve all types used in macro-generated code:
```rust
/// Re-exports for macro-generated code.
/// Not part of the public API — use prelude instead.
#[doc(hidden)]
pub mod __reexports {
    pub use fusion_capability_macros as __macros;
    pub use serde_json;
}
```

- [ ] **Step 1b: Write compile-fail test for invalid semver**

Create `crates/fusion-capability-macros/tests/compile-fail/invalid-semver.rs`:

```rust
use fusion_capability_macros::capability;

#[capability(
    id = "test.bad",
    description = "bad version",
    version = "not-a-version"
)]
struct BadVersion;
```

- [ ] **Step 2: Implement `#[capability]` proc macro**

Replace the stub in `crates/fusion-capability-macros/src/lib.rs`:

```rust
mod permission;

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemStruct};

struct CapabilityArgs {
    id: String,
    description: String,
    version: String,
}

impl syn::parse::Parse for CapabilityArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut id = None;
        let mut description = None;
        let mut version = None;

        while !input.is_empty() {
            let key: syn::Ident = input.parse()?;
            let _: syn::Token![=] = input.parse()?;
            let value: syn::LitStr = input.parse()?;

            match key.to_string().as_str() {
                "id" => id = Some(value.value()),
                "description" => description = Some(value.value()),
                "version" => version = Some(value.value()),
                other => {
                    return Err(syn::Error::new_spanned(&key, format!("unknown capability attribute: {other}")));
                }
            }

            if !input.is_empty() {
                let _: syn::Token![,] = input.parse()?;
            }
        }

        Ok(CapabilityArgs {
            id: id.ok_or_else(|| input.error("missing required attribute: id"))?,
            description: description.ok_or_else(|| input.error("missing required attribute: description"))?,
            version: version.ok_or_else(|| input.error("missing required attribute: version"))?,
        })
    }
}

    fn validate_semver(version: &str) -> Result<::semver::Version, String> {
    ::semver::Version::parse(version).map_err(|e| format!("invalid semver version '{version}': {e}"))
}

#[proc_macro_attribute]
pub fn capability(attr: TokenStream, item: TokenStream) -> TokenStream {
    let item_struct = parse_macro_input!(item as ItemStruct);
    let struct_name = &item_struct.ident;

    let args = match syn::parse::<CapabilityArgs>(attr) {
        Ok(a) => a,
        Err(e) => return e.to_compile_error().into(),
    };

    let id = &args.id;
    let description = &args.description;
    let version = &args.version;

    if let Err(e) = validate_semver(version) {
        return syn::Error::new_spanned(&item_struct, e).to_compile_error().into();
    }

    let permissions = permission::parse_permission_attrs(&item_struct.attrs);
    let permission_strings: Vec<String> = permissions.iter().map(|p| p.to_permission_string()).collect();

    // Use __fusion_capability to resolve transitive dependency semver
    // through the SDK re-export. Plugin authors depend on the SDK, not the macro crate.
    let expanded = quote! {
        #item_struct

        impl ::fusion_plugin_api::Plugin for #struct_name {
            fn metadata(&self) -> ::fusion_plugin_api::PluginMetadata {
                ::fusion_plugin_api::PluginMetadata {
                    name: #id.to_string(),
                    version: ::fusion_capability_sdk::__reexports::__macros::__reexports::semver::Version::parse(#version).unwrap(),
                    api_version: ::fusion_capability_sdk::__reexports::__macros::__reexports::semver::Version::parse(::fusion_plugin_api::CAPABILITY_ABI_VERSION).unwrap(),
                    min_compiler_version: ::fusion_capability_sdk::__reexports::__macros::__reexports::semver::Version::parse("0.11.0").unwrap(),
                    capabilities: vec![::fusion_plugin_api::CapabilityId::new(#id)],
                }
            }
        }

        impl ::fusion_plugin_api::CapabilityPlugin for #struct_name {
            fn capabilities(&self) -> Vec<::fusion_plugin_api::CapabilityContract> {
                vec![
                    ::fusion_plugin_api::CapabilityContract {
                        id: ::fusion_plugin_api::CapabilityId::new(#id),
                        version: ::fusion_capability_sdk::__reexports::__macros::__reexports::semver::Version::parse(#version).unwrap(),
                        description: #description.to_string(),
                        inputs_schema: ::fusion_capability_sdk::__reexports::serde_json::Value::Object(Default::default()),
                        outputs_schema: ::fusion_capability_sdk::__reexports::serde_json::Value::Object(Default::default()),
                        permissions: vec![#(#permission_strings),*],
                        estimated_cost_usd: 0.0,
                        estimated_latency_ms: 0,
                        reliability_score: 1.0,
                        supports_streaming: false,
                    }
                ]
            }
        }
    };

    TokenStream::from(expanded)
}
```

- [ ] **Step 3: Create compile-fail test runner**

Create `crates/fusion-capability-macros/tests/compiletest.rs`:

```rust
#[test]
fn compile_fail() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile-fail/*.rs");
}
```

- [ ] **Step 4: Run macro tests**

Run: `cargo test -p fusion-capability-macros`
Expected: Unit tests pass (permission parsing), compile-fail catches invalid semver

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: add #[capability] proc macro"
```

---

### Task 4: SDK Builders & Prelude

**Files:**
- Modify: `crates/fusion-capability-sdk/src/lib.rs`
- Create: `crates/fusion-capability-sdk/src/prelude.rs`
- Create: `crates/fusion-capability-sdk/src/builder.rs`
- Create: `crates/fusion-capability-sdk/src/manifest.rs`
- Create: `crates/fusion-capability-sdk/src/schema.rs`

**Interfaces:**
- Produces: `CapabilityBuilder` (fluent, immutable after `finish()`), `CapabilityManifestBuilder`, `SchemaBuilder`, prelude module

- [ ] **Step 1: Create `builder.rs` — `CapabilityBuilder`**

```rust
use fusion_plugin_api::{CapabilityContract, CapabilityId};
use semver::Version;
use serde_json::Value;

/// Fluent builder for constructing immutable `CapabilityContract` values.
///
/// # Example
/// ```
/// use fusion_capability_sdk::CapabilityBuilder;
///
/// let contract = CapabilityBuilder::new("echo.text")
///     .description("Echoes text back")
///     .version("0.1.0")
///     .finish();
///
/// assert_eq!(contract.id.as_str(), "echo.text");
/// ```
#[derive(Debug, Clone)]
pub struct CapabilityBuilder {
    id: String,
    version: Option<Version>,
    description: Option<String>,
    inputs_schema: Option<Value>,
    outputs_schema: Option<Value>,
    permissions: Vec<String>,
    estimated_cost_usd: f64,
    estimated_latency_ms: u64,
    reliability_score: f32,
    supports_streaming: bool,
}

impl CapabilityBuilder {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            version: None,
            description: None,
            inputs_schema: None,
            outputs_schema: None,
            permissions: Vec::new(),
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 0,
            reliability_score: 1.0,
            supports_streaming: false,
        }
    }

    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(Version::parse(&version.into()).expect("invalid semver version"));
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn inputs_schema(mut self, schema: Value) -> Self {
        self.inputs_schema = Some(schema);
        self
    }

    pub fn outputs_schema(mut self, schema: Value) -> Self {
        self.outputs_schema = Some(schema);
        self
    }

    pub fn permission(mut self, permission: impl Into<String>) -> Self {
        self.permissions.push(permission.into());
        self
    }

    pub fn estimated_cost_usd(mut self, cost: f64) -> Self {
        self.estimated_cost_usd = cost;
        self
    }

    pub fn estimated_latency_ms(mut self, latency: u64) -> Self {
        self.estimated_latency_ms = latency;
        self
    }

    pub fn reliability_score(mut self, score: f32) -> Self {
        self.reliability_score = score;
        self
    }

    pub fn supports_streaming(mut self, streaming: bool) -> Self {
        self.supports_streaming = streaming;
        self
    }

    /// Finalizes and returns an immutable `CapabilityContract`.
    pub fn finish(self) -> CapabilityContract {
        CapabilityContract {
            id: CapabilityId::new(self.id),
            version: self.version.unwrap_or_else(|| Version::new(0, 1, 0)),
            description: self.description.unwrap_or_default(),
            inputs_schema: self.inputs_schema.unwrap_or(Value::Object(Default::default())),
            outputs_schema: self.outputs_schema.unwrap_or(Value::Object(Default::default())),
            permissions: self.permissions,
            estimated_cost_usd: self.estimated_cost_usd,
            estimated_latency_ms: self.estimated_latency_ms,
            reliability_score: self.reliability_score,
            supports_streaming: self.supports_streaming,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_minimal_contract() {
        let contract = CapabilityBuilder::new("test.ping")
            .version("0.1.0")
            .finish();
        assert_eq!(contract.id.as_str(), "test.ping");
        assert_eq!(contract.version.to_string(), "0.1.0");
    }

    #[test]
    fn builds_full_contract() {
        let contract = CapabilityBuilder::new("test.full")
            .version("1.0.0")
            .description("A full test capability")
            .permission("Network")
            .estimated_cost_usd(0.01)
            .estimated_latency_ms(50)
            .reliability_score(0.99)
            .supports_streaming(true)
            .finish();
        assert_eq!(contract.description, "A full test capability");
        assert_eq!(contract.permissions, vec!["Network"]);
        assert_eq!(contract.estimated_cost_usd, 0.01);
        assert!(contract.supports_streaming);
    }

    #[test]
    fn contract_is_immutable_after_finish() {
        let contract = CapabilityBuilder::new("test.immutable")
            .version("0.1.0")
            .finish();
        // Verify it's a plain CapabilityContract with no builder methods
        let _: CapabilityContract = contract;
    }

    #[test]
    #[should_panic(expected = "invalid semver version")]
    fn invalid_version_panics() {
        CapabilityBuilder::new("bad.version").version("not-a-version");
    }
}
```

- [ ] **Step 2: Create `manifest.rs` — `CapabilityManifestBuilder`**

```rust
use fusion_plugin_api::CapabilityContract;
use serde::Serialize;

/// ADR-018 manifest stub builder.
/// Full manifest validation is provided in Sprint O4.
#[derive(Debug, Clone, Serialize)]
pub struct CapabilityManifest {
    pub abi_version: String,
    pub capability_id: String,
    pub capability_version: String,
    pub description: String,
}

/// Builder for constructing ADR-018 package manifest stubs.
#[derive(Debug, Clone)]
pub struct CapabilityManifestBuilder {
    contract: CapabilityContract,
    abi_version: Option<String>,
}

impl CapabilityManifestBuilder {
    pub fn new(contract: CapabilityContract) -> Self {
        Self {
            contract,
            abi_version: None,
        }
    }

    pub fn abi_version(mut self, version: impl Into<String>) -> Self {
        self.abi_version = Some(version.into());
        self
    }

    pub fn build(self) -> CapabilityManifest {
        CapabilityManifest {
            abi_version: self.abi_version.unwrap_or_else(|| fusion_plugin_api::CAPABILITY_ABI_VERSION.to_string()),
            capability_id: self.contract.id.to_string(),
            capability_version: self.contract.version.to_string(),
            description: self.contract.description,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CapabilityBuilder;

    #[test]
    fn builds_manifest() {
        let contract = CapabilityBuilder::new("test.ping")
            .version("0.1.0")
            .description("ping capability")
            .finish();
        let manifest = CapabilityManifestBuilder::new(contract)
            .abi_version("0.1.0")
            .build();
        assert_eq!(manifest.capability_id, "test.ping");
        assert_eq!(manifest.abi_version, "0.1.0");
    }

    #[test]
    fn manifest_default_abi() {
        let contract = CapabilityBuilder::new("test.ping")
            .version("0.1.0")
            .finish();
        let manifest = CapabilityManifestBuilder::new(contract).build();
        assert_eq!(manifest.abi_version, fusion_plugin_api::CAPABILITY_ABI_VERSION);
    }
}
```

- [ ] **Step 3: Create `schema.rs` — `SchemaBuilder`**

```rust
use serde_json::Value;

/// Builder for JSON Schema derivation.
///
/// In Sprint O1 this provides a manual construction API.
/// Future sprints will add automatic derivation from Rust types via `schemars`.
#[derive(Debug, Clone, Default)]
pub struct SchemaBuilder {
    schema: Option<Value>,
}

impl SchemaBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets an explicit JSON Schema value.
    pub fn schema(mut self, schema: Value) -> Self {
        self.schema = Some(schema);
        self
    }

    /// Derives a JSON Schema from a Rust type using `schemars` (requires `schemars` feature).
    #[cfg(feature = "schemars")]
    pub fn derive<T: schemars::JsonSchema>() -> Self {
        let schema = schemars::schema_for!(T);
        Self {
            schema: Some(serde_json::to_value(&schema).unwrap_or_default()),
        }
    }

    pub fn finish(self) -> Value {
        self.schema.unwrap_or(Value::Object(Default::default()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_schema() {
        let schema = SchemaBuilder::new().finish();
        assert_eq!(schema, json!({}));
    }

    #[test]
    fn explicit_schema() {
        let schema = SchemaBuilder::new()
            .schema(json!({"type": "string"}))
            .finish();
        assert_eq!(schema, json!({"type": "string"}));
    }

    #[cfg(feature = "schemars")]
    #[test]
    fn derive_schema() {
        #[derive(schemars::JsonSchema)]
        struct Input {
            value: String,
        }
        let schema = SchemaBuilder::derive::<Input>().finish();
        assert!(schema.is_object());
    }
}
```

- [ ] **Step 4: Create `prelude.rs`**

```rust
//! Intentionally small prelude for capability plugin authors.

pub use fusion_capability_macros::capability;

pub use fusion_plugin_api::{
    CapabilityPlugin,
    CapabilityContract,
    CapabilityId,
};

pub use crate::{
    CapabilityBuilder,
    CapabilityManifestBuilder,
};
```

- [ ] **Step 5: Update `fusion-capability-sdk/src/lib.rs`**

```rust
pub mod builder;
pub mod manifest;
pub mod schema;
pub mod prelude;

pub use builder::CapabilityBuilder;
pub use manifest::CapabilityManifestBuilder;
pub use schema::SchemaBuilder;
pub use prelude::*;
```

- [ ] **Step 6: Run SDK tests**

Run: `cargo test -p fusion-capability-sdk`
Expected: All unit tests pass

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: add CapabilityBuilder, ManifestBuilder, SchemaBuilder, prelude"
```

---

### Task 5: Integration Verification

**Files:**
- Create: `tests/capability_sdk_integration.rs`
- Verify: All crates compile, tests pass, clippy clean

- [ ] **Step 1: Create integration test at workspace level**

Create `tests/capability_sdk_integration.rs`:

```rust
//! Integration test verifying the SDK + macros work together
//! through the public prelude API.

use fusion_capability_sdk::prelude::*;

struct EchoCapability;

impl CapabilityPlugin for EchoCapability {
    fn capabilities(&self) -> Vec<CapabilityContract> {
        vec![
            CapabilityBuilder::new("echo.text")
                .description("Echoes input text")
                .version("0.1.0")
                .finish()
        ]
    }
}

#[test]
fn sdk_and_plugin_api_integration() {
    let cap = EchoCapability;
    let contracts = cap.capabilities();
    assert_eq!(contracts.len(), 1);
    assert_eq!(contracts[0].id.as_str(), "echo.text");
}

#[test]
fn manifest_from_contract() {
    let contract = CapabilityBuilder::new("test.pack")
        .description("Test packaging")
        .version("1.0.0")
        .finish();

    let manifest = CapabilityManifestBuilder::new(contract)
        .abi_version(fusion_plugin_api::CAPABILITY_ABI_VERSION)
        .build();

    assert_eq!(manifest.capability_version, "1.0.0");
}

// --- Full-stack macro expansion test ---
// Uses #[capability] macro (re-exported through SDK prelude) to verify
// that generated code referencing ::fusion_capability_sdk::__reexports resolves.

#[capability(
    id = "echo.text",
    description = "Echoes input text",
    version = "0.1.0"
)]
struct MacroEchoCapability;

#[test]
fn macro_generates_plugin_trait() {
    use fusion_plugin_api::Plugin;
    let cap = MacroEchoCapability;
    let meta = cap.metadata();
    assert_eq!(meta.name, "echo.text");
}

#[test]
fn macro_generates_capability_plugin_trait() {
    use fusion_plugin_api::CapabilityPlugin;
    let cap = MacroEchoCapability;
    let contracts = cap.capabilities();
    assert_eq!(contracts.len(), 1);
    assert_eq!(contracts[0].id.as_str(), "echo.text");
}
```

- [ ] **Step 2: Add `CAPABILITY_ABI_VERSION` to `fusion-plugin-api` exports if not done**

Ensure the constant is `pub` and accessible from integration tests.

- [ ] **Step 3: Run full test suite**

Run: `cargo test -p fusion-capability-macros -p fusion-capability-sdk`
Expected: All tests pass

- [ ] **Step 4: Clippy check**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: No warnings

- [ ] **Step 5: Verify no-default-features build**

Run: `cargo check -p fusion-capability-sdk --no-default-features`
Expected: Compiles without schemars

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: add integration tests and SDK verification"
```
