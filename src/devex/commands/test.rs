use std::path::Path;
use std::process::Command;

pub fn execute_test(project_dir: &Path) -> Result<(), String> {
    let manifest_path = project_dir.join("Cargo.toml");
    if !manifest_path.exists() {
        return Err("No Cargo.toml found in project directory".into());
    }

    let status = Command::new("cargo")
        .args(["test", "--offline"])
        .current_dir(project_dir)
        .status()
        .map_err(|e| format!("Failed to run cargo test: {e}"))?;

    if !status.success() {
        return Err("cargo test: some tests failed".into());
    }

    println!("All capability tests passed.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::devex::testing::mock_host::MockHostServices;
    use crate::devex::testing::native_runtime::NativeSandboxRuntime;
    use fusion_plugin_api::{CapabilityContract, CapabilityId, CapabilityPlugin};

    struct TestPlugin;

    impl fusion_plugin_api::Plugin for TestPlugin {
        fn metadata(&self) -> fusion_plugin_api::PluginMetadata {
            fusion_plugin_api::PluginMetadata {
                name: "test".into(),
                version: semver::Version::parse("0.1.0").unwrap(),
                api_version: semver::Version::parse("0.2.0").unwrap(),
                min_compiler_version: semver::Version::parse("0.11.0").unwrap(),
                capabilities: vec![CapabilityId::new("test.echo")],
            }
        }
    }

    impl CapabilityPlugin for TestPlugin {
        fn capabilities(&self) -> Vec<CapabilityContract> {
            vec![CapabilityContract {
                id: CapabilityId::new("test.echo"),
                version: semver::Version::parse("0.1.0").unwrap(),
                description: "Test echo".into(),
                inputs_schema: serde_json::json!({}),
                outputs_schema: serde_json::json!({}),
                permissions: vec![],
                dependencies: vec![],
                estimated_cost_usd: 0.0,
                estimated_latency_ms: 0,
                reliability_score: 1.0,
                supports_streaming: false,
                traits: vec![],
            }]
        }
    }

    #[test]
    fn test_capability_test_cycle() {
        let mut rt = NativeSandboxRuntime::new();
        let _host = MockHostServices::default();

        let id = CapabilityId::new("test.echo");
        rt.register(id.clone(), Box::new(TestPlugin));

        assert!(rt.contains(&id));
        let contracts = rt.contracts(&id);
        assert_eq!(contracts.len(), 1);
        assert_eq!(contracts[0].id, id);
        assert_eq!(contracts[0].description, "Test echo");
    }
}
