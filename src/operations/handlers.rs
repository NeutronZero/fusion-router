use std::sync::Arc;
use axum::{extract::State, http::StatusCode, Json};
use parking_lot::RwLock;
use serde_json::{json, Value};

use crate::capability::InMemoryCapabilityRegistry;
use crate::operations::TimeWindow;

use super::dashboard::{DashboardDataProvider, DefaultDashboardDataProvider};
use super::runtime_inspector::RuntimeInspector;
use super::policy_admin::PolicyAdmin;
use super::attestation_viewer::AttestationViewer;
use super::MockPackageVerifier;
use super::RuntimeModuleCache;

#[derive(Clone)]
pub struct OperationsState {
    pub dashboard: Arc<dyn DashboardDataProvider + Send + Sync>,
    pub inspector: Arc<RuntimeInspector>,
    pub policy_admin: Arc<PolicyAdmin>,
    pub attestation_viewer: Arc<AttestationViewer>,
}

impl OperationsState {
    pub fn new_mock() -> Self {
        use parking_lot::Mutex as ParkingMutex;
        use crate::telemetry::audit::AuditLog;

        let registry = Arc::new(RwLock::new(InMemoryCapabilityRegistry::new()));
        let cache = Arc::new(RuntimeModuleCache::new());
        let dashboard = Arc::new(DefaultDashboardDataProvider::new(registry, cache));
        let inspector = Arc::new(RuntimeInspector::new(Arc::new(RuntimeModuleCache::new())));
        let store = Arc::new(ParkingMutex::new(Vec::new()));
        let audit = Arc::new(AuditLog::new(100));
        let policy_admin = Arc::new(PolicyAdmin::new(store, audit.clone()));
        let verifier = Arc::new(MockPackageVerifier);
        let attestation_viewer = Arc::new(AttestationViewer::new(verifier, audit.clone()));

        Self {
            dashboard,
            inspector,
            policy_admin,
            attestation_viewer,
        }
    }
}

pub async fn registry_handler(
    State(state): State<OperationsState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match state.dashboard.registry_summary() {
        Ok(summary) => Ok(Json(serde_json::to_value(summary).unwrap())),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()})))),
    }
}

pub async fn runtime_handler(
    State(state): State<OperationsState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match state.dashboard.runtime_summary() {
        Ok(summary) => Ok(Json(serde_json::to_value(summary).unwrap())),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()})))),
    }
}

pub async fn metrics_handler(
    State(state): State<OperationsState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let window_secs = params.get("window").and_then(|w| w.trim_end_matches('h').parse::<i64>().ok()).unwrap_or(1);
    let now = chrono::Utc::now().timestamp();
    let window = TimeWindow {
        start_secs: now - window_secs * 3600,
        end_secs: now,
    };
    match state.dashboard.invocation_metrics(window) {
        Ok(metrics) => Ok(Json(serde_json::to_value(metrics).unwrap())),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()})))),
    }
}

pub async fn policies_list_handler(
    State(state): State<OperationsState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match state.policy_admin.list_policies() {
        Ok(policies) => Ok(Json(serde_json::to_value(policies).unwrap())),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()})))),
    }
}

pub async fn policies_create_handler(
    State(state): State<OperationsState>,
    Json(decl): Json<crate::policy::PolicyDeclaration>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match state.policy_admin.create_policy(decl) {
        Ok(()) => Ok(Json(json!({"status": "created"}))),
        Err(e) => Err((StatusCode::BAD_REQUEST, Json(json!({"error": e.to_string()})))),
    }
}

pub async fn attestations_handler(
    State(state): State<OperationsState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match state.attestation_viewer.list_packages() {
        Ok(statuses) => Ok(Json(serde_json::to_value(statuses).unwrap())),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()})))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;
    use axum::Router;

    #[tokio::test]
    async fn test_registry_route_returns_json() {
        let ops_state = OperationsState::new_mock();
        let app = Router::new()
            .route("/v1/operations/registry", get(registry_handler))
            .with_state(ops_state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let resp = reqwest::get(format!("http://{}/v1/operations/registry", addr))
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert!(body.get("total_capabilities").is_some());
    }
}
