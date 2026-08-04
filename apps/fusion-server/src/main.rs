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

    let app = fusion_studio_api::router();
    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));
    tracing::info!("FusionStudio server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
