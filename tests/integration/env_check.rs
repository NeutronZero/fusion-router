#[test]
fn test_env_file_has_required_keys() {
    let _ = dotenv::dotenv();

    let zen_key = std::env::var("OPENCODEZEN_API_KEY");
    assert!(zen_key.is_ok(), "OPENCODEZEN_API_KEY must be set in .env");
    assert!(!zen_key.unwrap().is_empty(), "OPENCODEZEN_API_KEY must not be empty");

    let or_key = std::env::var("OPENROUTER_API_KEY");
    assert!(or_key.is_ok(), "OPENROUTER_API_KEY must be set in .env");
    assert!(!or_key.unwrap().is_empty(), "OPENROUTER_API_KEY must not be empty");
}
