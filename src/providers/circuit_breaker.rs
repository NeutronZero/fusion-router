use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
use parking_lot::RwLock;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

pub struct CircuitBreaker {
    failure_threshold: u32,
    success_threshold: u32,
    cooldown_duration: Duration,
    state: RwLock<CircuitState>,
    failure_count: AtomicU32,
    success_count: AtomicU32,
    last_failure_time: RwLock<Option<Instant>>,
}

impl CircuitBreaker {
    pub fn new(failure_threshold: u32, success_threshold: u32, cooldown_secs: u64) -> Self {
        Self {
            failure_threshold,
            success_threshold,
            cooldown_duration: Duration::from_secs(cooldown_secs),
            state: RwLock::new(CircuitState::Closed),
            failure_count: AtomicU32::new(0),
            success_count: AtomicU32::new(0),
            last_failure_time: RwLock::new(None),
        }
    }

    pub fn can_execute(&self) -> bool {
        let state = *self.state.read();
        match state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                if let Some(last_failure) = *self.last_failure_time.read() {
                    if last_failure.elapsed() >= self.cooldown_duration {
                        *self.state.write() = CircuitState::HalfOpen;
                        self.success_count.store(0, Ordering::Relaxed);
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => true,
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
            }
            CircuitState::Closed => {
                let count = self.failure_count.fetch_add(1, Ordering::Relaxed) + 1;
                if count >= self.failure_threshold {
                    *self.state.write() = CircuitState::Open;
                    *self.last_failure_time.write() = Some(Instant::now());
                }
            }
            _ => {}
        }
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
}
