use std::time::Duration;

pub struct Backoff {
    base_ms: u64,
    max_ms: u64,
    attempt: u32,
}

impl Backoff {
    pub fn new(base_ms: u64, max_ms: u64) -> Self {
        Self { base_ms, max_ms, attempt: 0 }
    }

    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    pub fn next(&mut self) -> Duration {
        let clamped = self.attempt.min(30);
        let exp = self.base_ms.saturating_mul(1 << clamped);
        let cap = exp.min(self.max_ms);
        let jittered = if cap > 0 { rand::random::<u64>() % cap } else { 0 };
        self.attempt += 1;
        Duration::from_millis(jittered)
    }
}
