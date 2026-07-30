use async_trait::async_trait;

#[async_trait]
pub trait CapabilityHostServices: Send + Sync {
    async fn log(&self, level: tracing::Level, message: &str);
}
