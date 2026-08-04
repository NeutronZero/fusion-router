use fusion_core::{ProviderId, ProviderLifecycleState};
use fusion_infrastructure::{Database, ProviderRecord, ProviderRegistry, ProviderRepository};
use chrono::Utc;

#[tokio::test]
async fn test_beta_provider_setup_and_lifecycle_journey() {
    let db = Database::memory().expect("Init DB");
    let provider_repo = ProviderRepository::new(db);
    let registry = ProviderRegistry::new();

    // 1. Configure OpenRouter Provider
    let rec = ProviderRecord {
        provider_id: ProviderId("openrouter".to_string()),
        name: "OpenRouter Primary".to_string(),
        api_key_encrypted: "enc_key_abcdef".to_string(),
        base_url: Some("https://openrouter.ai/api/v1".to_string()),
        enabled: true,
        updated_at: Utc::now().to_rfc3339(),
    };

    provider_repo.save(&rec).expect("Persist to DB");
    registry.register(rec);

    // 2. Verify Initial Health & Lifecycle State
    let health = registry.get_health("openrouter").expect("Fetch health status");
    assert_eq!(health.state, ProviderLifecycleState::Healthy);
    assert!(health.capabilities.contains(&"Streaming".to_string()));
    assert!(health.capabilities.contains(&"Vision".to_string()));
    assert_eq!(health.model_count, 14);

    // 3. Hot-Reload Toggle: Disable Provider without Restart
    registry.set_enabled("openrouter", false).expect("Hot reload disable");
    let disabled_health = registry.get_health("openrouter").expect("Fetch health status after toggle");
    assert_eq!(disabled_health.state, ProviderLifecycleState::Unavailable);

    // 4. Hot-Reload Toggle: Re-Enable Provider
    registry.set_enabled("openrouter", true).expect("Hot reload enable");
    let reenabled_health = registry.get_health("openrouter").expect("Fetch health status after re-enable");
    assert_eq!(reenabled_health.state, ProviderLifecycleState::Healthy);
}
