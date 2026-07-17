use std::collections::HashMap;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginMeta,
    #[serde(default)]
    pub provider: Option<ProviderConfig>,
    #[serde(default)]
    pub strategy: Option<StrategyConfig>,
    #[serde(default)]
    pub pass: Option<PassConfig>,
    #[serde(default)]
    pub tool: Option<ToolConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PluginMeta {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub entry: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub models: Vec<String>,
    pub config: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StrategyConfig {
    pub kind: String,
    pub config: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PassConfig {
    pub name: String,
    pub config: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolConfig {
    pub name: String,
    pub config: HashMap<String, serde_json::Value>,
}

impl PluginManifest {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let manifest: PluginManifest = toml::from_str(&content)?;
        Ok(manifest)
    }

    pub fn discover(dir: &str) -> Vec<(String, Self)> {
        let dir_path = std::path::Path::new(dir);
        if !dir_path.exists() {
            tracing::warn!(plugin_dir = %dir, "plugin directory does not exist");
            return Vec::new();
        }

        let mut manifests = Vec::new();

        if let Ok(entries) = std::fs::read_dir(dir_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |e| e == "toml") {
                    let path_str = path.to_string_lossy().to_string();
                    match Self::load(&path_str) {
                        Ok(manifest) => {
                            let name = manifest.plugin.name.clone();
                            tracing::info!(plugin = %name, path = %path_str, "discovered plugin");
                            manifests.push((name, manifest));
                        }
                        Err(e) => {
                            tracing::warn!(path = %path_str, error = %e, "failed to load plugin manifest");
                        }
                    }
                }
            }
        }

        manifests
    }
}
