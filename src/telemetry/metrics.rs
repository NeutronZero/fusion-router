use std::sync::OnceLock;
use prometheus::{
    HistogramVec, IntCounter, TextEncoder, Encoder,
};

static METRICS: OnceLock<FusionMetrics> = OnceLock::new();

pub struct FusionMetrics {
    pub requests_total: IntCounter,
    pub request_duration_seconds: HistogramVec,
    pub errors_total: IntCounter,
    pub tokens_total: IntCounter,
    pub provider_latency_seconds: HistogramVec,
}

fn safe_int_counter(name: &str, help: &str) -> IntCounter {
    let opts = prometheus::Opts::new(name, help);
    let counter = IntCounter::with_opts(opts).unwrap();
    let _ = prometheus::default_registry().register(Box::new(counter.clone()));
    counter
}

fn safe_histogram_vec(name: &str, help: &str, labels: &[&str]) -> HistogramVec {
    let opts = prometheus::HistogramOpts::new(name, help);
    let hist = HistogramVec::new(opts, labels).unwrap();
    let _ = prometheus::default_registry().register(Box::new(hist.clone()));
    hist
}

impl FusionMetrics {
    pub fn instance() -> &'static Self {
        METRICS.get_or_init(Self::new)
    }

    fn new() -> Self {
        Self {
            requests_total: safe_int_counter(
                "fusionrouter_requests_total",
                "Total number of requests"
            ),
            request_duration_seconds: safe_histogram_vec(
                "fusionrouter_request_duration_seconds",
                "Request duration in seconds",
                &["route"]
            ),
            errors_total: safe_int_counter(
                "fusionrouter_errors_total",
                "Total number of errors"
            ),
            tokens_total: safe_int_counter(
                "fusionrouter_tokens_total",
                "Total tokens consumed"
            ),
            provider_latency_seconds: safe_histogram_vec(
                "fusionrouter_provider_latency_seconds",
                "Provider latency in seconds",
                &["provider"]
            ),
        }
    }
}

pub fn render_metrics() -> String {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_instance_is_singleton() {
        let a = FusionMetrics::instance();
        let b = FusionMetrics::instance();
        assert!(std::ptr::eq(a, b));
    }

    #[test]
    fn test_metrics_increment_counters() {
        let metrics = FusionMetrics::instance();
        let before = metrics.requests_total.get();
        metrics.requests_total.inc();
        assert_eq!(metrics.requests_total.get(), before + 1);
    }

    #[test]
    fn test_metrics_render_contains_expected() {
        let _ = FusionMetrics::instance();
        let output = render_metrics();
        assert!(output.contains("fusionrouter_requests_total"));
        assert!(output.contains("fusionrouter_errors_total"));
        assert!(output.contains("fusionrouter_tokens_total"));
    }

    #[test]
    fn test_metrics_observe_duration() {
        let metrics = FusionMetrics::instance();
        metrics
            .request_duration_seconds
            .with_label_values(&["test_route"])
            .observe(0.042);
        let output = render_metrics();
        assert!(output.contains("fusionrouter_request_duration_seconds"));
    }
}
