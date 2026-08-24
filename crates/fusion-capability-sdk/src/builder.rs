use fusion_core::NanoUSD;
use fusion_plugin_api::{CapabilityContract, CapabilityId, CapabilityTrait, Permission};
use semver::Version;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct CapabilityBuilder {
    id: String,
    version: Option<Version>,
    description: Option<String>,
    inputs_schema: Option<Value>,
    outputs_schema: Option<Value>,
    permissions: Vec<Permission>,
    dependencies: Vec<CapabilityId>,
    estimated_cost: NanoUSD,
    estimated_latency_ms: u64,
    reliability_score: f32,
    supports_streaming: bool,
    traits: Vec<CapabilityTrait>,
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
            dependencies: Vec::new(),
            estimated_cost: NanoUSD::ZERO,
            estimated_latency_ms: 0,
            reliability_score: 1.0,
            supports_streaming: false,
            traits: Vec::new(),
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

    pub fn permission(mut self, permission: Permission) -> Self {
        self.permissions.push(permission);
        self
    }

    pub fn estimated_cost(mut self, cost: NanoUSD) -> Self {
        self.estimated_cost = cost;
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

    pub fn trait_(mut self, trait_: CapabilityTrait) -> Self {
        self.traits.push(trait_);
        self
    }

    pub fn finish(self) -> CapabilityContract {
        CapabilityContract {
            id: CapabilityId::new(self.id),
            version: self.version.unwrap_or_else(|| Version::new(0, 1, 0)),
            description: self.description.unwrap_or_default(),
            inputs_schema: self
                .inputs_schema
                .unwrap_or(Value::Object(Default::default())),
            outputs_schema: self
                .outputs_schema
                .unwrap_or(Value::Object(Default::default())),
            permissions: self.permissions,
            dependencies: self.dependencies,
            estimated_cost: self.estimated_cost,
            estimated_latency_ms: self.estimated_latency_ms,
            reliability_score: self.reliability_score,
            supports_streaming: self.supports_streaming,
            traits: self.traits,
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
            .permission(Permission::Network)
            .estimated_cost(NanoUSD::from_nanos(10_000_000))
            .estimated_latency_ms(50)
            .reliability_score(0.99)
            .supports_streaming(true)
            .finish();
        assert_eq!(contract.description, "A full test capability");
        assert_eq!(contract.permissions, vec![Permission::Network]);
        assert_eq!(contract.estimated_cost, NanoUSD::from_nanos(10_000_000));
        assert!(contract.supports_streaming);
    }

    #[test]
    fn builds_with_typed_permissions() {
        let contract = CapabilityBuilder::new("test.typed")
            .version("0.1.0")
            .permission(Permission::Network)
            .permission(Permission::Http("https://api.example.com".into()))
            .finish();
        assert_eq!(contract.permissions.len(), 2);
        assert_eq!(contract.permissions[0], Permission::Network);
    }

    #[test]
    fn contract_is_immutable_after_finish() {
        let contract = CapabilityBuilder::new("test.immutable")
            .version("0.1.0")
            .finish();
        let _: CapabilityContract = contract;
    }

    #[test]
    #[should_panic(expected = "invalid semver version")]
    fn invalid_version_panics() {
        CapabilityBuilder::new("bad.version").version("not-a-version");
    }
}
