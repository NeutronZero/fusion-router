use std::fs;
use std::path::Path;

pub struct PluginScaffolder;

impl PluginScaffolder {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PluginScaffolder {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginScaffolder {
    pub fn scaffold_plugin<P: AsRef<Path>>(&self, path: P, name: &str) -> std::io::Result<()> {
        let base_path = path.as_ref().join(name);
        fs::create_dir_all(&base_path)?;

        let cargo_toml = format!(
            r#"
[package]
name = "{}"
version = "0.1.0"
edition = "2021"

[dependencies]
fusion-plugin-api = {{ path = "../../crates/fusion-plugin-api" }}
semver = {{ version = "1.0", features = ["serde"] }}
"#,
            name
        );

        let src_dir = base_path.join("src");
        fs::create_dir_all(&src_dir)?;

        // Mirrors the current `Plugin` trait contract (see
        // crates/fusion-plugin-api/src/lib.rs): metadata must carry
        // api_version, min_compiler_version and the capability list or the
        // scaffolded plugin will not compile.
        let lib_rs = r#"
use fusion_plugin_api::{CapabilityId, Plugin, PluginMetadata};
use semver::Version;

pub struct MyPlugin;

impl Plugin for MyPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: env!("CARGO_PKG_NAME").to_string(),
            version: Version::parse(env!("CARGO_PKG_VERSION")).expect("valid version"),
            api_version: Version::parse("0.2.0").expect("valid api version"),
            min_compiler_version: Version::parse("0.11.0").expect("valid compiler version"),
            capabilities: vec![CapabilityId::new("my_plugin.echo")],
        }
    }
}
"#;

        fs::write(base_path.join("Cargo.toml"), cargo_toml)?;
        fs::write(src_dir.join("lib.rs"), lib_rs)?;

        Ok(())
    }

    /// Fields the generated plugin `lib.rs` must contain for the current
    /// `PluginMetadata` contract; used by the template completeness test.
    #[cfg(test)]
    pub(crate) const REQUIRED_TEMPLATE_FIELDS: &[&str] = &[
        "api_version",
        "min_compiler_version",
        "capabilities",
        "CapabilityId::new",
        "Version::parse(env!(\"CARGO_PKG_VERSION\"))",
    ];

    pub fn scaffold_capability<P: AsRef<Path>>(&self, path: P, name: &str) -> std::io::Result<()> {
        let base_path = path.as_ref().join(name);
        fs::create_dir_all(base_path.join("src"))?;
        fs::create_dir_all(base_path.join("tests"))?;

        let cargo_toml = format!(
            r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
fusion-capability-sdk = {{ path = "path/to/fusion-capability-sdk" }}
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"

[dev-dependencies]
fusion-router = {{ path = "path/to/fusion-router" }}
"#
        );

        let lib_rs = format!(
            r#"use fusion_capability_sdk::prelude::*;

#[capability(id = "{name}", description = "A scaffolded capability", version = "0.1.0")]
pub struct MyCapability;
"#
        );

        let manifest_toml = format!(
            r#"[package]
name = "{name}"
version = "0.1.0"

[capabilities]
{name} = {{ description = "A scaffolded capability", version = "0.1.0" }}
"#
        );

        let integration_rs = format!(
            r#"use fusion_router::devex::testing::mock_host::MockHostServices;
use fusion_router::devex::testing::native_runtime::NativeSandboxRuntime;
use fusion_plugin_api::CapabilityId;

#[test]
fn test_{name}_scaffolded() {{
    let mut rt = NativeSandboxRuntime::new();
    rt.register(
        CapabilityId::new("{name}"),
        Box::new({name}::MyCapability),
    );
    assert!(rt.contains(&CapabilityId::new("{name}")));
}}
"#
        );

        fs::write(base_path.join("Cargo.toml"), cargo_toml)?;
        fs::write(base_path.join("src/lib.rs"), lib_rs)?;
        fs::write(base_path.join("manifest.toml"), manifest_toml)?;
        fs::write(base_path.join("tests/integration.rs"), integration_rs)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scaffold_plugin_template_matches_current_metadata_contract() {
        let dir = tempfile::tempdir().unwrap();
        PluginScaffolder::new()
            .scaffold_plugin(dir.path(), "my-plugin")
            .unwrap();

        let lib = fs::read_to_string(dir.path().join("my-plugin/src/lib.rs")).unwrap();
        for field in PluginScaffolder::REQUIRED_TEMPLATE_FIELDS {
            assert!(
                lib.contains(field),
                "scaffolded plugin template must contain '{field}' (current PluginMetadata \
                 contract requires api_version/min_compiler_version/capabilities)"
            );
        }

        // The template must no longer reference the removed `description`
        // metadata field.
        assert!(
            !lib.contains("description:"),
            "stale 'description' field must not appear in the template"
        );

        let cargo = fs::read_to_string(dir.path().join("my-plugin/Cargo.toml")).unwrap();
        assert!(
            cargo.contains("semver"),
            "template Cargo.toml must declare the semver dependency"
        );
    }
}
