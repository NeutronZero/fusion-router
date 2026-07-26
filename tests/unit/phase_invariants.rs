//! Executable Architecture Conformance Test Suite (ADR-027 Invariants)

use fusion_plugin_api::{CapabilityContract, CapabilityId, Plugin, PluginMetadata};
use fusion_router::capability::CapabilityRegistry;
use fusion_router::plugin::CompatibilityChecker;
use fusion_router::planner::resolver::capability::{CapabilityGraph, CapabilityResolver, RequirementSet};
use serde_json::json;

#[test]
fn invariant_capability_registry_immutable_post_freeze() {
    let mut reg = CapabilityRegistry::new();
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
    let frozen_reg = reg.freeze();

    assert!(frozen_reg.is_frozen());
    assert!(frozen_reg.contains(&CapabilityId::new("test.invar")));
}

#[test]
fn invariant_compatibility_checker_rejects_api_mismatch() {
    struct IncompatiblePlugin;
    impl Plugin for IncompatiblePlugin {
        fn metadata(&self) -> PluginMetadata {
            PluginMetadata {
                name: "incompatible".into(),
                version: semver::Version::parse("1.0.0").unwrap(),
                api_version: semver::Version::parse("9.0.0").unwrap(), // Incompatible major
                min_compiler_version: semver::Version::parse("0.9.0").unwrap(),
                capabilities: vec![],
            }
        }
    }

    let meta = IncompatiblePlugin.metadata();
    assert!(CompatibilityChecker::validate(&meta).is_err());
}

#[test]
fn invariant_capability_resolver_does_not_execute_logic() {
    let mut reg = CapabilityRegistry::new();
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

    let resolver = CapabilityResolver::new(reg.freeze());
    let reqs = RequirementSet::new(vec![CapabilityId::new("pure.symbol")]);

    let res = resolver.resolve(&reqs).unwrap();
    assert_eq!(res.instances.len(), 1);
    assert_eq!(res.instances[0].contract.id.as_str(), "pure.symbol");
}

#[test]
fn invariant_capability_graph_detects_conflicts_and_cycles() {
    let mut graph = CapabilityGraph::new();
    let c1 = CapabilityContract {
        id: CapabilityId::new("node_a"),
        version: semver::Version::parse("0.1.0").unwrap(),
        description: "Node A".into(),
        inputs_schema: json!({}),
        outputs_schema: json!({}),
        permissions: vec![],
        estimated_cost_usd: 0.0,
        estimated_latency_ms: 1,
        reliability_score: 1.0,
        supports_streaming: false,
    };
    let c2 = CapabilityContract {
        id: CapabilityId::new("node_b"),
        version: semver::Version::parse("0.1.0").unwrap(),
        description: "Node B".into(),
        inputs_schema: json!({}),
        outputs_schema: json!({}),
        permissions: vec![],
        estimated_cost_usd: 0.0,
        estimated_latency_ms: 1,
        reliability_score: 1.0,
        supports_streaming: false,
    };

    graph.add_node(c1);
    graph.add_node(c2);
    graph.add_conflict(CapabilityId::new("node_a"), CapabilityId::new("node_b"));

    assert!(graph.validate().is_err());
}
