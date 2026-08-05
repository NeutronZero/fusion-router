use fusion_core::PlatformStatus;
use fusion_infrastructure::Database;
use fusion_kernel::{CapabilitySystem, EventBus, KernelEvent, SystemCatalog};
use std::net::SocketAddr;
use uuid::Uuid;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    tracing::info!("Initializing FusionRouter v0.14.0 (AF-003 Architecture)...");

    let event_bus = EventBus::new(100);
    let _capability_system = CapabilitySystem::new();
    let _catalog = SystemCatalog::new();

    let db = Database::memory().expect("In-memory SQLite DB");
    db.migrate().expect("Run SQLite migrations");

    let _ = event_bus.publish(KernelEvent::PlatformStatusChanged {
        id: Uuid::new_v4(),
        status: PlatformStatus::Ready,
    });

    // SIMULATION-ONLY sandbox binary. This serves the Studio UI with placeholder
    // data (see fusion-studio-api) and is NOT the production request path — the
    // production server is the `fusion-router` monolith in src/main.rs.
    //
    // Default port 8787 (NOT 8080) to avoid clashing with the monolith's default.
    let port: u16 = std::env::var("FUSION_STUDIO_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8787);

    let app = fusion_studio_api::router();
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    tracing::info!("FusionStudio SIMULATION server listening on http://{} (simulation-only)", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
