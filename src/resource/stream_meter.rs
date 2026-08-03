use std::time::Instant;
use crate::providers::ModelPricing;
use crate::types::ChatStreamChunk;

#[derive(Debug, Clone)]
pub struct StreamMeter {
    prompt_tokens: u64,
    completion_tokens: u64,
    first_chunk_at: Option<Instant>,
    last_chunk_at: Option<Instant>,
    stream_started_at: Instant,
    cost_millicosts: u64,
    finalized: bool,
}

impl StreamMeter {
    pub fn new() -> Self {
        Self {
            prompt_tokens: 0,
            completion_tokens: 0,
            first_chunk_at: None,
            last_chunk_at: None,
            stream_started_at: Instant::now(),
            cost_millicosts: 0,
            finalized: false,
        }
    }
}

impl Default for StreamMeter {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamMeter {
    pub fn record_chunk(&mut self, chunk: &ChatStreamChunk, pricing: Option<&ModelPricing>) {
        if let Some(ref usage) = chunk.usage {
            self.prompt_tokens = usage.prompt_tokens as u64;
            self.completion_tokens = usage.completion_tokens as u64;
        } else if let Some(ref content) = chunk.content {
            self.completion_tokens += estimate_tokens(content);
        }

        let now = Instant::now();
        if self.first_chunk_at.is_none() {
            self.first_chunk_at = Some(now);
        }
        self.last_chunk_at = Some(now);

        self.update_cost(pricing);
    }

    pub fn finalize(&mut self, pricing: Option<&ModelPricing>) -> StreamMeterReport {
        if !self.finalized {
            self.update_cost(pricing);
            self.finalized = true;
        }

        let ttfb_ms = self
            .first_chunk_at
            .map(|t| t.duration_since(self.stream_started_at).as_millis() as u64);
        let total_duration_ms = self
            .last_chunk_at
            .map(|t| t.duration_since(self.stream_started_at).as_millis() as u64);

        StreamMeterReport {
            prompt_tokens: self.prompt_tokens,
            completion_tokens: self.completion_tokens,
            total_tokens: self.prompt_tokens + self.completion_tokens,
            cost_millicosts: self.cost_millicosts,
            ttfb_ms,
            total_duration_ms,
        }
    }

    pub fn prompt_tokens(&self) -> u64 {
        self.prompt_tokens
    }

    pub fn completion_tokens(&self) -> u64 {
        self.completion_tokens
    }

    pub fn cost_millicosts(&self) -> u64 {
        self.cost_millicosts
    }

    fn update_cost(&mut self, pricing: Option<&ModelPricing>) {
        if let Some(p) = pricing {
            let prompt_cost =
                (self.prompt_tokens as f64 / 1_000_000.0 * p.input_cost_per_1k * 1000.0) as u64;
            let completion_cost =
                (self.completion_tokens as f64 / 1_000_000.0 * p.output_cost_per_1k * 1000.0) as u64;
            self.cost_millicosts = prompt_cost + completion_cost;
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct StreamMeterReport {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub cost_millicosts: u64,
    pub ttfb_ms: Option<u64>,
    pub total_duration_ms: Option<u64>,
}

fn estimate_tokens(s: &str) -> u64 {
    (s.len() as f64 / 4.0).ceil() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Usage;

    #[test]
    fn test_initial_state() {
        let meter = StreamMeter::new();
        assert_eq!(meter.prompt_tokens(), 0);
        assert_eq!(meter.completion_tokens(), 0);
        assert!(meter.first_chunk_at.is_none());
        assert!(meter.last_chunk_at.is_none());
    }

    #[test]
    fn test_record_chunk_accumulates_tokens() {
        let mut meter = StreamMeter::new();
        let chunk = ChatStreamChunk {
            content: Some("Hello world".to_string()),
            finish_reason: None,
            usage: None,
        };
        meter.record_chunk(&chunk, None);
        assert_eq!(meter.completion_tokens(), 3);
        assert_eq!(meter.prompt_tokens(), 0);
    }

    #[test]
    fn test_record_chunk_sets_first_chunk_at() {
        let mut meter = StreamMeter::new();
        let chunk = ChatStreamChunk {
            content: Some("a".to_string()),
            finish_reason: None,
            usage: None,
        };
        meter.record_chunk(&chunk, None);
        let first = meter.first_chunk_at;
        assert!(first.is_some());
        std::thread::sleep(std::time::Duration::from_millis(1));
        meter.record_chunk(&chunk, None);
        assert_eq!(meter.first_chunk_at, first);
        assert!(meter.last_chunk_at.unwrap() > first.unwrap());
    }

    #[test]
    fn test_finalize_returns_report() {
        let mut meter = StreamMeter::new();
        let chunk = ChatStreamChunk {
            content: Some("Hello beautiful world".to_string()),
            finish_reason: None,
            usage: None,
        };
        meter.record_chunk(&chunk, None);
        let report = meter.finalize(None);
        assert_eq!(report.completion_tokens, 6);
        assert_eq!(report.prompt_tokens, 0);
        assert_eq!(report.total_tokens, 6);
        assert!(report.ttfb_ms.is_some());
        assert!(report.total_duration_ms.is_some());
    }

    #[test]
    fn test_finalize_sets_cost() {
        let mut meter = StreamMeter::new();
        let chunk = ChatStreamChunk {
            content: Some("Hello world".to_string()),
            finish_reason: None,
            usage: None,
        };
        let pricing = ModelPricing {
            input_cost_per_1k: 500.0,
            output_cost_per_1k: 500.0,
        };
        meter.record_chunk(&chunk, Some(&pricing));
        let report = meter.finalize(Some(&pricing));
        assert!(report.cost_millicosts > 0);
    }

    #[test]
    fn test_usage_from_final_chunk() {
        let mut meter = StreamMeter::new();
        let content_chunk = ChatStreamChunk {
            content: Some("Hello".to_string()),
            finish_reason: None,
            usage: None,
        };
        meter.record_chunk(&content_chunk, None);
        let final_chunk = ChatStreamChunk {
            content: None,
            finish_reason: Some("stop".to_string()),
            usage: Some(Usage {
                prompt_tokens: 50,
                completion_tokens: 150,
                total_tokens: 200,
            }),
        };
        meter.record_chunk(&final_chunk, None);
        assert_eq!(meter.prompt_tokens(), 50);
        assert_eq!(meter.completion_tokens(), 150);
    }

    #[test]
    fn test_empty_content_zero_tokens() {
        let mut meter = StreamMeter::new();
        let chunk = ChatStreamChunk {
            content: Some("".to_string()),
            finish_reason: None,
            usage: None,
        };
        meter.record_chunk(&chunk, None);
        assert_eq!(meter.completion_tokens(), 0);
    }

    #[test]
    fn test_two_phase() {
        let mut meter = StreamMeter::new();
        let chunk = ChatStreamChunk {
            content: Some("test".to_string()),
            finish_reason: None,
            usage: None,
        };
        let pricing = ModelPricing {
            input_cost_per_1k: 500.0,
            output_cost_per_1k: 500.0,
        };
        meter.record_chunk(&chunk, Some(&pricing));
        let report1 = meter.finalize(Some(&pricing));
        let report2 = meter.finalize(Some(&pricing));
        assert_eq!(report1.completion_tokens, report2.completion_tokens);
        assert_eq!(report1.total_tokens, report2.total_tokens);
        assert_eq!(report1.cost_millicosts, report2.cost_millicosts);
    }
}
