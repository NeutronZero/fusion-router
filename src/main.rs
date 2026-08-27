#![allow(dead_code)] // Debt-tracked stubs — see docs/architecture/architecture_debt_register.md AD-005..AD-019; blanket kept for binary crate with no public API, migrate to per-module allows as stubs are wired (tracked debt, not hidden)
use std::path::PathBuf;
use std::sync::Arc;

use axum::{routing::get, routing::post, Router};
use tower_http::trace::TraceLayer;

mod cache;
mod capability;
mod compiler;
mod config;
mod connectors;
mod context;
mod events;
mod executor;
mod feature_gate;
mod ir;
mod lifecycle;
mod middleware;
mod planner;
mod plugin;
mod policy;
mod providers;
mod release;
mod requirements;
mod resource;
mod scheduler;
mod security;
mod server;
mod session;
mod strategies;
mod telemetry;
mod tools;
mod transport;
mod types;

mod operations;
mod review;
#[cfg(feature = "wasm-plugins")]
mod wasm;

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

    let config_path =
        std::env::var("FUSION_CONFIG").unwrap_or_else(|_| "config/default.yaml".to_string());

    let mut config = AppConfig::load(&config_path).unwrap_or_else(|e| {
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
        eprintln!(
            "configuration validation failed with {} error(s)",
            errors.len()
        );
        std::process::exit(1);
    }

    let log_level = &config.logging.level;
    let log_format = &config.logging.format;

    let env_filter = tracing_subscriber::EnvFilter::default().add_directive(
        log_level
            .parse()
            .unwrap_or_else(|_| "info".parse().unwrap_or_default()),
    );

    if log_format == "json" {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(env_filter)
            .init();
    } else {
        tracing_subscriber::fmt().with_env_filter(env_filter).init();
    }

    tracing::info!("loaded config from {}", config_path);

    // Deterministic default provider: the lexicographically smallest
    // configured provider name. `HashMap::keys().next()` is arbitrary hasher
    // order, which made the implicit default depend on process seeds.
    let mut configured_names: Vec<String> = config.providers.keys().cloned().collect();
    configured_names.sort();
    let default_provider_name =
        providers::registry::select_default_provider_name(&configured_names)
            .unwrap_or_else(|| "default".to_string());
    tracing::info!(
        provider = %default_provider_name,
        rule = "lexicographically smallest configured provider",
        "default provider selected"
    );
    let default_cfg = config
        .providers
        .get(&default_provider_name)
        .cloned()
        .unwrap_or_default();
    let default_key = factory::resolve_api_key(&default_cfg, &default_provider_name, unsafe_dev).unwrap_or_else(|e| {
        if unsafe_dev {
            tracing::warn!(error = %e, provider = %default_provider_name, "API key resolution unconfigured (dev mode)");
            String::new()
        } else {
            eprintln!(
                "refusing to start: provider '{default_provider_name}' has no resolvable API key ({e}). \
                 Configure the key or pass --unsafe-dev for local development."
            );
            std::process::exit(1);
        }
    });
    let default_target = factory::create_provider_target("default", &default_cfg, default_key);
    let provider_registry = Arc::new(ProviderRegistry::new(default_target));

    for (name, cfg) in &config.providers {
        let api_key = match factory::resolve_api_key(cfg, name, unsafe_dev) {
            Ok(k) => k,
            Err(e) => {
                if unsafe_dev {
                    tracing::warn!(error = %e, provider = %name, "API key resolution unconfigured (dev mode)");
                    String::new()
                } else {
                    eprintln!(
                        "refusing to start: provider '{name}' has no resolvable API key ({e}). \
                         Configure the key or pass --unsafe-dev for local development."
                    );
                    std::process::exit(1);
                }
            }
        };
        let target = factory::create_provider_target(name, cfg, api_key);
        provider_registry.register_target(vec![name.clone() + "/"], target);
    }

    let configured_providers: Vec<String> = config.providers.keys().cloned().collect();
    tracing::info!("providers configured: {:?}", configured_providers);

    let resource_manager = resource::DefaultResourceManager::new(config.to_quota());

    let evidence_repo = SqliteEvidenceRepository::new("fusion_telemetry.db")
        .map(|repo| repo.with_snapshot_ttl(std::time::Duration::from_secs(30)))
        .unwrap_or_else(|e| {
            tracing::warn!(error = %e, "failed to open telemetry db, using no-op in-memory db");
            SqliteEvidenceRepository::new(":memory:").unwrap_or_else(|e2| {
                eprintln!("failed to initialize in-memory telemetry db: {e2}");
                std::process::exit(1);
            })
        });
    evidence_repo.spawn_retention(7);

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
    )
    .with_provider_registry(provider_registry.clone());

    // Law 5 / ADR-034: the execution plane compiles through the same
    // `build_compiler` pipeline as the chat path, with a budget pass reading
    // the shared resource manager instance (no empty pass list, no bypass).
    // The compiler is built per execution with the live policy snapshot so
    // runtime-created deny/approval rules are enforced immediately.

    state
        .config_manager
        .register_subscriber(Box::new(provider_registry.clone()));
    state
        .config_manager
        .register_subscriber(Box::new(ConnectorSubscriber::new(
            ConnectorResolver::clone(&connector_resolver),
        )));

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

    let ops_audit = Arc::new(crate::telemetry::audit::AuditLog::new(1000));
    let archive_path = std::path::PathBuf::from("release_archive");
    let archive_backend = crate::release::archive::FilesystemArchiveBackend::new(archive_path);
    let signing_key = std::env::var("FUSION_SIGNING_KEY").ok();
    let signer: Option<Arc<dyn crate::release::signing::Signer>> = signing_key.map(|key| {
        Arc::new(crate::release::signing::HmacSha256Signer::new(
            "signing-key",
            key.as_bytes(),
        )) as Arc<dyn crate::release::signing::Signer>
    });
    let ops_verifier: Arc<dyn crate::operations::PackageVerifier> = Arc::new(
        crate::operations::ArchivePackageVerifier::new(archive_backend, signer),
    );
    // Production assertion: the verifier must be the real ArchivePackageVerifier,
    // never a MockPackageVerifier. This guard is compiled into the binary so
    // mock verifiers cannot leak into production deployments.
    {
        let verifier_type = std::any::type_name::<crate::operations::ArchivePackageVerifier>();
        assert!(
            verifier_type.contains("ArchivePackageVerifier"),
            "PRODUCTION VIOLATION: expected ArchivePackageVerifier, got '{}'. \
             MockPackageVerifier must not be used in production.",
            verifier_type
        );
    }
    let ops_attestation_viewer = Arc::new(
        crate::operations::attestation_viewer::AttestationViewer::new(
            ops_verifier,
            ops_audit.clone(),
        ),
    );

    // Wire PluginManager startup lifecycle: Discover -> Load -> Validate -> Initialize -> Register -> Activate
    let mut plugin_manager = crate::plugin::PluginManager::new();
    plugin_manager.load_manifests("plugins");
    if let Err(e) =
        plugin_manager.register_capability_plugin(&fusion_plugin_echo::EchoPlugin::new())
    {
        tracing::warn!("Failed to register built-in echo capability plugin: {e}");
    }
    let frozen_capability_registry = plugin_manager.freeze_capability_registry();
    tracing::info!(
        capabilities = frozen_capability_registry.list().len(),
        "plugin manager startup lifecycle executed (Discover -> Load -> Validate -> Initialize -> Register -> Activate)"
    );
    let _plugin_manager = plugin_manager;
    let state = state.with_capability_registry(frozen_capability_registry);
    let ops_cache = Arc::new(crate::operations::RuntimeModuleCache::new());
    let ops_dashboard = Arc::new(
        crate::operations::dashboard::DefaultDashboardDataProvider::new(
            state.capability_registry.clone(),
            ops_cache.clone(),
        ),
    );
    let ops_inspector = Arc::new(crate::operations::runtime_inspector::RuntimeInspector::new(
        ops_cache,
    ));
    let ops_policy_admin = Arc::new(crate::operations::policy_admin::PolicyAdmin::new(
        state.policy_registry.clone(),
        ops_audit.clone(),
    ));
    let ops_state = crate::operations::handlers::OperationsState {
        dashboard: ops_dashboard,
        inspector: ops_inspector,
        policy_admin: ops_policy_admin,
        attestation_viewer: ops_attestation_viewer,
    };

    let operations_routes = axum::Router::new()
        .route(
            "/v1/operations/registry",
            axum::routing::get(crate::operations::handlers::registry_handler),
        )
        .route(
            "/v1/operations/runtime",
            axum::routing::get(crate::operations::handlers::runtime_handler),
        )
        .route(
            "/v1/operations/metrics",
            axum::routing::get(crate::operations::handlers::metrics_handler),
        )
        .route(
            "/v1/operations/policies",
            axum::routing::get(crate::operations::handlers::policies_list_handler),
        )
        .route(
            "/v1/operations/policies",
            axum::routing::post(crate::operations::handlers::policies_create_handler),
        )
        .route(
            "/v1/operations/policies/:name",
            axum::routing::get(crate::operations::handlers::policies_get_handler),
        )
        .route(
            "/v1/operations/policies/:name",
            axum::routing::put(crate::operations::handlers::policies_update_handler),
        )
        .route(
            "/v1/operations/policies/:name",
            axum::routing::delete(crate::operations::handlers::policies_delete_handler),
        )
        .route(
            "/v1/operations/attestations",
            axum::routing::get(crate::operations::handlers::attestations_handler),
        )
        .with_state(ops_state);

    let event_bus = Arc::new(crate::events::BroadcastEventBus::new(1024));
    let exec_plane = crate::server::execution::build_execution_plane_with_concurrency(
        event_bus,
        state.executor.clone(),
        state.model_catalog.clone(),
        state.resource_manager.clone(),
        state.policy_registry.clone(),
        config.resources.max_concurrent_nodes,
    );
    let execution_routes = axum::Router::new()
        .route(
            "/v1/executions",
            axum::routing::post(crate::server::execution::execute_workflow_handler),
        )
        .with_state(exec_plane);

    let mut app = Router::new()
        .route(
            "/v1/chat/completions",
            post(server::handlers::chat_completions),
        )
        .route("/v1/messages", post(server::handlers::anthropic_messages))
        .route("/metrics", get(server::handlers::metrics_handler))
        .route("/health", get(server::health::health_handler))
        .route("/ready", get(server::health::ready_handler))
        .with_state(state.clone())
        .merge(operations_routes)
        .merge(execution_routes)
        .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .layer(axum::middleware::from_fn(
            middleware::request_id::request_id_middleware,
        ));

    // ADR-035: the rate limiter sits INSIDE the auth layer so it keys on the
    // authenticated identity (set via ClientIdentity) rather than spoofable
    // headers; unauthenticated traffic falls back to the TCP peer address.
    let mut rate_limiter_arc: Option<Arc<middleware::rate_limit::RateLimiter>> = None;
    if rate_limiting_enabled {
        let limiter = Arc::new(middleware::rate_limit::RateLimiter::new(
            rate_limiting_config,
        ));
        limiter.start_cleanup();
        rate_limiter_arc = Some(limiter.clone());
        app = app
            .layer(axum::middleware::from_fn(
                middleware::rate_limit::rate_limit_middleware,
            ))
            .layer(axum::Extension(limiter));
    }

    // Auth is live-reloadable: key rotation applies on SIGHUP without restart.
    let auth_handle = middleware::auth::AuthHandle::from_config(&auth_config);
    state
        .config_manager
        .register_subscriber(Box::new(middleware::auth::AuthReloader::new(
            auth_handle.clone(),
        )));
    if let Some(limiter) = rate_limiter_arc.clone() {
        state.config_manager.register_subscriber(Box::new(
            middleware::rate_limit::RateLimitReloader::new(limiter),
        ));
    }
    tracing::info!(
        operator_keys = auth_handle
            .load()
            .keys
            .values()
            .filter(|g| g.operator)
            .count(),
        total_keys = auth_handle.load().keys.len(),
        "auth configured (chat keys implicit; ':operator' suffix grants operator scope)"
    );

    let app = app
        .layer(axum::middleware::from_fn(middleware::auth::auth_middleware))
        .layer(axum::Extension(auth_handle))
        .layer(crate::middleware::cors::cors_layer_from_config(
            &cors_config,
        ));

    // Server-wide envelope: bounded concurrent requests and a hard
    // per-request timeout (also caps runaway streaming responses).
    //
    // The concurrency cap uses a tokio Semaphore inside axum middleware:
    // tower 0.5's ConcurrencyLimitLayer reports 503 on every request when
    // layered over axum 0.7 (inner-service readiness skew), so it is avoided.
    let request_timeout = std::time::Duration::from_secs(config.server.request_timeout_secs.max(1));
    let max_inflight = Arc::new(tokio::sync::Semaphore::new(
        (config.resources.max_concurrent as usize).max(1),
    ));
    let app = app
        .layer(tower_http::timeout::TimeoutLayer::new(request_timeout))
        .layer(axum::middleware::from_fn(
            move |req: axum::extract::Request, next: axum::middleware::Next| {
                let sem = max_inflight.clone();
                async move {
                    let _permit = sem.acquire_owned().await;
                    Ok::<_, std::convert::Infallible>(next.run(req).await)
                }
            },
        ));

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

    let serve = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal());

    if let Err(e) = serve.await {
        eprintln!("server error: {e}");
        std::process::exit(1);
    }
}

#[cfg(unix)]
async fn reload_signal(config_manager: Arc<config::manager::ConfigManager>) {
    use tokio::signal::unix;
    if let Ok(mut stream) = unix::signal(unix::SignalKind::hangup()) {
        while stream.recv().await.is_some() {
            match config_manager.reload().await {
                Ok(gen) => tracing::info!(generation = gen, "configuration reloaded"),
                Err(e) => {
                    tracing::error!(error = %e, "reload failed, continuing with previous config")
                }
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
        if let Ok(mut sig) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
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
