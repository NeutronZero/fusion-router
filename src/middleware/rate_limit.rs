use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use dashmap::{DashMap, DashSet};
use serde_json::json;
use tokio::time::sleep;

use crate::config::RateLimitingConfig;

#[derive(Clone)]
pub struct RateLimiter {
    buckets: Arc<DashMap<String, Bucket>>,
    config: RateLimitingConfig,
    cleanup_started: Arc<DashSet<bool>>,
}

#[derive(Clone)]
struct Bucket {
    tokens: f64,
    last_refill: Instant,
    last_access: Instant,
}

impl RateLimiter {
    pub fn new(config: RateLimitingConfig) -> Self {
        Self {
            buckets: Arc::new(DashMap::new()),
            config,
            cleanup_started: Arc::new(DashSet::new()),
        }
    }

    pub fn start_cleanup(&self) {
        if self.cleanup_started.contains(&true) {
            return;
        }
        self.cleanup_started.insert(true);

        let buckets = self.buckets.clone();
        let interval = Duration::from_secs(self.config.cleanup_interval_secs);
        tokio::spawn(async move {
            loop {
                sleep(interval).await;
                let cutoff = Instant::now() - Duration::from_secs(interval.as_secs() * 2);
                buckets.retain(|_, b| b.last_access > cutoff);
            }
        });
    }

    fn refill_tokens(&self, bucket: &mut Bucket) {
        let now = Instant::now();
        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
        let rate = self.config.requests_per_minute as f64 / 60.0;
        bucket.tokens = (bucket.tokens + elapsed * rate).min(self.config.burst_size as f64);
        bucket.last_refill = now;
    }

    pub fn check_rate(&self, client_id: &str) -> Result<(), u64> {
        let mut bucket = self.buckets.entry(client_id.to_string()).or_insert_with(|| Bucket {
            tokens: self.config.burst_size as f64,
            last_refill: Instant::now(),
            last_access: Instant::now(),
        });

        self.refill_tokens(&mut bucket);
        bucket.last_access = Instant::now();

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(())
        } else {
            let retry_after = (1.0 / (self.config.requests_per_minute as f64 / 60.0)).ceil() as u64;
            Err(retry_after)
        }
    }
}

pub async fn rate_limit_middleware(
    req: Request,
    next: Next,
) -> Result<Response, (StatusCode, String)> {
    let config = req
        .extensions()
        .get::<RateLimiter>()
        .cloned();

    let limiter = match config {
        Some(l) => l,
        None => return Ok(next.run(req).await),
    };

    let path = req.uri().path();
    if path == "/health" || path == "/ready" || path == "/metrics" {
        return Ok(next.run(req).await);
    }

    let client_id = req
        .headers()
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| {
            req.headers()
                .get("x-forwarded-for")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "unknown".to_string());

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

    #[test]
    fn test_rate_limiter_allows_burst() {
        let config = RateLimitingConfig {
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
}
