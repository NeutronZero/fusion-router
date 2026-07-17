use axum::http::{HeaderName, HeaderValue, Method};
use tower_http::cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer};
use crate::config::CorsConfig;

pub fn cors_layer_from_config(config: &CorsConfig) -> CorsLayer {
    let cors = CorsLayer::new();

    let has_wildcard = config.allowed_origins.iter().any(|o| o == "*");
    let cors = if has_wildcard {
        cors.allow_origin(AllowOrigin::any())
    } else {
        let origins: Vec<HeaderValue> = config.allowed_origins.iter()
            .filter_map(|o| o.parse::<HeaderValue>().ok())
            .collect();
        cors.allow_origin(AllowOrigin::list(origins))
    };

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CorsConfig;

    #[test]
    fn test_cors_layer_defaults() {
        let config = CorsConfig::default();
        let layer = cors_layer_from_config(&config);
        let _ = layer;
    }

    #[test]
    fn test_cors_layer_empty_origins() {
        let config = CorsConfig {
            allowed_origins: vec![],
            allowed_methods: vec![],
            allowed_headers: vec![],
        };
        let layer = cors_layer_from_config(&config);
        let _ = layer;
    }

    #[test]
    fn test_cors_layer_specific_origin() {
        let config = CorsConfig {
            allowed_origins: vec!["https://example.com".into()],
            allowed_methods: vec!["GET".into()],
            allowed_headers: vec!["x-custom".into()],
        };
        let layer = cors_layer_from_config(&config);
        let _ = layer;
    }
}
