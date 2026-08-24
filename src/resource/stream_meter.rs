use std::time::Instant;
use crate::providers::ModelPricing;
use crate::types::{ChatStreamChunk, NanoUSD};

#[derive(Debug, Clone)]
pub struct StreamMeter {
    prompt_tokens: u64,
    completion_tokens: u64,
    first_chunk_at: Option<Instant>,
    last_chunk_at: Option<Instant>,
    stream_started_at: Instant,
    cost: NanoUSD,
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
            cost: NanoUSD::ZERO,
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
            cost: self.cost,
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

    pub fn cost(&self) -> NanoUSD {
        self.cost
    }

    fn update_cost(&mut self, pricing: Option<&ModelPricing>) {
        if let Some(p) = pricing {
            let prompt_nanos = nanos_for_tokens(self.prompt_tokens, p.input_cost_per_1k);
            let completion_nanos = nanos_for_tokens(self.completion_tokens, p.output_cost_per_1k);
            self.cost = NanoUSD::from_nanos(prompt_nanos + completion_nanos);
        }
    }
}

fn nanos_for_tokens(tokens: u64, nanos_per_1k_tokens: NanoUSD) -> u64 {
    let product = (tokens as u128) * (nanos_per_1k_tokens.as_nanos() as u128);
    ((product + 500) / 1_000).min(u64::MAX as u128) as u64
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct StreamMeterReport {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub cost: NanoUSD,
    pub ttfb_ms: Option<u64>,
    pub total_duration_ms: Option<u64>,
}

fn estimate_tokens(s: &str) -> u64 {
    let char_count = s.chars().count() as u64;
    char_count.div_ceil(4)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Usage;

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("a"), 1);
        assert_eq!(estimate_tokens("hello world"), 3);
    }

    #[test]
    fn test_record_chunk_accumulates_content() {
        let mut meter = StreamMeter::new();
        let chunk1 = ChatStreamChunk {
            content: Some("Hello ".to_string()),
            finish_reason: None,
            usage: None,
        };
        let chunk2 = ChatStreamChunk {
            content: Some("world!".to_string()),
            finish_reason: None,
            usage: None,
        };
        meter.record_chunk(&chunk1, None);
        assert_eq!(meter.completion_tokens(), 2);
        meter.record_chunk(&chunk2, None);
        assert_eq!(meter.completion_tokens(), 4);
    }

    #[test]
    fn test_first_and_last_chunk_timing() {
        let mut meter = StreamMeter::new();
        assert!(meter.first_chunk_at.is_none());
        assert!(meter.last_chunk_at.is_none());

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
            input_cost_per_1k: NanoUSD::from_nanos(500_000_000_000),
            output_cost_per_1k: NanoUSD::from_nanos(500_000_000_000),
        };
        meter.record_chunk(&chunk, Some(&pricing));
        let report = meter.finalize(Some(&pricing));
        assert!(report.cost > NanoUSD::ZERO);
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
            input_cost_per_1k: NanoUSD::from_nanos(500_000_000_000),
            output_cost_per_1k: NanoUSD::from_nanos(500_000_000_000),
        };
        meter.record_chunk(&chunk, Some(&pricing));
        let report1 = meter.finalize(Some(&pricing));
        let report2 = meter.finalize(Some(&pricing));
        assert_eq!(report1.completion_tokens, report2.completion_tokens);
        assert_eq!(report1.total_tokens, report2.total_tokens);
        assert_eq!(report1.cost, report2.cost);
    }
}
