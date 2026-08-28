use parking_lot::RwLock;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

pub struct CircuitBreaker {
    failure_threshold: AtomicU32,
    success_threshold: u32,
    cooldown_secs: AtomicU64,
    state: RwLock<CircuitState>,
    failure_count: AtomicU32,
    success_count: AtomicU32,
    last_failure_time: RwLock<Option<Instant>>,
    probe_in_flight: AtomicBool,
}

impl CircuitBreaker {
    pub fn new(failure_threshold: u32, success_threshold: u32, cooldown_secs: u64) -> Self {
        Self {
            failure_threshold: AtomicU32::new(failure_threshold),
            success_threshold,
            cooldown_secs: AtomicU64::new(cooldown_secs),
            state: RwLock::new(CircuitState::Closed),
            failure_count: AtomicU32::new(0),
            success_count: AtomicU32::new(0),
            last_failure_time: RwLock::new(None),
            probe_in_flight: AtomicBool::new(false),
        }
    }

    pub fn can_execute(&self) -> bool {
        let state = *self.state.read();
        match state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                if let Some(last_failure) = *self.last_failure_time.read() {
                    let cooldown = Duration::from_secs(self.cooldown_secs.load(Ordering::Relaxed));
                    if last_failure.elapsed() >= cooldown {
                        *self.state.write() = CircuitState::HalfOpen;
                        self.success_count.store(0, Ordering::Relaxed);
                        self.probe_in_flight.store(true, Ordering::Release);
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => self
                .probe_in_flight
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok(),
        }
    }

    pub fn record_success(&self) {
        let state = *self.state.read();
        match state {
            CircuitState::HalfOpen => {
                let count = self.success_count.fetch_add(1, Ordering::Relaxed) + 1;
                if count >= self.success_threshold {
                    *self.state.write() = CircuitState::Closed;
                    self.failure_count.store(0, Ordering::Relaxed);
                }
                self.probe_in_flight.store(false, Ordering::Release);
            }
            CircuitState::Closed => {
                self.failure_count.store(0, Ordering::Relaxed);
            }
            _ => {}
        }
    }

    pub fn record_failure(&self) {
        let state = *self.state.read();
        match state {
            CircuitState::HalfOpen => {
                *self.state.write() = CircuitState::Open;
                *self.last_failure_time.write() = Some(Instant::now());
                self.probe_in_flight.store(false, Ordering::Release);
            }
            CircuitState::Closed => {
                let count = self.failure_count.fetch_add(1, Ordering::Relaxed) + 1;
                if count >= self.failure_threshold.load(Ordering::Relaxed) {
                    *self.state.write() = CircuitState::Open;
                    *self.last_failure_time.write() = Some(Instant::now());
                }
            }
            _ => {}
        }
    }

    pub fn update_thresholds(&self, failure_threshold: u32, cooldown_secs: u64) {
        self.failure_threshold
            .store(failure_threshold, Ordering::Relaxed);
        self.cooldown_secs.store(cooldown_secs, Ordering::Relaxed);
    }

    pub fn state(&self) -> CircuitState {
        *self.state.read()
    }

    pub fn reset(&self) {
        *self.state.write() = CircuitState::Closed;
        self.failure_count.store(0, Ordering::Relaxed);
        self.success_count.store(0, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_starts_closed() {
        let cb = CircuitBreaker::new(3, 2, 5);
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.can_execute());
    }

    #[test]
    fn test_opens_after_threshold() {
        let cb = CircuitBreaker::new(3, 2, 5);
        cb.record_failure();
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.can_execute());
    }

    #[test]
    fn test_success_resets_in_closed() {
        let cb = CircuitBreaker::new(3, 2, 5);
        cb.record_failure();
        cb.record_success();
        assert_eq!(cb.failure_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_update_thresholds_affects_behavior() {
        let cb = CircuitBreaker::new(3, 2, 30);
        cb.update_thresholds(5, 30);
        for _ in 0..3 {
            cb.record_failure();
        }
        assert!(cb.can_execute(), "should still be closed (threshold is 5)");
        for _ in 0..2 {
            cb.record_failure();
        }
        assert!(!cb.can_execute(), "should be open after 5 failures");
    }
}
