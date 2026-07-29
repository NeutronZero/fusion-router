use fusion_plugin_api::CapabilityContract;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct CapabilityManifest {
    pub abi_version: String,
    pub capability_id: String,
    pub capability_version: String,
    pub description: String,
}

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
