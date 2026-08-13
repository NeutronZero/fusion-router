#![allow(dead_code)] // Intentional: binary crate exposes no public API; pub items in module tree are stubs for future production wiring (CircuitBreakingProvider, WorkflowPlanner, DynamicPlanner)
use std::path::PathBuf;
use std::sync::Arc;

use axum::{routing::get, routing::post, Router};
use tower_http::trace::TraceLayer;

mod server;
mod context;
mod requirements;
mod planner;
mod ir;
mod compiler;
mod scheduler;
mod executor;
mod strategies;
mod providers;
mod models;
mod transport;
mod resource;
mod telemetry;
mod types;
mod config;
mod plugin;
mod capability;
mod policy;
mod session;
mod lifecycle;
mod trigger;
mod connectors;
mod workflow;
mod tools;
mod security;
mod cache;
mod middleware;
mod release;
mod feature_gate;
mod events;

#[cfg(feature = "wasm-plugins")]
mod wasm;
mod operations;
mod review;

use config::AppConfig;
use providers::factory;
use providers::registry::ProviderRegistry;
use scheduler::connector_resolver::ConnectorResolver;
use scheduler::connector_subscriber::ConnectorSubscriber;
use telemetry::SqliteEvidenceRepository;

#[tokio::main]
async fn main() {
    let _ = dotenv::dotenv();

    let unsafe_dev = std::env::args().any(|a| a == "--unsafe-dev");
    if unsafe_dev {
        tracing::warn!(
            "==================================================================\n\
             == WARNING: running with --unsafe-dev                          ==\n\
             == Authentication, rate limiting, CORS, and tool defaults are  ==\n\
             == NOT enforced. DO NOT expose this server to untrusted        ==\n\
             == networks.                                                   ==\n\
             =================================================================="
        );
    }

    // Offline mode: `fusion-router review [args]` runs a multi-model code
    // review entirely in-process (no HTTP server, no lifecycle binding).
    if std::env::args().nth(1).as_deref() == Some("review") {
        let env_filter = tracing_subscriber::EnvFilter::default()
            .add_directive("info".parse().unwrap_or_default());
        tracing_subscriber::fmt().with_env_filter(env_filter).init();
        let args = review::ReviewArgs::from_args();
        if let Err(e) = review::run(args).await {
            eprintln!("review failed: {e:?}");
            std::process::exit(1);
        }
        return;
    }

    telemetry::tracing::init_console();
    let _ = telemetry::tracing::init_tracing();

    let config_path = std::env::var("FUSION_CONFIG")
        .unwrap_or_else(|_| "config/default.yaml".to_string());

    let mut config = AppConfig::load(&config_path)
        .unwrap_or_else(|e| {
            eprintln!("failed to load config from {config_path}: {e}, trying default.yaml");
            AppConfig::load("config/default.yaml").unwrap_or_else(|e2| {
                eprintln!("failed to load config from config/default.yaml: {e2}");
                std::process::exit(1);
            })
        });

    if config.unsafe_dev && !unsafe_dev {
        eprintln!(
            "config sets `unsafe_dev: true`, but the flag is only honored from\n\
             the command line. Start with `--unsafe-dev` if an insecure run\n\
             is intentionally required."
        );
        std::process::exit(1);
    }

    if unsafe_dev {
        config.unsafe_dev = true;
    }

    if let Err(errors) = config.validate() {
        for err in &errors {
            eprintln!("config validation error: {err}");
        }
        eprintln!("configuration validation failed with {} error(s)", errors.len());
        std::process::exit(1);
    }

    let log_level = &config.logging.level;
    let log_format = &config.logging.format;

    let env_filter = tracing_subscriber::EnvFilter::default()
        .add_directive(log_level.parse().unwrap_or_else(|_| "info".parse().unwrap_or_default()));

    if log_format == "json" {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(env_filter)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .init();
    }

    tracing::info!("loaded config from {}", config_path);

    let default_provider_name = config.providers.keys().next().cloned().unwrap_or_else(|| "default".to_string());
    let default_cfg = config.providers.get(&default_provider_name).cloned().unwrap_or_default();
    let default_key = factory::resolve_api_key(&default_cfg, &default_provider_name, unsafe_dev)
        .unwrap_or_else(|e| {
            tracing::warn!(error = %e, provider = %default_provider_name, "API key resolution unconfigured");
            String::new()
        });
    let default_target = factory::create_provider_target("default", &default_cfg, default_key);
    let provider_registry = Arc::new(ProviderRegistry::new(default_target));

    for (name, cfg) in &config.providers {
        let api_key = factory::resolve_api_key(cfg, name, unsafe_dev)
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, provider = %name, "API key resolution unconfigured");
                String::new()
            });
        let target = factory::create_provider_target(name, cfg, api_key);
        provider_registry.register_target(vec![name.clone() + "/"], target);
    }

    let configured_providers: Vec<String> = config.providers.keys().cloned().collect();
    tracing::info!("providers configured: {:?}", configured_providers);

    let resource_manager = resource::DefaultResourceManager::new(config.to_quota());

    let evidence_repo = SqliteEvidenceRepository::new("fusion_telemetry.db")
        .unwrap_or_else(|e| {
            tracing::warn!(error = %e, "failed to open telemetry db, using no-op in-memory db");
            SqliteEvidenceRepository::new(":memory:").unwrap_or_else(|e2| {
                eprintln!("failed to initialize in-memory telemetry db: {e2}");
                std::process::exit(1);
            })
        });

    let host = config.server.host.clone();
    let port = config.server.port;
    let cors_config = config.server.cors.clone();
    let auth_config = config.auth.clone();
    let rate_limiting_enabled = config.rate_limiting.enabled;
    let rate_limiting_config = config.rate_limiting.clone();

    let connector_resolver = Arc::new(ConnectorResolver::new());

    let state = server::handlers::AppState::new(
        provider_registry.clone() as Arc<dyn providers::ChatProvider + Send + Sync>,
        resource_manager,
        Arc::new(evidence_repo),
        config.clone(),
        PathBuf::from(&config_path),
        connector_resolver.clone(),
    );

    // Law 5 / ADR-034: the execution plane compiles through the same
    // `build_compiler` pipeline as the chat path, with a budget pass reading
    // the shared resource manager instance (no empty pass list, no bypass).
    let exec_plane_compiler: Arc<dyn crate::compiler::Compiler> = Arc::new(
        crate::compiler::build_compiler(
            config.model_catalog.clone(),
            state.resource_manager.clone(),
            None,
        ),
    );

    state.config_manager.register_subscriber(Box::new(provider_registry.clone()));
    state.config_manager.register_subscriber(Box::new(
        ConnectorSubscriber::new(ConnectorResolver::clone(&connector_resolver)),
    ));

    #[cfg(unix)]
    {
        let cm_for_reload = state.config_manager.clone();
        tokio::spawn(async move {
            reload_signal(cm_for_reload).await;
        });
    }

    // Spawn connector health checker
    let health_checker = Arc::new(scheduler::connector_health::ConnectorHealthChecker::new(60));
    let hc_resolver = state.connector_resolver.clone();
    let hc_checker = health_checker.clone();
    tokio::spawn(async move {
        hc_checker.run(hc_resolver).await;
    });

    let ops_registry = Arc::new(parking_lot::RwLock::new(crate::capability::InMemoryCapabilityRegistry::new()));
    let ops_cache = Arc::new(crate::operations::RuntimeModuleCache::new());
    let ops_dashboard = Arc::new(crate::operations::dashboard::DefaultDashboardDataProvider::new(
        ops_registry.clone(),
        ops_cache.clone(),
    ));
    let ops_inspector = Arc::new(crate::operations::runtime_inspector::RuntimeInspector::new(ops_cache.clone()));
    let ops_store = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let ops_audit = Arc::new(crate::telemetry::audit::AuditLog::new(1000));
    let ops_policy_admin = Arc::new(crate::operations::policy_admin::PolicyAdmin::new(ops_store, ops_audit.clone()));
    let ops_verifier = Arc::new(crate::operations::MockPackageVerifier);
    let ops_attestation_viewer = Arc::new(crate::operations::attestation_viewer::AttestationViewer::new(ops_verifier, ops_audit));

    let ops_state = crate::operations::handlers::OperationsState {
        dashboard: ops_dashboard,
        inspector: ops_inspector,
        policy_admin: ops_policy_admin,
        attestation_viewer: ops_attestation_viewer,
    };

    let operations_routes = axum::Router::new()
        .route("/v1/operations/registry", axum::routing::get(crate::operations::handlers::registry_handler))
        .route("/v1/operations/runtime", axum::routing::get(crate::operations::handlers::runtime_handler))
        .route("/v1/operations/metrics", axum::routing::get(crate::operations::handlers::metrics_handler))
        .route("/v1/operations/policies", axum::routing::get(crate::operations::handlers::policies_list_handler))
        .route("/v1/operations/policies", axum::routing::post(crate::operations::handlers::policies_create_handler))
        .route("/v1/operations/attestations", axum::routing::get(crate::operations::handlers::attestations_handler))
        .with_state(ops_state);

    let event_bus = Arc::new(crate::events::BroadcastEventBus::new(1024));
    let exec_plane = crate::server::execution::build_execution_plane(
        event_bus,
        state.executor.clone(),
        exec_plane_compiler,
    );
    let execution_routes = axum::Router::new()
        .route("/v1/executions", axum::routing::post(crate::server::execution::execute_workflow_handler))
        .with_state(exec_plane);

    let mut app = Router::new()
        .route("/v1/chat/completions", post(server::handlers::chat_completions))
        .route("/v1/messages", post(server::handlers::anthropic_messages))
        .route("/metrics", get(server::handlers::metrics_handler))
        .route("/health", get(server::health::health_handler))
        .route("/ready", get(server::health::ready_handler))
        .with_state(state.clone())
        .merge(operations_routes)
        .merge(execution_routes)
        .layer(TraceLayer::new_for_http())
        .layer(axum::middleware::from_fn(middleware::request_id::request_id_middleware));

    // ADR-035: the rate limiter sits INSIDE the auth layer so it keys on the
    // authenticated identity (set via ClientIdentity) rather than spoofable
    // headers; unauthenticated traffic falls back to the TCP peer address.
    if rate_limiting_enabled {
        let limiter = middleware::rate_limit::RateLimiter::new(rate_limiting_config);
        limiter.start_cleanup();
        app = app
            .layer(axum::middleware::from_fn(middleware::rate_limit::rate_limit_middleware))
            .layer(axum::Extension(limiter));
    }

    let app = app
        .layer(axum::middleware::from_fn(middleware::auth::auth_middleware))
        .layer(axum::Extension(auth_config))
        .layer(crate::middleware::cors::cors_layer_from_config(&cors_config));

    let addr = match format!("{}:{}", host, port).parse::<std::net::SocketAddr>() {
        Ok(addr) => addr,
        Err(e) => {
            eprintln!("invalid socket address '{}:{}': {}", host, port, e);
            std::process::exit(1);
        }
    };
    tracing::info!("FusionRouter listening on {}", addr);

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("failed to bind listener on {}: {}", addr, e);
            std::process::exit(1);
        }
    };

    let shutdown_timeout = std::time::Duration::from_secs(config.server.shutdown_timeout_secs);

    let serve = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal());

    match tokio::time::timeout(shutdown_timeout, serve).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            eprintln!("server error: {e}");
            std::process::exit(1);
        }
        Err(_) => {
            tracing::warn!(
                timeout_secs = %config.server.shutdown_timeout_secs,
                "graceful shutdown exceeded its bound; forcing exit"
            );
        }
    }
}

#[cfg(unix)]
async fn reload_signal(config_manager: Arc<config::manager::ConfigManager>) {
    use tokio::signal::unix;
    if let Ok(mut stream) = unix::signal(unix::SignalKind::hangup()) {
        while stream.recv().await.is_some() {
            match config_manager.reload().await {
                Ok(gen) => tracing::info!(generation = gen, "configuration reloaded"),
                Err(e) => tracing::error!(error = %e, "reload failed, continuing with previous config"),
            }
        }
    } else {
        tracing::warn!("failed to install SIGHUP handler");
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::warn!("failed to install Ctrl+C handler: {}", e);
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            sig.recv().await;
        } else {
            tracing::warn!("failed to install SIGTERM handler");
            std::future::pending::<()>().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("shutdown signal received, gracefully shutting down");
}



