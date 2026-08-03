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

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Duration {
        let clamped = self.attempt.min(30);
        let exp = self.base_ms.saturating_mul(1 << clamped);
        let cap = exp.min(self.max_ms);
        let jittered = if cap > 0 { rand::random::<u64>() % cap } else { 0 };
        self.attempt += 1;
        Duration::from_millis(jittered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_starts_at_zero_attempts() {
        let backoff = Backoff::new(100, 1000);
        assert_eq!(backoff.attempt, 0);
    }

    #[test]
    fn test_next_bounded_by_cap() {
        let mut backoff = Backoff::new(10_000, 100);
        for _ in 0..10 {
            let delay = backoff.next();
            assert!(
                delay.as_millis() < 100,
                "delay must stay below max_ms cap"
            );
        }
    }

    #[test]
    fn test_next_grows_exponentially_until_cap() {
        let mut backoff = Backoff::new(100, 10_000);

        let d1 = backoff.next().as_millis();
        let d2 = backoff.next().as_millis();
        let d3 = backoff.next().as_millis();

        assert!(d1 < 100);
        assert!(d2 < 200);
        assert!(d3 < 400);
    }

    #[test]
    fn test_next_clamps_exponent_at_30() {
        let mut backoff = Backoff::new(1, u64::MAX);

        let mut max_delay = 0u128;
        for _ in 0..40 {
            let delay = backoff.next().as_millis();
            max_delay = max_delay.max(delay);
        }

        assert_eq!(backoff.attempt, 40);
        assert!(
            max_delay < (1u128 << 30),
            "delay must stay below 2^30 ms even after 40 attempts"
        );
    }

    #[test]
    fn test_next_with_zero_base_returns_zero() {
        let mut backoff = Backoff::new(0, 1000);
        assert_eq!(backoff.next(), Duration::from_millis(0));
    }

    #[test]
    fn test_reset_restarts_sequence() {
        let mut backoff = Backoff::new(100, 10_000);
        backoff.next();
        backoff.next();
        backoff.reset();

        assert_eq!(backoff.attempt, 0);
        assert!(backoff.next().as_millis() < 100);
    }
}
