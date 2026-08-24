use crate::resource::stream_meter::StreamMeterReport;
use prometheus::{Histogram, IntCounter};
use std::sync::OnceLock;

static STREAM_METRICS: OnceLock<StreamMetrics> = OnceLock::new();

pub struct StreamMetrics {
    pub ttfb_seconds: Histogram,
    pub inter_token_latency_seconds: Histogram,
    pub streaming_duration_seconds: Histogram,
    pub streaming_tokens_total: IntCounter,
    pub streaming_requests_total: IntCounter,
    pub streaming_errors_total: IntCounter,
}

fn safe_int_counter(name: &str, help: &str) -> IntCounter {
    let opts = prometheus::Opts::new(name, help);
    let counter = IntCounter::with_opts(opts).expect("valid static counter opts");
    let _ = prometheus::default_registry().register(Box::new(counter.clone()));
    counter
}

fn safe_histogram(name: &str, help: &str, buckets: Vec<f64>) -> Histogram {
    let opts = prometheus::HistogramOpts::new(name, help).buckets(buckets);
    let hist = Histogram::with_opts(opts).expect("valid static histogram opts");
    let _ = prometheus::default_registry().register(Box::new(hist.clone()));
    hist
}

impl StreamMetrics {
    pub fn instance() -> &'static Self {
        STREAM_METRICS.get_or_init(Self::new)
    }

    fn new() -> Self {
        Self {
            ttfb_seconds: safe_histogram(
                "fusionrouter_stream_ttfb_seconds",
                "Time to first byte for streaming responses",
                vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0],
            ),
            inter_token_latency_seconds: safe_histogram(
                "fusionrouter_stream_inter_token_latency_seconds",
                "Time between successive streaming chunks",
                vec![0.01, 0.05, 0.1, 0.5, 1.0],
            ),
            streaming_duration_seconds: safe_histogram(
                "fusionrouter_stream_duration_seconds",
                "Total duration of streaming responses",
                vec![1.0, 5.0, 10.0, 30.0, 60.0, 120.0],
            ),
            streaming_tokens_total: safe_int_counter(
                "fusionrouter_stream_tokens_total",
                "Total tokens streamed across all requests",
            ),
            streaming_requests_total: safe_int_counter(
                "fusionrouter_stream_requests_total",
                "Total number of streaming requests",
            ),
            streaming_errors_total: safe_int_counter(
                "fusionrouter_stream_errors_total",
                "Total number of streaming errors",
            ),
        }
    }

    pub fn record_report(&self, report: &StreamMeterReport) {
        self.streaming_requests_total.inc();
        self.streaming_tokens_total.inc_by(report.total_tokens);
        if let Some(ttfb) = report.ttfb_ms {
            self.ttfb_seconds.observe(ttfb as f64 / 1000.0);
        }
        if let Some(dur) = report.total_duration_ms {
            self.streaming_duration_seconds.observe(dur as f64 / 1000.0);
        }
    }

    pub fn record_request(&self) {
        self.streaming_requests_total.inc();
    }

    pub fn record_error(&self) {
        self.streaming_errors_total.inc();
    }

    pub fn record_tokens(&self, count: u64) {
        self.streaming_tokens_total.inc_by(count);
    }

    pub fn observe_ttfb(&self, secs: f64) {
        self.ttfb_seconds.observe(secs);
    }

    pub fn observe_inter_token_latency(&self, secs: f64) {
        self.inter_token_latency_seconds.observe(secs);
    }

    pub fn observe_duration(&self, secs: f64) {
        self.streaming_duration_seconds.observe(secs);
    }
}

impl Default for StreamMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prometheus::Encoder;

    #[test]
    fn test_stream_metrics_instance_is_singleton() {
        let a = StreamMetrics::instance();
        let b = StreamMetrics::instance();
        assert!(std::ptr::eq(a, b));
    }

    #[test]
    fn test_stream_metrics_record_request() {
        let m = StreamMetrics::instance();
        let before = m.streaming_requests_total.get();
        m.record_request();
        assert_eq!(m.streaming_requests_total.get(), before + 1);
    }

    #[test]
    fn test_stream_metrics_record_error() {
        let m = StreamMetrics::instance();
        let before = m.streaming_errors_total.get();
        m.record_error();
        assert_eq!(m.streaming_errors_total.get(), before + 1);
    }

    #[test]
    fn test_stream_metrics_record_tokens() {
        let m = StreamMetrics::instance();
        let before = m.streaming_tokens_total.get();
        m.record_tokens(42);
        assert_eq!(m.streaming_tokens_total.get(), before + 42);
    }

    #[test]
    fn test_stream_metrics_record_report_increments_counters() {
        let m = StreamMetrics::instance();
        let req_before = m.streaming_requests_total.get();
        let tok_before = m.streaming_tokens_total.get();

        let report = StreamMeterReport {
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
            cost: crate::types::NanoUSD::from_nanos(5_000_000),
            ttfb_ms: Some(200),
            total_duration_ms: Some(5000),
        };
        m.record_report(&report);

        assert_eq!(m.streaming_requests_total.get(), req_before + 1);
        assert_eq!(m.streaming_tokens_total.get(), tok_before + 30);
    }

    #[test]
    fn test_stream_metrics_record_report_without_timing() {
        let m = StreamMetrics::instance();
        let report = StreamMeterReport {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 5,
            cost: crate::types::NanoUSD::ZERO,
            ttfb_ms: None,
            total_duration_ms: None,
        };
        m.record_report(&report);
        assert!(m.streaming_tokens_total.get() >= 5);
    }

    #[test]
    fn test_stream_metrics_render_contains_expected() {
        let _ = StreamMetrics::instance();
        let encoder = prometheus::TextEncoder::new();
        let metric_families = prometheus::gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer).unwrap();
        let output = String::from_utf8(buffer).unwrap_or_default();

        assert!(output.contains("fusionrouter_stream_ttfb_seconds"));
        assert!(output.contains("fusionrouter_stream_inter_token_latency_seconds"));
        assert!(output.contains("fusionrouter_stream_duration_seconds"));
        assert!(output.contains("fusionrouter_stream_tokens_total"));
        assert!(output.contains("fusionrouter_stream_requests_total"));
        assert!(output.contains("fusionrouter_stream_errors_total"));
    }

    #[test]
    fn test_stream_metrics_observe_histograms() {
        let m = StreamMetrics::instance();
        m.observe_ttfb(0.5);
        m.observe_inter_token_latency(0.05);
        m.observe_duration(2.0);

        let encoder = prometheus::TextEncoder::new();
        let metric_families = prometheus::gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer).unwrap();
        let output = String::from_utf8(buffer).unwrap_or_default();

        assert!(output.contains("fusionrouter_stream_ttfb_seconds"));
        assert!(output.contains("fusionrouter_stream_inter_token_latency_seconds"));
        assert!(output.contains("fusionrouter_stream_duration_seconds"));
    }
}
