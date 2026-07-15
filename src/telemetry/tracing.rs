use tracing_subscriber::EnvFilter;

pub fn setup_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("fusion_router=info"));

    let use_json = std::env::var("FUSION_TRACING_FORMAT")
        .map(|v| v == "json")
        .unwrap_or(false);

    if use_json {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(filter)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .init();
    }
}
