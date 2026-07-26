use std::sync::OnceLock;
use prometheus::{GaugeVec, HistogramVec, IntCounterVec};

static CONNECTOR_METRICS: OnceLock<ConnectorMetrics> = OnceLock::new();

pub struct ConnectorMetrics {
    pub health_status: GaugeVec,
    pub check_duration_seconds: HistogramVec,
    pub checks_total: IntCounterVec,
}

fn safe_gauge_vec(name: &str, help: &str, labels: &[&str]) -> GaugeVec {
    let opts = prometheus::Opts::new(name, help);
    let gauge = GaugeVec::new(opts, labels).unwrap();
    let _ = prometheus::default_registry().register(Box::new(gauge.clone()));
    gauge
}

fn safe_histogram_vec(name: &str, help: &str, labels: &[&str]) -> HistogramVec {
    let opts = prometheus::HistogramOpts::new(name, help);
    let hist = HistogramVec::new(opts, labels).unwrap();
    let _ = prometheus::default_registry().register(Box::new(hist.clone()));
    hist
}

fn safe_int_counter_vec(name: &str, help: &str, labels: &[&str]) -> IntCounterVec {
    let opts = prometheus::Opts::new(name, help);
    let counter = IntCounterVec::new(opts, labels).unwrap();
    let _ = prometheus::default_registry().register(Box::new(counter.clone()));
    counter
}

impl ConnectorMetrics {
    pub fn instance() -> &'static Self {
        CONNECTOR_METRICS.get_or_init(Self::new)
    }

    fn new() -> Self {
        Self {
            health_status: safe_gauge_vec(
                "fusionrouter_connector_health_status",
                "Current health status of connectors (1 = healthy, 0 = unhealthy)",
                &["connector_name"],
            ),
            check_duration_seconds: safe_histogram_vec(
                "fusionrouter_connector_check_duration_seconds",
                "Duration of connector health checks in seconds",
                &["connector_name", "status"],
            ),
            checks_total: safe_int_counter_vec(
                "fusionrouter_connector_checks_total",
                "Total number of connector health checks",
                &["connector_name", "status"],
            ),
        }
    }

    pub fn set_healthy(&self, connector_name: &str) {
        self.health_status
            .with_label_values(&[connector_name])
            .set(1.0);
    }

    pub fn set_unhealthy(&self, connector_name: &str) {
        self.health_status
            .with_label_values(&[connector_name])
            .set(0.0);
    }

    pub fn observe_check_duration(&self, connector_name: &str, status: &str, secs: f64) {
        self.check_duration_seconds
            .with_label_values(&[connector_name, status])
            .observe(secs);
    }

    pub fn record_check(&self, connector_name: &str, status: &str) {
        self.checks_total
            .with_label_values(&[connector_name, status])
            .inc();
    }
}

impl Default for ConnectorMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prometheus::Encoder;

    #[test]
    fn test_connector_metrics_instance_is_singleton() {
        let a = ConnectorMetrics::instance();
        let b = ConnectorMetrics::instance();
        assert!(std::ptr::eq(a, b));
    }

    #[test]
    fn test_connector_metrics_health_status() {
        let m = ConnectorMetrics::instance();
        m.set_healthy("test_connector");
        m.set_unhealthy("test_connector");

        let encoder = prometheus::TextEncoder::new();
        let metric_families = prometheus::gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer).unwrap();
        let output = String::from_utf8(buffer).unwrap_or_default();
        assert!(output.contains("fusionrouter_connector_health_status"));
    }

    #[test]
    fn test_connector_metrics_record_check() {
        let m = ConnectorMetrics::instance();
        m.record_check("test_connector", "healthy");
        let val = m.checks_total
            .with_label_values(&["test_connector", "healthy"])
            .get();
        assert_eq!(val, 1);
    }

    #[test]
    fn test_connector_metrics_observe_duration() {
        let m = ConnectorMetrics::instance();
        m.observe_check_duration("test_connector", "healthy", 0.042);

        let encoder = prometheus::TextEncoder::new();
        let metric_families = prometheus::gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer).unwrap();
        let output = String::from_utf8(buffer).unwrap_or_default();
        assert!(output.contains("fusionrouter_connector_check_duration_seconds"));
    }

    #[test]
    fn test_connector_metrics_render_contains_expected() {
        let m = ConnectorMetrics::instance();
        m.set_healthy("test_render");
        m.record_check("test_render", "healthy");
        m.observe_check_duration("test_render", "healthy", 0.1);

        let encoder = prometheus::TextEncoder::new();
        let metric_families = prometheus::gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer).unwrap();
        let output = String::from_utf8(buffer).unwrap_or_default();

        assert!(output.contains("fusionrouter_connector_health_status"));
        assert!(output.contains("fusionrouter_connector_check_duration_seconds"));
        assert!(output.contains("fusionrouter_connector_checks_total"));
    }
}