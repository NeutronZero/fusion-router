use std::fs;
use std::path::Path;

pub struct PluginScaffolder;

impl PluginScaffolder {
    pub fn new() -> Self {
        Self
    }

    pub fn scaffold_plugin<P: AsRef<Path>>(&self, path: P, name: &str) -> std::io::Result<()> {
        let base_path = path.as_ref().join(name);
        fs::create_dir_all(&base_path)?;
        
        let cargo_toml = format!(r#"
[package]
name = "{}"
version = "0.1.0"
edition = "2021"

[dependencies]
fusion-plugin-api = {{ path = "../../crates/fusion-plugin-api" }}
"#, name);

        let src_dir = base_path.join("src");
        fs::create_dir_all(&src_dir)?;

        let lib_rs = r#"
use fusion_plugin_api::{Plugin, PluginMetadata};

pub struct MyPlugin;

impl Plugin for MyPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: env!("CARGO_PKG_NAME").to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            description: "A scaffolded plugin".to_string(),
        }
    }
}
"#;

        fs::write(base_path.join("Cargo.toml"), cargo_toml)?;
        fs::write(src_dir.join("lib.rs"), lib_rs)?;

        Ok(())
    }

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
