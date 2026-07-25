use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug)]
pub struct BudgetEnvelope {
    pub max_cost_millicosts: u64,
    pub max_tokens: u64,
    pub max_iterations: u32,
    spent_cost_millicosts: Arc<AtomicU64>,
    spent_tokens: Arc<AtomicU64>,
    current_iterations: Arc<AtomicU64>,
}

impl BudgetEnvelope {
    pub fn new(max_cost_millicosts: u64, max_tokens: u64, max_iterations: u32) -> Self {
        Self {
            max_cost_millicosts,
            max_tokens,
            max_iterations,
            spent_cost_millicosts: Arc::new(AtomicU64::new(0)),
            spent_tokens: Arc::new(AtomicU64::new(0)),
            current_iterations: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn record_and_check(&self, cost_millicosts: u64, tokens: u64) -> Result<(), BudgetExceededError> {
        let prev_cost = self.spent_cost_millicosts.fetch_add(cost_millicosts, Ordering::SeqCst);
        let new_cost = prev_cost + cost_millicosts;
        let prev_tokens = self.spent_tokens.fetch_add(tokens, Ordering::SeqCst);
        let new_tokens = prev_tokens + tokens;

        if new_cost > self.max_cost_millicosts {
            return Err(BudgetExceededError::Cost {
                spent: new_cost,
                max: self.max_cost_millicosts,
            });
        }
        if new_tokens > self.max_tokens {
            return Err(BudgetExceededError::Tokens {
                spent: new_tokens,
                max: self.max_tokens,
            });
        }
        Ok(())
    }

    pub fn increment_iteration(&self) -> Result<u64, BudgetExceededError> {
        let iter = self.current_iterations.fetch_add(1, Ordering::SeqCst) + 1;
        if iter > self.max_iterations as u64 {
            return Err(BudgetExceededError::Iterations {
                current: iter,
                max: self.max_iterations,
            });
        }
        Ok(iter)
    }

    pub fn spent_cost_millicosts(&self) -> u64 {
        self.spent_cost_millicosts.load(Ordering::Acquire)
    }

    pub fn spent_tokens(&self) -> u64 {
        self.spent_tokens.load(Ordering::Acquire)
    }

    pub fn current_iterations(&self) -> u64 {
        self.current_iterations.load(Ordering::Acquire)
    }
}

impl Clone for BudgetEnvelope {
    fn clone(&self) -> Self {
        Self {
            max_cost_millicosts: self.max_cost_millicosts,
            max_tokens: self.max_tokens,
            max_iterations: self.max_iterations,
            spent_cost_millicosts: self.spent_cost_millicosts.clone(),
            spent_tokens: self.spent_tokens.clone(),
            current_iterations: self.current_iterations.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BudgetExceededError {
    Cost { spent: u64, max: u64 },
    Tokens { spent: u64, max: u64 },
    Iterations { current: u64, max: u32 },
}

impl std::fmt::Display for BudgetExceededError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cost { spent, max } => write!(f, "Cost budget exceeded: {} millicosts spent, {} max", spent, max),
            Self::Tokens { spent, max } => write!(f, "Token budget exceeded: {} tokens spent, {} max", spent, max),
            Self::Iterations { current, max } => write!(f, "Iteration budget exceeded: {} iterations, {} max", current, max),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_within_budget() {
        let env = BudgetEnvelope::new(1000, 100, 5);
        assert!(env.record_and_check(500, 30).is_ok());
        assert_eq!(env.spent_cost_millicosts(), 500);
        assert_eq!(env.spent_tokens(), 30);
    }

    #[test]
    fn test_record_exceeds_cost() {
        let env = BudgetEnvelope::new(1000, 100, 5);
        assert!(env.record_and_check(600, 30).is_ok());
        let err = env.record_and_check(500, 30).unwrap_err();
        assert_eq!(err, BudgetExceededError::Cost { spent: 1100, max: 1000 });
    }

    #[test]
    fn test_record_exceeds_tokens() {
        let env = BudgetEnvelope::new(1000, 100, 5);
        assert!(env.record_and_check(100, 80).is_ok());
        let err = env.record_and_check(100, 30).unwrap_err();
        assert_eq!(err, BudgetExceededError::Tokens { spent: 110, max: 100 });
    }

    #[test]
    fn test_increment_iteration_within_limit() {
        let env = BudgetEnvelope::new(1000, 100, 3);
        assert_eq!(env.increment_iteration().unwrap(), 1);
        assert_eq!(env.increment_iteration().unwrap(), 2);
        assert_eq!(env.increment_iteration().unwrap(), 3);
        assert_eq!(env.current_iterations(), 3);
    }

    #[test]
    fn test_increment_iteration_exceeds() {
        let env = BudgetEnvelope::new(1000, 100, 2);
        assert!(env.increment_iteration().is_ok());
        assert!(env.increment_iteration().is_ok());
        let err = env.increment_iteration().unwrap_err();
        assert_eq!(err, BudgetExceededError::Iterations { current: 3, max: 2 });
    }

    #[test]
    fn test_clone_shares_atomics() {
        let env = BudgetEnvelope::new(1000, 100, 5);
        let cloned = env.clone();
        assert!(env.record_and_check(300, 50).is_ok());
        assert_eq!(cloned.spent_cost_millicosts(), 300);
        assert_eq!(cloned.spent_tokens(), 50);
    }

    #[test]
    fn test_zero_budget_rejects_any_use() {
        let env = BudgetEnvelope::new(0, 0, 0);
        let err = env.record_and_check(1, 0).unwrap_err();
        assert_eq!(err, BudgetExceededError::Cost { spent: 1, max: 0 });
        assert!(env.increment_iteration().is_err());
    }

    #[test]
    fn test_exact_budget_boundary() {
        let env = BudgetEnvelope::new(1000, 200, 3);
        assert!(env.record_and_check(1000, 200).is_ok());
        assert_eq!(env.spent_cost_millicosts(), 1000);
        assert_eq!(env.spent_tokens(), 200);
        // One more token should exceed
        let err = env.record_and_check(0, 1).unwrap_err();
        assert_eq!(err, BudgetExceededError::Tokens { spent: 201, max: 200 });
    }

    #[test]
    fn test_multiple_records_sum_correctly() {
        let env = BudgetEnvelope::new(5000, 500, 10);
        assert!(env.record_and_check(1000, 100).is_ok());
        assert!(env.record_and_check(2000, 200).is_ok());
        assert!(env.record_and_check(500, 50).is_ok());
        assert_eq!(env.spent_cost_millicosts(), 3500);
        assert_eq!(env.spent_tokens(), 350);
    }

    #[test]
    fn test_record_after_failure_still_accumulates() {
        let env = BudgetEnvelope::new(100, 50, 5);
        assert!(env.record_and_check(60, 30).is_ok());
        let _ = env.record_and_check(60, 30); // exceeds
        assert_eq!(env.spent_cost_millicosts(), 120); // still accumulated
        assert_eq!(env.spent_tokens(), 60);
    }
}