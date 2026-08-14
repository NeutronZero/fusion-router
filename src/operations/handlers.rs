use std::sync::Arc;
use axum::{extract::State, http::StatusCode, Json};
use serde_json::{json, Value};

use crate::operations::TimeWindow;

use super::dashboard::DashboardDataProvider;
use super::runtime_inspector::RuntimeInspector;
use super::policy_admin::PolicyAdmin;
use super::attestation_viewer::AttestationViewer;

#[cfg(test)]
use {
    parking_lot::RwLock,
    crate::capability::InMemoryCapabilityRegistry,
    super::dashboard::DefaultDashboardDataProvider,
    super::MockPackageVerifier,
    super::RuntimeModuleCache,
};

#[derive(Clone)]
pub struct OperationsState {
    pub dashboard: Arc<dyn DashboardDataProvider + Send + Sync>,
    #[allow(dead_code)]
    pub inspector: Arc<RuntimeInspector>,
    pub policy_admin: Arc<PolicyAdmin>,
    pub attestation_viewer: Arc<AttestationViewer>,
}

#[cfg(test)]
impl OperationsState {
    pub fn new_mock() -> Self {
        use crate::telemetry::audit::AuditLog;

        let registry = Arc::new(RwLock::new(InMemoryCapabilityRegistry::new()));
        let cache = Arc::new(RuntimeModuleCache::new());
        let dashboard = Arc::new(DefaultDashboardDataProvider::new(registry, cache));
        let inspector = Arc::new(RuntimeInspector::new(Arc::new(RuntimeModuleCache::new())));
        let policy_registry = Arc::new(crate::policy::PolicyRegistry::new());
        let audit = Arc::new(AuditLog::new(100));
        let policy_admin = Arc::new(PolicyAdmin::new(policy_registry, audit.clone()));
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

fn json_value<T: serde::Serialize>(value: T) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    serde_json::to_value(value).map(Json).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("response serialization failed: {}", e)})),
        )
    })
}

pub async fn registry_handler(
    State(state): State<OperationsState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match state.dashboard.registry_summary() {
        Ok(summary) => json_value(summary),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()})))),
    }
}

pub async fn runtime_handler(
    State(state): State<OperationsState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match state.dashboard.runtime_summary() {
        Ok(summary) => json_value(summary),
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
        Ok(metrics) => json_value(metrics),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()})))),
    }
}

pub async fn policies_list_handler(
    State(state): State<OperationsState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match state.policy_admin.list_policies() {
        Ok(policies) => json_value(policies),
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
        Ok(statuses) => json_value(statuses),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()})))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;
    use axum::Router;

    struct Unserializable;

    impl serde::Serialize for Unserializable {
        fn serialize<S: serde::Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("boom"))
        }
    }

    #[test]
    fn test_json_value_maps_serialization_failure_to_error_response() {
        let result = json_value(Unserializable);
        assert!(result.is_err());
        let (status, body) = result.unwrap_err();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(body.0.get("error").is_some());
    }

    #[test]
    fn test_json_value_serializes_ok_value() {
        let result = json_value(json!({"total_capabilities": 3}));
        assert!(result.is_ok());
        let body = result.unwrap().0;
        assert_eq!(body["total_capabilities"], 3);
    }

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
