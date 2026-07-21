#[cfg(feature = "otel")]
pub fn init_tracing() -> anyhow::Result<()> {
    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry_otlp::{SpanExporter, WithExportConfig};
    use opentelemetry_sdk::trace::TracerProvider;
    use opentelemetry_sdk::Resource;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::Registry;

    let exporter = SpanExporter::builder()
        .with_tonic()
        .with_endpoint(
            std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:4317".to_string()),
        )
        .build()?;

    let provider = TracerProvider::builder()
        .with_batch_exporter(exporter, opentelemetry_sdk::runtime::Tokio)
        .with_resource(Resource::new(vec![opentelemetry::KeyValue::new(
            "service.name",
            "fusion-router",
        )]))
        .build();

    let tracer = provider.tracer("fusion-router");
    let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);
    let subscriber = Registry::default().with(telemetry);
    tracing::subscriber::set_global_default(subscriber)?;
    Ok(())
}

#[cfg(not(feature = "otel"))]
pub fn init_tracing() -> anyhow::Result<()> {
    Ok(())
}

#[cfg(feature = "dev-console")]
pub fn init_console() {
    console_subscriber::init();
}

#[cfg(not(feature = "dev-console"))]
pub fn init_console() {}
