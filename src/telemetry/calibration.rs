use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::providers::ProviderRegistry;
use crate::telemetry::EvidenceRepository;

#[derive(Debug, Clone)]
pub struct CalibratorConfig {
    pub min_sample_size: u64,
    pub smoothing_factor: f32,
    pub window_hours: u32,
    pub target_success_rate: f64,
    pub min_score_floor: f32,
}

impl Default for CalibratorConfig {
    fn default() -> Self {
        Self {
            min_sample_size: 30,
            smoothing_factor: 0.2,
            window_hours: 24,
            target_success_rate: 0.95,
            min_score_floor: 0.1,
        }
    }
}

pub struct FeedbackCalibrator {
    repo: Arc<dyn EvidenceRepository>,
    registry: Arc<ProviderRegistry>,
    config: CalibratorConfig,
    base_capabilities: parking_lot::RwLock<std::collections::HashMap<String, crate::providers::ModelCapabilities>>,
}

impl FeedbackCalibrator {
    pub fn new(
        repo: Arc<dyn EvidenceRepository>,
        registry: Arc<ProviderRegistry>,
        config: CalibratorConfig,
    ) -> Self {
        Self {
            repo,
            registry,
            config,
            base_capabilities: parking_lot::RwLock::new(std::collections::HashMap::new()),
        }
    }

    pub fn set_baseline(&self, model: &str, caps: crate::providers::ModelCapabilities) {
        self.base_capabilities.write().insert(model.to_string(), caps);
    }

    pub async fn calibrate_once(&self) -> anyhow::Result<usize> {
        let stats = self.repo.get_model_stats(self.config.window_hours).await?;
        let mut updated_count = 0;

        for stat in stats {
            if stat.total_requests < self.config.min_sample_size {
                info!(
                    model = %stat.model,
                    samples = stat.total_requests,
                    min = self.config.min_sample_size,
                    "Skipping calibration for model due to insufficient samples"
                );
                continue;
            }

            let current_caps = match self.registry.get_capabilities(&stat.model) {
                Some(caps) => caps,
                None => continue,
            };

            let base_caps = {
                let mut bases = self.base_capabilities.write();
                bases.entry(stat.model.clone()).or_insert(current_caps.clone()).clone()
            };

            let success_rate = if stat.total_requests > 0 {
                stat.success_count as f64 / stat.total_requests as f64
            } else {
                1.0
            };

            let health_factor = if success_rate >= self.config.target_success_rate {
                1.0f32
            } else {
                ((success_rate / self.config.target_success_rate) as f32).max(self.config.min_score_floor)
            };

            let target_coding = (base_caps.coding_score * health_factor).max(self.config.min_score_floor);
            let target_reasoning = (base_caps.reasoning_score * health_factor).max(self.config.min_score_floor);

            let alpha = self.config.smoothing_factor;
            let new_coding = alpha * target_coding + (1.0 - alpha) * current_caps.coding_score;
            let new_reasoning = alpha * target_reasoning + (1.0 - alpha) * current_caps.reasoning_score;

            let mut new_caps = current_caps.clone();
            new_caps.coding_score = new_coding;
            new_caps.reasoning_score = new_reasoning;

            self.registry.update_capabilities(&stat.model, new_caps);
            updated_count += 1;

            info!(
                model = %stat.model,
                success_rate = %success_rate,
                health_factor = %health_factor,
                new_coding = %new_coding,
                new_reasoning = %new_reasoning,
                "Calibrated model capabilities"
            );
        }

        Ok(updated_count)
    }
}

pub fn spawn_calibration_loop(
    calibrator: Arc<FeedbackCalibrator>,
    interval: Duration,
    cancel_token: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut timer = tokio::time::interval(interval);
        loop {
            tokio::select! {
                _ = cancel_token.cancelled() => {
                    info!("Feedback calibration loop shutting down");
                    break;
                }
                _ = timer.tick() => {
                    if let Err(e) = calibrator.calibrate_once().await {
                        warn!(error = %e, "Feedback calibration loop iteration failed");
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::circuit_breaker::CircuitBreaker;
    use crate::providers::router::ProviderTarget;
    use crate::providers::{ChatProvider, ModelCapabilities, ModelPricing};
    use crate::telemetry::SqliteEvidenceRepository;
    use crate::types::{ChatCompletionRequest, ChatCompletionResponse, ChatMessage, Choice, ExecutionRecord, Intent};
    use uuid::Uuid;

    struct DummyProvider;
    #[async_trait::async_trait]
    impl ChatProvider for DummyProvider {
        fn name(&self) -> &str { "dummy" }
        async fn chat_completion(&self, _req: &ChatCompletionRequest) -> anyhow::Result<ChatCompletionResponse> {
            Ok(ChatCompletionResponse {
                id: "dummy".into(), object: "chat.completion".into(), created: 0, model: "dummy".into(),
                choices: vec![Choice { index: 0, message: ChatMessage { role: "assistant".into(), content: "dummy".into() }, finish_reason: "stop".into() }],
                native_tool_calls: None,
                usage: None,
            })
        }
    }

    fn dummy_target(name: &str) -> ProviderTarget {
        ProviderTarget::new(
            name.into(),
            CircuitBreaker::new(3, 2, 5),
            Box::new(|| Arc::new(DummyProvider)),
        )
    }

    fn sample_caps() -> ModelCapabilities {
        ModelCapabilities {
            coding_score: 0.9,
            reasoning_score: 0.9,
            max_context_tokens: 128_000,
            max_output_tokens: 0,
            supports_tools: true,
            supports_streaming: true,
            supports_vision: false,
            supports_audio: false,
            supports_pdf: false,
            supports_json_mode: true,
            supports_thinking: false,
            supports_parallel_tools: false,
            supports_structured_output: false,
        }
    }

    fn sample_pricing() -> ModelPricing {
        ModelPricing { input_cost_per_1k: 1.0, output_cost_per_1k: 2.0 }
    }

    async fn record_executions(repo: &SqliteEvidenceRepository, model: &str, total: u64, successes: u64) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        for i in 0..total {
            let record = ExecutionRecord {
                record_id: Uuid::new_v4(),
                plan_id: Uuid::new_v4(),
                node_id: Uuid::new_v4(),
                model: model.to_string(),
                provider: "test".to_string(),
                intent: Intent::General,
                latency_ms: 100,
                tokens: 50,
                cost: 0.001,
                success: i < successes,
                timestamp: now,
            };
            repo.record(record).await.unwrap();
        }
    }

    #[tokio::test]
    async fn test_calibration_cold_start_skipped() {
        let repo = Arc::new(SqliteEvidenceRepository::new(":memory:").unwrap());
        let registry = Arc::new(ProviderRegistry::new(dummy_target("default")));
        registry.register_target_with_capabilities(
            vec!["test/".into()],
            dummy_target("test-model"),
            sample_caps(),
            sample_pricing(),
        );

        // Record only 10 executions (< min_sample_size = 30)
        record_executions(&repo, "test-model", 10, 5).await;

        let calibrator = FeedbackCalibrator::new(
            repo,
            registry.clone(),
            CalibratorConfig { min_sample_size: 30, ..Default::default() },
        );

        let count = calibrator.calibrate_once().await.unwrap();
        assert_eq!(count, 0);

        // Capabilities should be unchanged
        let caps = registry.get_capabilities("test-model").unwrap();
        assert_eq!(caps.coding_score, 0.9);
    }

    #[tokio::test]
    async fn test_calibration_penalizes_low_success_rate() {
        let repo = Arc::new(SqliteEvidenceRepository::new(":memory:").unwrap());
        let registry = Arc::new(ProviderRegistry::new(dummy_target("default")));
        registry.register_target_with_capabilities(
            vec!["test/".into()],
            dummy_target("test-model"),
            sample_caps(),
            sample_pricing(),
        );

        // Record 40 executions with 50% success rate (20/40)
        record_executions(&repo, "test-model", 40, 20).await;

        let calibrator = FeedbackCalibrator::new(
            repo,
            registry.clone(),
            CalibratorConfig {
                min_sample_size: 30,
                smoothing_factor: 1.0, // Instant update without EMA delay for test
                target_success_rate: 0.95,
                min_score_floor: 0.1,
                window_hours: 24,
            },
        );
        calibrator.set_baseline("test-model", sample_caps());

        let count = calibrator.calibrate_once().await.unwrap();
        assert_eq!(count, 1);

        let caps = registry.get_capabilities("test-model").unwrap();
        // health_factor = 0.5 / 0.95 = 0.5263
        // target_coding = 0.9 * 0.5263 = ~0.4736
        assert!(caps.coding_score < 0.9);
        assert!(caps.coding_score >= 0.1);
    }

    #[tokio::test]
    async fn test_calibration_recovers_on_improved_metrics() {
        let repo = Arc::new(SqliteEvidenceRepository::new(":memory:").unwrap());
        let registry = Arc::new(ProviderRegistry::new(dummy_target("default")));
        registry.register_target_with_capabilities(
            vec!["test/".into()],
            dummy_target("test-model"),
            sample_caps(),
            sample_pricing(),
        );

        // 1. Degrade model
        record_executions(&repo, "test-model", 40, 20).await;
        let calibrator = FeedbackCalibrator::new(
            repo.clone(),
            registry.clone(),
            CalibratorConfig {
                min_sample_size: 30,
                smoothing_factor: 1.0,
                target_success_rate: 0.95,
                min_score_floor: 0.1,
                window_hours: 24,
            },
        );
        calibrator.set_baseline("test-model", sample_caps());
        calibrator.calibrate_once().await.unwrap();

        let degraded = registry.get_capabilities("test-model").unwrap().coding_score;

        // 2. Add 100 successful requests so success rate becomes high
        record_executions(&repo, "test-model", 100, 100).await;
        calibrator.calibrate_once().await.unwrap();

        let recovered = registry.get_capabilities("test-model").unwrap().coding_score;
        assert!(recovered > degraded);
    }
}
