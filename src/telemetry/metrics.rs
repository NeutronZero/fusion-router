use prometheus::{Encoder, HistogramVec, IntCounter, IntCounterVec, TextEncoder};
use std::sync::OnceLock;

static METRICS: OnceLock<FusionMetrics> = OnceLock::new();

pub struct FusionMetrics {
    pub requests_total: IntCounter,
    pub request_duration_seconds: HistogramVec,
    pub errors_total: IntCounter,
    pub tokens_total: IntCounter,
    pub provider_latency_seconds: HistogramVec,
    pub strategy_latency_seconds: HistogramVec,
    pub strategy_errors_total: IntCounterVec,
    pub graph_hash_count: IntCounter,
}

fn safe_int_counter(name: &str, help: &str) -> IntCounter {
    let opts = prometheus::Opts::new(name, help);
    let counter = IntCounter::with_opts(opts).expect("valid static counter opts");
    let _ = prometheus::default_registry().register(Box::new(counter.clone()));
    counter
}

fn safe_int_counter_vec(name: &str, help: &str, labels: &[&str]) -> IntCounterVec {
    let opts = prometheus::Opts::new(name, help);
    let counter = IntCounterVec::new(opts, labels).expect("valid static counter vec opts");
    let _ = prometheus::default_registry().register(Box::new(counter.clone()));
    counter
}

fn safe_histogram_vec(name: &str, help: &str, labels: &[&str]) -> HistogramVec {
    let opts = prometheus::HistogramOpts::new(name, help);
    let hist = HistogramVec::new(opts, labels).expect("valid static histogram vec opts");
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
                "Total number of requests",
            ),
            request_duration_seconds: safe_histogram_vec(
                "fusionrouter_request_duration_seconds",
                "Request duration in seconds",
                &["route"],
            ),
            errors_total: safe_int_counter("fusionrouter_errors_total", "Total number of errors"),
            tokens_total: safe_int_counter("fusionrouter_tokens_total", "Total tokens consumed"),
            provider_latency_seconds: safe_histogram_vec(
                "fusionrouter_provider_latency_seconds",
                "Provider latency in seconds",
                // NOTE: cardinality bounded by known provider enum; custom
                // provider names are normalized to "other" by the caller.
                &["provider"],
            ),
            strategy_latency_seconds: safe_histogram_vec(
                "fusionrouter_strategy_latency_seconds",
                "Per-strategy latency in seconds",
                // NOTE: cardinality bounded by StrategyKind enum variants;
                // Custom(String) variants are normalized to "other".
                &["strategy"],
            ),
            strategy_errors_total: safe_int_counter_vec(
                "fusionrouter_strategy_errors_total",
                "Per-strategy error count",
                // NOTE: cardinality bounded by StrategyKind enum variants.
                &["strategy"],
            ),
            graph_hash_count: safe_int_counter(
                "fusionrouter_graph_hash_count",
                // Unlabeled: each request compiles a unique graph, so a
                // per-hash label would create unbounded cardinality.
                "Total compiled graphs",
            ),
        }
    }
}

pub fn render_metrics() -> String {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    let _ = encoder.encode(&metric_families, &mut buffer);
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
    fn test_metrics_render_uses_prometheus_format() {
        let metrics = FusionMetrics::instance();
        metrics
            .request_duration_seconds
            .with_label_values(&["test"])
            .observe(0.01);
        let output = render_metrics();

        // Standard prometheus text format: HELP and TYPE declarations
        assert!(output.contains("# HELP fusionrouter_requests_total"));
        assert!(output.contains("# TYPE fusionrouter_requests_total counter"));
        assert!(output.contains("# TYPE fusionrouter_request_duration_seconds histogram"));

        // Counter lines must end with a plain integer
        for line in output
            .lines()
            .filter(|l| l.starts_with("fusionrouter_requests_total"))
        {
            let value = line.rsplit(' ').next().unwrap_or("");
            assert!(
                value.parse::<u64>().is_ok(),
                "counter sample must be an integer, got: {line}"
            );
        }
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

    #[test]
    fn test_metrics_strategy_latency() {
        let metrics = FusionMetrics::instance();
        metrics
            .strategy_latency_seconds
            .with_label_values(&["Single"])
            .observe(0.125);
        let output = render_metrics();
        assert!(output.contains("fusionrouter_strategy_latency_seconds"));
        assert!(output.contains("strategy=\"Single\""));
    }

    #[test]
    fn test_metrics_strategy_errors() {
        let metrics = FusionMetrics::instance();
        metrics
            .strategy_errors_total
            .with_label_values(&["Consensus"])
            .inc();
        let output = render_metrics();
        assert!(output.contains("fusionrouter_strategy_errors_total"));
        assert!(output.contains("strategy=\"Consensus\""));
    }

    #[test]
    fn test_metrics_graph_hash_count() {
        let metrics = FusionMetrics::instance();
        let before = metrics.graph_hash_count.get();
        metrics.graph_hash_count.inc();
        let output = render_metrics();
        assert_eq!(metrics.graph_hash_count.get(), before + 1);
        assert!(output.contains("fusionrouter_graph_hash_count"));
    }
}
