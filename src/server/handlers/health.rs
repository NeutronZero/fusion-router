use axum::{http::StatusCode, response::IntoResponse};

pub async fn metrics_handler() -> impl IntoResponse {
    let metrics = crate::telemetry::metrics::render_metrics();
    (
        StatusCode::OK,
        [("Content-Type", "text/plain; charset=utf-8")],
        metrics,
    )
}
