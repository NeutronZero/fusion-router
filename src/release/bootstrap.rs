use std::path::PathBuf;

use crate::feature_gate::{FeatureDefinition, FeatureFlag, FeatureRegistry, Stability};
use crate::release::gates::connector::{ConnectorGate, ConnectorGateConfig, FilesystemConnectorBackend};
use crate::release::gates::determinism::{DeterminismGate, DeterminismGateConfig, RealDeterminismBackend};
use crate::release::gates::plugin::{FilesystemPluginBackend, PluginGate, PluginGateConfig};
use crate::release::gates::provider::{FilesystemProviderBackend, ProviderGate, ProviderGateConfig};
use crate::release::gates::replay::{FilesystemReplayBackend, ReplayGate, ReplayGateConfig};
use crate::release::gates::semver::SemVerGate;
use crate::release::gates::strategy::{FilesystemStrategyBackend, StrategyGate, StrategyGateConfig};
use crate::release::gates::upgrade::{FilesystemUpgradeBackend, UpgradeGate, UpgradeGateConfig};
use crate::release::runner::GateRunner;

pub fn build_default_runner(workspace_root: PathBuf, baseline_ref: &str) -> GateRunner {
    let mut runner = GateRunner::new();

    let semver_gate = SemVerGate::new(baseline_ref, workspace_root.clone());
    let replay_gate = ReplayGate::new(
        Box::new(FilesystemReplayBackend::new(workspace_root.clone())),
        ReplayGateConfig { fixture_root: workspace_root.clone() },
    );
    let upgrade_gate = UpgradeGate::new(
        Box::new(FilesystemUpgradeBackend::new(workspace_root.clone())),
        UpgradeGateConfig { fixture_root: workspace_root.clone() },
    );
    let determinism_gate = DeterminismGate::new(
        Box::new(RealDeterminismBackend),
        DeterminismGateConfig { fixture_root: workspace_root.clone() },
    );

    let plugin_gate = PluginGate::new(
        Box::new(FilesystemPluginBackend::new(workspace_root.clone())),
        PluginGateConfig { fixture_root: workspace_root.clone() },
    );
    let strategy_gate = StrategyGate::new(
        Box::new(FilesystemStrategyBackend::new(workspace_root.clone())),
        StrategyGateConfig { fixture_root: workspace_root.clone() },
    );
    let provider_gate = ProviderGate::new(
        Box::new(FilesystemProviderBackend::new(workspace_root.clone())),
        ProviderGateConfig { fixture_root: workspace_root.clone() },
    );
    let connector_gate = ConnectorGate::new(
        Box::new(FilesystemConnectorBackend::new(workspace_root.clone())),
        ConnectorGateConfig { fixture_root: workspace_root.clone() },
    );

    runner.register(Box::new(semver_gate));
    runner.register(Box::new(replay_gate));
    runner.register(Box::new(upgrade_gate));
    runner.register(Box::new(determinism_gate));
    runner.register(Box::new(plugin_gate));
    runner.register(Box::new(strategy_gate));
    runner.register(Box::new(provider_gate));
    runner.register(Box::new(connector_gate));

    runner
}

pub fn bootstrap(workspace_root: PathBuf, baseline_ref: &str) -> (GateRunner, FeatureRegistry) {
    let runner = build_default_runner(workspace_root, baseline_ref);
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
