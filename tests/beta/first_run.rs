use fusion_infrastructure::{Database, LocalDiscoveryProber, ProviderRecord, ProviderRepository};
use fusion_security::SecretManager;
use fusion_core::ProviderId;
use chrono::Utc;

#[tokio::test]
async fn test_beta_first_run_user_journey() {
    // 1. Initialize DB & Secret Manager
    let db = Database::memory().expect("Initialize DB");
    let key = SecretManager::generate_random_key();
    let secret_manager = SecretManager::new(key);

    // 2. Perform Local Model Server Discovery
    let prober = LocalDiscoveryProber::new();
    let discovered_servers = prober.probe_all().await;
    assert_eq!(discovered_servers.len(), 3);

    // 3. User Enters API Key -> Encrypt & Persist Provider
    let provider_repo = ProviderRepository::new(db);
    let raw_key = "sk-openrouter-secret-key-99999";
    let encrypted_key = secret_manager.encrypt(raw_key).expect("Encrypt API key");

    let record = ProviderRecord {
        provider_id: ProviderId("openrouter".to_string()),
        name: "OpenRouter Primary".to_string(),
        api_key_encrypted: encrypted_key.clone(),
        base_url: Some("https://openrouter.ai/api/v1".to_string()),
        enabled: true,
        updated_at: Utc::now().to_rfc3339(),
    };

    provider_repo.save(&record).expect("Persist provider");

    // 4. Verify Provider Retrieval & Decryption
    let saved_providers = provider_repo.list().expect("List providers");
    assert_eq!(saved_providers.len(), 1);
    assert_eq!(saved_providers[0].name, "OpenRouter Primary");

    let decrypted_key = secret_manager.decrypt(&saved_providers[0].api_key_encrypted).expect("Decrypt key");
    assert_eq!(decrypted_key, raw_key);

    // 5. Verify Redacted Display for UI
    let redacted = secret_manager.redact(&decrypted_key);
    assert_eq!(redacted, "sk-o...9999");
}
