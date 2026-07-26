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
