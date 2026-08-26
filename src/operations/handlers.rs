use axum::{extract::State, http::StatusCode, Json};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::operations::TimeWindow;

use super::attestation_viewer::AttestationViewer;
use super::dashboard::DashboardDataProvider;
use super::policy_admin::PolicyAdmin;
use super::runtime_inspector::RuntimeInspector;

#[cfg(test)]
use {
    super::dashboard::DefaultDashboardDataProvider, super::MockPackageVerifier,
    super::RuntimeModuleCache, crate::capability::InMemoryCapabilityRegistry,
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

        let registry: Arc<dyn crate::capability::CapabilityRegistry> =
            Arc::new(InMemoryCapabilityRegistry::new());
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

/// Maximum accepted metrics window in hours.
const MAX_WINDOW_HOURS: u64 = 720;

/// Parses the `window` query parameter as a whole number of hours.
/// Rejects 0 and values above MAX_WINDOW_HOURS (400 material); accepts an
/// optional trailing `h` suffix (`"24h"` or `"24"`).
fn parse_window_hours(params: &std::collections::HashMap<String, String>) -> Result<u64, String> {
    let Some(raw) = params.get("window") else {
        return Ok(1);
    };
    let cleaned = raw.trim().trim_end_matches('h');
    let hours: u64 = cleaned
        .parse()
        .map_err(|_| format!("invalid window '{raw}': expected integer hours (e.g. '24h')"))?;
    if hours == 0 {
        return Err("window must be at least 1 hour".into());
    }
    if hours > MAX_WINDOW_HOURS {
        return Err(format!(
            "window exceeds maximum of {MAX_WINDOW_HOURS} hours"
        ));
    }
    Ok(hours)
}

pub async fn registry_handler(
    State(state): State<OperationsState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match state.dashboard.registry_summary() {
        Ok(summary) => json_value(summary),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )),
    }
}

pub async fn runtime_handler(
    State(state): State<OperationsState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match state.dashboard.runtime_summary() {
        Ok(summary) => json_value(summary),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )),
    }
}

pub async fn metrics_handler(
    State(state): State<OperationsState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let window_hours = match parse_window_hours(&params) {
        Ok(h) => h,
        Err(msg) => {
            return Err((StatusCode::BAD_REQUEST, Json(json!({"error": msg}))));
        }
    };
    let now = chrono::Utc::now().timestamp();
    // window_hours is bounded [1, 720], so the multiply cannot overflow; the
    // saturating ops keep start <= end even under clock skew.
    let window_secs = window_hours.saturating_mul(3600);
    let start_secs = now.saturating_sub(window_secs.min(i64::MAX as u64) as i64);
    let window = TimeWindow {
        start_secs,
        end_secs: now,
    };
    match state.dashboard.invocation_metrics(window) {
        Ok(metrics) => json_value(metrics),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )),
    }
}

pub async fn policies_list_handler(
    State(state): State<OperationsState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match state.policy_admin.list_policies() {
        Ok(policies) => json_value(policies),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )),
    }
}

pub async fn policies_create_handler(
    State(state): State<OperationsState>,
    Json(decl): Json<crate::policy::PolicyDeclaration>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match state.policy_admin.create_policy(decl) {
        Ok(()) => Ok(Json(json!({"status": "created"}))),
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": e.to_string()})),
        )),
    }
}

pub async fn policies_get_handler(
    State(state): State<OperationsState>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match state.policy_admin.get_policy(&name) {
        Ok(Some(decl)) => json_value(decl),
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("policy '{name}' not found")})),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )),
    }
}

pub async fn policies_update_handler(
    State(state): State<OperationsState>,
    axum::extract::Path(name): axum::extract::Path<String>,
    Json(decl): Json<crate::policy::PolicyDeclaration>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match state.policy_admin.update_policy(&name, decl) {
        Ok(()) => Ok(Json(json!({"status": "updated"}))),
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": e.to_string()})),
        )),
    }
}

pub async fn policies_delete_handler(
    State(state): State<OperationsState>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match state.policy_admin.delete_policy(&name) {
        Ok(()) => Ok(Json(json!({"status": "deleted"}))),
        Err(e) => Err((StatusCode::NOT_FOUND, Json(json!({"error": e.to_string()})))),
    }
}

pub async fn attestations_handler(
    State(state): State<OperationsState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match state.attestation_viewer.list_packages() {
        Ok(statuses) => json_value(statuses),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )),
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

    fn window_params(window: Option<&str>) -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        if let Some(w) = window {
            m.insert("window".to_string(), w.to_string());
        }
        m
    }

    #[test]
    fn test_parse_window_hours_rejection_table() {
        // Accepted values
        assert_eq!(parse_window_hours(&window_params(None)).unwrap(), 1);
        assert_eq!(parse_window_hours(&window_params(Some("1h"))).unwrap(), 1);
        assert_eq!(parse_window_hours(&window_params(Some("24h"))).unwrap(), 24);
        assert_eq!(parse_window_hours(&window_params(Some("12"))).unwrap(), 12);
        assert_eq!(
            parse_window_hours(&window_params(Some("720h"))).unwrap(),
            720
        );

        // Rejected values
        for bad in [
            "0h",
            "0",
            "721h",
            "-4h",
            "-1",
            "abc",
            "1.5h",
            "",
            "99999999999999999999",
        ] {
            assert!(
                parse_window_hours(&window_params(Some(bad))).is_err(),
                "window '{bad}' must be rejected"
            );
        }
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
