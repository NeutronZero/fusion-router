use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    extract::{ConnectInfo, Request},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use dashmap::DashMap;
use serde_json::json;
use tokio::time::sleep;

use crate::config::RateLimitingConfig;
use crate::middleware::auth::ClientIdentity;

/// Upper bound on distinct rate-limit buckets; past this the limiter denies
/// new clients instead of growing unboundedly (M2 / ADR-035).
pub const MAX_BUCKETS: usize = 100_000;

#[derive(Clone)]
pub struct RateLimiter {
    buckets: Arc<DashMap<String, Bucket>>,
    config: Arc<arc_swap::ArcSwap<RateLimitingConfig>>,
    cleanup_started: Arc<AtomicBool>,
}

#[derive(Clone)]
struct Bucket {
    tokens: f64,
    last_refill: Instant,
    last_access: Instant,
}

/// Derives the rate-limit bucket key from unspoofable identity only:
/// 1. authenticated identity (set by the auth middleware, already inside the
///    auth layer); 2. the TCP peer address via `ConnectInfo`; 3. fallback
///    `"unknown"` when neither is available (only possible when the router
///    lacks connect-info support — production wiring always provides it).
///
/// `x-forwarded-for` is never consulted (M2): it is client-controlled, so
/// spoofing it must not mint fresh buckets or reset existing ones.
pub fn client_identity(req: &Request, authenticated: Option<&str>) -> String {
    if let Some(identity) = authenticated {
        return identity.to_string();
    }
    if let Some(ConnectInfo(addr)) = req.extensions().get::<ConnectInfo<SocketAddr>>() {
        return format!("peer:{}", addr.ip());
    }
    "unknown".to_string()
}

impl RateLimiter {
    pub fn new(config: RateLimitingConfig) -> Self {
        Self {
            buckets: Arc::new(DashMap::new()),
            config: Arc::new(arc_swap::ArcSwap::from_pointee(config)),
            cleanup_started: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn start_cleanup(&self) {
        if self
            .cleanup_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
        {
            return;
        }

        let buckets = self.buckets.clone();
        let interval_secs = self.config.load().cleanup_interval_secs.max(1);
        let interval = Duration::from_secs(interval_secs);
        tokio::spawn(async move {
            loop {
                sleep(interval).await;
                let buckets = buckets.clone();
                if let Err(e) = tokio::task::spawn_blocking(move || {
                    let cutoff = Instant::now() - Duration::from_secs(interval.as_secs() * 2);
                    buckets.retain(|_, b| b.last_access > cutoff);
                })
                .await
                {
                    tracing::warn!(error = %e, "Rate limiter cleanup panicked, restarting");
                }
            }
        });
    }

    fn refill_tokens(&self, bucket: &mut Bucket) {
        let cfg = self.config.load();
        let now = Instant::now();
        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
        let rate = cfg.requests_per_minute as f64 / 60.0;
        bucket.tokens = (bucket.tokens + elapsed * rate).min(cfg.burst_size as f64);
        bucket.last_refill = now;
    }

    /// Hot-applies new limiter settings (rpm/burst) without touching buckets.
    pub fn update_config(&self, config: RateLimitingConfig) {
        self.config.store(Arc::new(config));
    }

    pub fn check_rate(&self, client_id: &str) -> Result<(), u64> {
        if !self.buckets.contains_key(client_id) && self.buckets.len() >= MAX_BUCKETS {
            // Bucket cap: deny new clients instead of growing the map
            // without bound (M2 / ADR-035). Cleanup reclaims stale buckets.
            return Err(429);
        }
        let cfg = self.config.load();
        let mut bucket = self
            .buckets
            .entry(client_id.to_string())
            .or_insert_with(|| Bucket {
                tokens: cfg.burst_size as f64,
                last_refill: Instant::now(),
                last_access: Instant::now(),
            });

        self.refill_tokens(&mut bucket);
        bucket.last_access = Instant::now();

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(())
        } else {
            let retry_after = (1.0 / (cfg.requests_per_minute as f64 / 60.0)).ceil() as u64;
            Err(retry_after)
        }
    }
}

/// Reload subscriber hot-applying rate-limit settings.
pub struct RateLimitReloader {
    pub limiter: Arc<RateLimiter>,
    staged: std::sync::Mutex<Option<RateLimitingConfig>>,
}

impl RateLimitReloader {
    pub fn new(limiter: Arc<RateLimiter>) -> Self {
        Self {
            limiter,
            staged: std::sync::Mutex::new(None),
        }
    }
}

impl crate::config::manager::ConfigSubscriber for RateLimitReloader {
    fn priority(&self) -> u8 {
        2
    }

    fn prepare(
        &self,
        _old: &crate::config::manager::ConfigSnapshot,
        new: &crate::config::manager::ConfigSnapshot,
    ) -> Result<(), crate::config::error::ReloadError> {
        *self.staged.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(new.config.rate_limiting.clone());
        Ok(())
    }

    fn commit(&self, _generation: u64) {
        if let Some(cfg) = self.staged.lock().unwrap_or_else(|e| e.into_inner()).take() {
            self.limiter.update_config(cfg);
            tracing::info!("rate limiting settings hot-applied");
        }
    }
}

pub async fn rate_limit_middleware(
    req: Request,
    next: Next,
) -> Result<Response, (StatusCode, String)> {
    // Production wiring inserts Arc<RateLimiter>; accept the bare type too so
    // tests can insert either form.
    let limiter: Option<Arc<RateLimiter>> = match req.extensions().get::<Arc<RateLimiter>>() {
        Some(l) => Some(l.clone()),
        None => req
            .extensions()
            .get::<RateLimiter>()
            .map(|l| Arc::new(l.clone())),
    };

    let limiter = match limiter {
        Some(l) => l,
        // Fail closed (ADR-035): a router wiring this middleware without the
        // limiter extension is a wiring bug, not an invitation.
        None => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                json!({"error": "rate limiting unavailable"}).to_string(),
            ))
        }
    };

    let path = req.uri().path();
    if path == "/health" || path == "/ready" || path == "/metrics" {
        return Ok(next.run(req).await);
    }

    let authenticated = req
        .extensions()
        .get::<ClientIdentity>()
        .map(|id| id.0.as_str());

    let client_id = client_identity(&req, authenticated);

    match limiter.check_rate(&client_id) {
        Ok(()) => Ok(next.run(req).await),
        Err(retry_after) => Err((
            StatusCode::TOO_MANY_REQUESTS,
            json!({"error": "rate_limit_exceeded", "retry_after_secs": retry_after}).to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn test_req() -> Request {
        Request::builder()
            .uri("/v1/chat/completions")
            .body(axum::body::Body::empty())
            .unwrap()
    }

    #[test]
    fn test_client_identity_prefers_connect_info_over_spoofed_xff() {
        let mut req = test_req();
        req.headers_mut()
            .insert("x-forwarded-for", HeaderValue::from_static("203.0.113.9"));
        req.extensions_mut()
            .insert(ConnectInfo("1.2.3.4:5555".parse::<SocketAddr>().unwrap()));

        let id = client_identity(&req, None);
        assert_eq!(
            id, "peer:1.2.3.4",
            "x-forwarded-for must never key a bucket"
        );
    }

    #[test]
    fn test_client_identity_ignores_spoofed_xff_without_connect_info() {
        let mut req = test_req();
        req.headers_mut()
            .insert("x-forwarded-for", HeaderValue::from_static("203.0.113.9"));
        assert_eq!(client_identity(&req, None), "unknown");
    }

    #[test]
    fn test_client_identity_uses_authenticated_identity() {
        let req = test_req();
        assert_eq!(
            client_identity(&req, Some("key:abcd")),
            "key:abcd",
            "authenticated identity must win over peer address"
        );
    }

    #[test]
    fn test_spoofed_xff_cannot_reset_buckets() {
        // Same peer, different spoofed x-forwarded-for: same bucket, so the
        // second request is limited (burst 2 already consumed).
        let config = RateLimitingConfig {
            enabled: true,
            requests_per_minute: 60,
            burst_size: 2,
            cleanup_interval_secs: 300,
        };
        let limiter = RateLimiter::new(config);
        assert!(limiter.check_rate("peer:1.2.3.4").is_ok());
        assert!(limiter.check_rate("peer:1.2.3.4").is_ok());
        assert!(limiter.check_rate("peer:1.2.3.4").is_err());
    }

    #[test]
    fn test_bucket_count_is_capped() {
        let config = RateLimitingConfig {
            enabled: true,
            requests_per_minute: 60,
            burst_size: 2,
            cleanup_interval_secs: 300,
        };
        let limiter = RateLimiter::new(config);

        for i in 0..MAX_BUCKETS {
            assert!(limiter.check_rate(&format!("client-{i}")).is_ok());
        }
        assert_eq!(limiter.buckets.len(), MAX_BUCKETS);

        // A brand-new client past the cap is denied (bucket cap enforced);
        // an existing client keeps its bucket.
        assert!(limiter
            .check_rate(&format!("client-{}", MAX_BUCKETS + 1))
            .is_err());
        assert!(limiter.check_rate("client-0").is_ok());
    }

    #[test]
    fn test_rate_limiter_allows_burst() {
        let config = RateLimitingConfig {
            enabled: false,
            requests_per_minute: 60,
            burst_size: 5,
            cleanup_interval_secs: 300,
        };
        let limiter = RateLimiter::new(config);

        for _ in 0..5 {
            assert!(limiter.check_rate("test-client").is_ok());
        }
    }

    #[test]
    fn test_rate_limiter_blocks_after_burst() {
        let config = RateLimitingConfig {
            enabled: false,
            requests_per_minute: 60,
            burst_size: 3,
            cleanup_interval_secs: 300,
        };
        let limiter = RateLimiter::new(config);

        for _ in 0..3 {
            assert!(limiter.check_rate("test-client").is_ok());
        }
        let result = limiter.check_rate("test-client");
        assert!(result.is_err());
        assert!(result.unwrap_err() > 0);
    }

    #[test]
    fn test_rate_limiter_different_clients_independent() {
        let config = RateLimitingConfig {
            enabled: false,
            requests_per_minute: 60,
            burst_size: 2,
            cleanup_interval_secs: 300,
        };
        let limiter = RateLimiter::new(config);

        assert!(limiter.check_rate("client-a").is_ok());
        assert!(limiter.check_rate("client-a").is_ok());
        assert!(limiter.check_rate("client-a").is_err());

        assert!(limiter.check_rate("client-b").is_ok());
        assert!(limiter.check_rate("client-b").is_ok());
        assert!(limiter.check_rate("client-b").is_err());
    }

    #[tokio::test]
    async fn test_rate_limiter_zero_cleanup_interval_clamped() {
        let config = RateLimitingConfig {
            enabled: true,
            requests_per_minute: 60,
            burst_size: 5,
            cleanup_interval_secs: 0,
        };
        let limiter = RateLimiter::new(config);

        limiter.start_cleanup();

        assert!(limiter.cleanup_started.load(Ordering::Relaxed));

        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
