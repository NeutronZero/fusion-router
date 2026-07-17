use std::sync::OnceLock;
use prometheus::{
    register_int_counter, register_histogram_vec,
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

impl FusionMetrics {
    pub fn instance() -> &'static Self {
        METRICS.get_or_init(Self::new)
    }

    fn new() -> Self {
        Self {
            requests_total: register_int_counter!(
                "fusionrouter_requests_total",
                "Total number of requests"
            )
            .unwrap(),
            request_duration_seconds: register_histogram_vec!(
                "fusionrouter_request_duration_seconds",
                "Request duration in seconds",
                &["route"]
            )
            .unwrap(),
            errors_total: register_int_counter!(
                "fusionrouter_errors_total",
                "Total number of errors"
            )
            .unwrap(),
            tokens_total: register_int_counter!(
                "fusionrouter_tokens_total",
                "Total tokens consumed"
            )
            .unwrap(),
            provider_latency_seconds: register_histogram_vec!(
                "fusionrouter_provider_latency_seconds",
                "Provider latency in seconds",
                &["provider"]
            )
            .unwrap(),
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
