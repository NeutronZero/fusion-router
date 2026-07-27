use std::path::PathBuf;

use crate::feature_gate::{FeatureDefinition, FeatureFlag, FeatureRegistry, Stability};
use crate::release::gates::semver::SemVerGate;
use crate::release::runner::GateRunner;

pub fn bootstrap(workspace_root: PathBuf, baseline_ref: &str) -> (GateRunner, FeatureRegistry) {
    let mut runner = GateRunner::new();

    let gate = SemVerGate::new(baseline_ref, workspace_root);
    runner.register(Box::new(gate));

    let registry = FeatureRegistry::new(FEATURE_DEFINITIONS);

    (runner, registry)
}

const FEATURE_DEFINITIONS: &[FeatureDefinition] = &[
    FeatureDefinition {
        id: FeatureFlag::Streaming,
        introduced: "0.1.0",
        removed: None,
        stability: Stability::Stable,
        default_enabled: true,
        description: "Enable streaming responses",
    },
    FeatureDefinition {
        id: FeatureFlag::Replay,
        introduced: "0.5.0",
        removed: None,
        stability: Stability::Experimental,
        default_enabled: false,
        description: "Enable request replay",
    },
    FeatureDefinition {
        id: FeatureFlag::ConnectorHealth,
        introduced: "0.8.0",
        removed: None,
        stability: Stability::Stable,
        default_enabled: true,
        description: "Enable connector health checks",
    },
    FeatureDefinition {
        id: FeatureFlag::SemanticCache,
        introduced: "0.9.0",
        removed: None,
        stability: Stability::Experimental,
        default_enabled: false,
        description: "Enable semantic caching",
    },
    FeatureDefinition {
        id: FeatureFlag::WasmPlugins,
        introduced: "0.10.0",
        removed: None,
        stability: Stability::Deprecated,
        default_enabled: false,
        description: "Enable WASM plugin support",
    },
];
