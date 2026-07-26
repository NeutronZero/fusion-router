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
}
