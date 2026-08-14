use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use fusion_router::config::manager::ConfigManager;
use fusion_router::config::error::ReloadError;
use fusion_router::config::{
    AppConfig, AuthConfig, CorsConfig, LoggingConfig, RateLimitingConfig,
    ResourceConfig, ServerConfig, StrategyConfig, ToolsConfig,
};
use fusion_router::providers::circuit_breaker::CircuitBreaker;
use fusion_router::providers::registry::ProviderRegistry;
use fusion_router::providers::router::ProviderTarget;
use fusion_router::providers::{ChatProvider, ModelRequirements};
use fusion_router::scheduler::connector_resolver::ConnectorResolver;
use fusion_router::scheduler::connector_subscriber::ConnectorSubscriber;
use fusion_router::types::ModelCatalog;
use fusion_router::types::{ChatCompletionRequest, ChatCompletionResponse, ChatMessage, Choice};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

struct DummyProvider;

#[async_trait]
impl ChatProvider for DummyProvider {
    fn name(&self) -> &str {
        "dummy"
    }
    async fn chat_completion(
        &self,
        _req: &ChatCompletionRequest,
    ) -> anyhow::Result<ChatCompletionResponse> {
        Ok(ChatCompletionResponse {
            id: "dummy".into(),
            object: "chat.completion".into(),
            created: 0,
            model: "dummy".into(),
            choices: vec![Choice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".into(),
                    content: "dummy".into(),
                },
                finish_reason: "stop".into(),
            }],
            native_tool_calls: None,
            usage: None,
        })
    }
}

fn dummy_target(name: &str) -> ProviderTarget {
    ProviderTarget::new(
        name.into(),
        CircuitBreaker::new(3, 2, 5),
        Box::new(|| Arc::new(DummyProvider)),
    )
}

fn empty_config() -> AppConfig {
    AppConfig {
        unsafe_dev: false,
        server: ServerConfig {
            host: "0.0.0.0".into(),
            port: 8080,
            shutdown_timeout_secs: 30,
            cors: CorsConfig::default(),
        },
        resources: ResourceConfig {
            max_daily_cost: fusion_router::types::NanoUSD::from_nanos(100_000_000_000),
            max_daily_tokens: 1_000_000,
            max_concurrent: 5,
            max_concurrent_nodes: 16,
            provider_limits: HashMap::new(),
        },
        providers: HashMap::new(),
        policies: vec![],
        strategies: StrategyConfig::default(),
        tools: ToolsConfig::default(),
        auth: AuthConfig::default(),
        rate_limiting: RateLimitingConfig::default(),
        logging: LoggingConfig::default(),
        model_catalog: ModelCatalog::default(),
        connectors: HashMap::new(),
        features: HashMap::new(),
    }
}

/// Build a YAML config string with both providers and connectors.
fn config_yaml_connectors(provider_block: &str, connector_block: &str) -> String {
    format!(
        r#"server:
  host: "0.0.0.0"
  port: 8080
  shutdown_timeout_secs: 30
resources:
  max_daily_cost: 100.0
  max_daily_tokens: 1000000
  max_concurrent: 5
  max_concurrent_nodes: 16
auth:
  enabled: false
  api_keys: []
providers:
{}
connectors:
{}"#,
        provider_block, connector_block
    )
}

/// Build a YAML config string for reload.  `provider_block` goes directly
/// under the `providers:` key.
fn config_yaml(provider_block: &str) -> String {
    format!(
        r#"server:
  host: "0.0.0.0"
  port: 8080
  shutdown_timeout_secs: 30
resources:
  max_daily_cost: 100.0
  max_daily_tokens: 1000000
  max_concurrent: 5
  max_concurrent_nodes: 16
auth:
  enabled: false
  api_keys: []
providers:
{}"#,
        provider_block
    )
}

fn write_temp_config(yaml: &str) -> (PathBuf, TempGuard) {
    let guard = TempGuard::new();
    let path = guard.path.join("config.yaml");
    std::fs::write(&path, yaml).expect("write temp config");
    (path, guard)
}

struct TempGuard {
    path: PathBuf,
}

impl TempGuard {
    fn new() -> Self {
        // Windows SystemTime granularity (~100 ns) means parallel tests in
        // the same process can observe identical timestamps; pair the clock
        // with a process-wide monotonic counter so temp dirs never collide
        // (otherwise tests overwrite each other's config files).
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "fr_cfg_test_{}_{}_{}",
            std::process::id(),
            ts,
            seq
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self { path: dir }
    }
}

impl Drop for TempGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

// ---------------------------------------------------------------------------
// Test 1 — full two-phase reload completes successfully
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_provider_registry_completes_two_phase_reload() {
    std::env::set_var("FR_TEST_OR_KEY", "sk-or-test-001");
    std::env::set_var("FR_TEST_ZEN_KEY", "sk-zen-test-001");

    let yaml = config_yaml(
        "  openrouter:\n    api_key_env: FR_TEST_OR_KEY\n  zen:\n    api_key_env: FR_TEST_ZEN_KEY\n",
    );
    let (config_path, _guard) = write_temp_config(&yaml);

    let registry = Arc::new(ProviderRegistry::new(dummy_target("default")));
    let manager = ConfigManager::new(config_path, empty_config(), vec![Box::new(registry.clone())]);

    let gen = manager.reload().await.expect("reload should succeed");
    assert_eq!(gen, 2);

    let snap = manager.snapshot();
    assert_eq!(snap.generation, 2);
    assert!(snap.config.providers.contains_key("openrouter"));
    assert!(snap.config.providers.contains_key("zen"));

    let selected = registry.select_targets(&ModelRequirements::default());
    assert_eq!(selected.len(), 2);
    assert!(selected.iter().any(|t| t.name == "openrouter"));
    assert!(selected.iter().any(|t| t.name == "zen"));

    std::env::remove_var("FR_TEST_OR_KEY");
    std::env::remove_var("FR_TEST_ZEN_KEY");
}

// ---------------------------------------------------------------------------
// Test 2 — bad prepare is rejected; old state is preserved
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_provider_registry_rejects_bad_prepare() {
    std::env::set_var("FR_TEST_VALID_KEY", "sk-valid-002");

    let registry = Arc::new(ProviderRegistry::new(dummy_target("default")));
    registry.register_target(vec!["survivor/".into()], dummy_target("survivor-model"));

    let yaml = config_yaml(
        "  valid:\n    api_key_env: FR_TEST_VALID_KEY\n  broken:\n    api_key_env: FR_DOES_NOT_EXIST_002\n",
    );
    let (config_path, _guard) = write_temp_config(&yaml);

    let manager = ConfigManager::new(config_path, empty_config(), vec![Box::new(registry.clone())]);

    let err = manager.reload().await.unwrap_err();
    match &err {
        ReloadError::Subscriber { name, .. } => {
            assert_eq!(name, "ProviderRegistry");
        }
        other => panic!("expected ReloadError::Subscriber, got {other:?}"),
    }

    // Old prefix-based state is untouched because the snapshot was never
    // swapped.
    let survivors = registry.get_matching_targets("survivor/model");
    assert_eq!(survivors.len(), 1);
    assert_eq!(survivors[0].name, "survivor-model");

    std::env::remove_var("FR_TEST_VALID_KEY");
}

// ---------------------------------------------------------------------------
// Test 3 — get_matching_targets routes by prefix
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_chat_provider_delegates_to_correct_target() {
    let registry = ProviderRegistry::new(dummy_target("fallback"));

    registry.register_target(vec!["zen/".into()], dummy_target("zen-target"));
    registry.register_target(vec!["openrouter/".into()], dummy_target("or-target"));

    let matched = registry.get_matching_targets("zen/some-model");
    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0].name, "zen-target");

    let matched = registry.get_matching_targets("openrouter/gpt-4o");
    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0].name, "or-target");

    let matched = registry.get_matching_targets("unknown/model");
    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0].name, "fallback");
}

// ---------------------------------------------------------------------------
// Test 4 — generation counter increments on each reload
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_config_generation_increments_on_each_reload() {
    std::env::set_var("FR_TEST_GEN_KEY", "sk-gen-004");

    let yaml = config_yaml("  p:\n    api_key_env: FR_TEST_GEN_KEY\n");
    let (config_path, _guard) = write_temp_config(&yaml);

    let registry = Arc::new(ProviderRegistry::new(dummy_target("default")));
    let manager = ConfigManager::new(config_path, empty_config(), vec![Box::new(registry.clone())]);

    assert_eq!(manager.snapshot().generation, 1);

    let g2 = manager.reload().await.expect("first reload");
    assert_eq!(g2, 2);
    assert_eq!(manager.snapshot().generation, 2);

    let g3 = manager.reload().await.expect("second reload");
    assert_eq!(g3, 3);
    assert_eq!(manager.snapshot().generation, 3);

    std::env::remove_var("FR_TEST_GEN_KEY");
}

// ---------------------------------------------------------------------------
// Test 5 — commit swaps targets in place
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_subscriber_commit_updates_targets() {
    std::env::set_var("FR_TEST_COMMIT_KEY", "sk-commit-005");

    let registry = Arc::new(ProviderRegistry::new(dummy_target("default")));

    registry.register_target(vec!["old/".into()], dummy_target("old-target"));
    assert_eq!(
        registry.get_matching_targets("old/model")[0].name,
        "old-target",
        "pre-seeded target visible before reload"
    );

    let yaml = config_yaml("  p:\n    api_key_env: FR_TEST_COMMIT_KEY\n");
    let (config_path, _guard) = write_temp_config(&yaml);

    let manager = ConfigManager::new(config_path, empty_config(), vec![Box::new(registry.clone())]);

    manager.reload().await.expect("reload should succeed");

    let selected = registry.select_targets(&ModelRequirements::default());
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].name, "p");

    std::env::remove_var("FR_TEST_COMMIT_KEY");
}

// ---------------------------------------------------------------------------
// Test 6 — connector added via config reload
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_connector_add_via_reload() {
    let resolver = ConnectorResolver::new();
    let subscriber = Box::new(ConnectorSubscriber::new(resolver.clone()));

    let registry = Arc::new(ProviderRegistry::new(dummy_target("default")));
    let yaml = config_yaml_connectors(
        "",
        "  my-http:\n    connector_type: http\n",
    );
    let (config_path, _guard) = write_temp_config(&yaml);

    let manager = ConfigManager::new(
        config_path,
        empty_config(),
        vec![Box::new(registry), subscriber],
    );

    manager.reload().await.expect("reload should succeed");

    let names = resolver.connector_names();
    assert_eq!(names.len(), 1);
    assert_eq!(names[0], "http");
}

// ---------------------------------------------------------------------------
// Test 7 — connector removed via config reload
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_connector_remove_via_reload() {
    let resolver = ConnectorResolver::new();
    let subscriber = Box::new(ConnectorSubscriber::new(resolver.clone()));

    // Pre-register a connector so it exists before reload
    let http = Arc::new(fusion_router::connectors::http::HttpConnector::new());
    let _ = resolver.register_connector(http);

    let registry = Arc::new(ProviderRegistry::new(dummy_target("default")));
    // Empty connectors in the new config
    let yaml = config_yaml_connectors("", "");
    let (config_path, _guard) = write_temp_config(&yaml);

    let manager = ConfigManager::new(
        config_path,
        empty_config(),
        vec![Box::new(registry), subscriber],
    );

    manager.reload().await.expect("reload should succeed");

    let names = resolver.connector_names();
    assert!(names.is_empty(), "all connectors should be removed");
}

// ---------------------------------------------------------------------------
// Test 8 — invalid connector type in config is rejected
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_connector_invalid_type_rejected() {
    let resolver = ConnectorResolver::new();
    let subscriber = Box::new(ConnectorSubscriber::new(resolver.clone()));

    let registry = Arc::new(ProviderRegistry::new(dummy_target("default")));
    let yaml = config_yaml_connectors(
        "",
        "  bad:\n    connector_type: nonexistent\n",
    );
    let (config_path, _guard) = write_temp_config(&yaml);

    let manager = ConfigManager::new(
        config_path,
        empty_config(),
        vec![Box::new(registry), subscriber],
    );

    let err = manager.reload().await.unwrap_err();
    match &err {
        ReloadError::ConnectorError(msg) => {
            assert!(msg.contains("nonexistent"), "reason should mention unknown type");
        }
        other => panic!("expected ReloadError::ConnectorError, got {other:?}"),
    }
}
