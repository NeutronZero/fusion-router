//! Integration test verifying the SDK + macros work together
//! through the public prelude API.

use fusion_capability_sdk::prelude::*;
use fusion_plugin_api::Plugin;

struct EchoCapability;

impl Plugin for EchoCapability {
    fn metadata(&self) -> fusion_plugin_api::PluginMetadata {
        fusion_plugin_api::PluginMetadata {
            name: "echo.text".into(),
            version: semver::Version::parse("0.1.0").unwrap(),
            api_version: semver::Version::parse(fusion_plugin_api::CAPABILITY_ABI_VERSION).unwrap(),
            min_compiler_version: semver::Version::parse("0.11.0").unwrap(),
            capabilities: vec![fusion_plugin_api::CapabilityId::new("echo.text")],
        }
    }
}

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
