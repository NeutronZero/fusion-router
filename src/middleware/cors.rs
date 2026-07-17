use axum::http::{HeaderName, HeaderValue, Method};
use tower_http::cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer};
use crate::config::CorsConfig;

pub fn cors_layer_from_config(config: &CorsConfig) -> CorsLayer {
    let origins: Vec<HeaderValue> = config.allowed_origins.iter()
        .filter_map(|o| o.parse::<HeaderValue>().ok())
        .collect();

    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins));

    let cors = if config.allowed_methods.is_empty() {
        cors
    } else {
        let methods: Vec<Method> = config.allowed_methods.iter()
            .filter_map(|m| m.parse::<Method>().ok())
            .collect();
        cors.allow_methods(AllowMethods::list(methods))
    };

    if config.allowed_headers.is_empty() {
        cors
    } else {
        let headers: Vec<HeaderName> = config.allowed_headers.iter()
            .filter_map(|h| h.parse::<HeaderName>().ok())
            .collect();
        cors.allow_headers(AllowHeaders::list(headers))
    }
}
