//! Per-request budget envelope (canonical definition lives in `fusion_types`).
//! Re-exported here so existing callers and the `ExecutionInstance` field keep
//! their `crate::resource::BudgetEnvelope` path (Phase 6.3b lift).

pub use fusion_types::BudgetEnvelope;

#[cfg(test)]
mod tests {
    use super::*;
    use fusion_core::NanoUSD;
    use fusion_types::BudgetExceededError;

    #[test]
    fn test_record_within_budget() {
        let env = BudgetEnvelope::new(NanoUSD::from_nanos(1000), 100, 5);
        assert!(env.record_and_check(NanoUSD::from_nanos(500), 30).is_ok());
        assert_eq!(env.spent_cost().as_nanos(), 500);
        assert_eq!(env.spent_tokens(), 30);
    }

    #[test]
    fn test_record_exceeds_cost() {
        let env = BudgetEnvelope::new(NanoUSD::from_nanos(1000), 100, 5);
        assert!(env.record_and_check(NanoUSD::from_nanos(600), 30).is_ok());
        let err = env.record_and_check(NanoUSD::from_nanos(500), 30).unwrap_err();
        assert_eq!(err, BudgetExceededError::Cost { spent: 1100, max: 1000 });
    }

    #[test]
    fn test_record_exceeds_tokens() {
        let env = BudgetEnvelope::new(NanoUSD::from_nanos(1000), 100, 5);
        assert!(env.record_and_check(NanoUSD::from_nanos(100), 80).is_ok());
        let err = env.record_and_check(NanoUSD::from_nanos(100), 30).unwrap_err();
        assert_eq!(err, BudgetExceededError::Tokens { spent: 110, max: 100 });
    }

    #[test]
    fn test_increment_iteration_within_limit() {
        let env = BudgetEnvelope::new(NanoUSD::from_nanos(1000), 100, 3);
        assert_eq!(env.increment_iteration().unwrap(), 1);
        assert_eq!(env.increment_iteration().unwrap(), 2);
        assert_eq!(env.increment_iteration().unwrap(), 3);
        assert_eq!(env.current_iterations(), 3);
    }

    #[test]
    fn test_increment_iteration_exceeds() {
        let env = BudgetEnvelope::new(NanoUSD::from_nanos(1000), 100, 2);
        assert!(env.increment_iteration().is_ok());
        assert!(env.increment_iteration().is_ok());
        let err = env.increment_iteration().unwrap_err();
        assert_eq!(err, BudgetExceededError::Iterations { current: 3, max: 2 });
    }

    #[test]
    fn test_clone_shares_atomics() {
        let env = BudgetEnvelope::new(NanoUSD::from_nanos(1000), 100, 5);
        let cloned = env.clone();
        assert!(env.record_and_check(NanoUSD::from_nanos(300), 50).is_ok());
        assert_eq!(cloned.spent_cost().as_nanos(), 300);
        assert_eq!(cloned.spent_tokens(), 50);
    }

    #[test]
    fn test_zero_budget_rejects_any_use() {
        let env = BudgetEnvelope::new(NanoUSD::ZERO, 0, 0);
        let err = env.record_and_check(NanoUSD::from_nanos(1), 0).unwrap_err();
        assert_eq!(err, BudgetExceededError::Cost { spent: 1, max: 0 });
        assert!(env.increment_iteration().is_err());
    }

    #[test]
    fn test_exact_budget_boundary() {
        let env = BudgetEnvelope::new(NanoUSD::from_nanos(1000), 200, 3);
        assert!(env.record_and_check(NanoUSD::from_nanos(1000), 200).is_ok());
        assert_eq!(env.spent_cost().as_nanos(), 1000);
        assert_eq!(env.spent_tokens(), 200);
        // One more token should exceed
        let err = env.record_and_check(NanoUSD::ZERO, 1).unwrap_err();
        assert_eq!(err, BudgetExceededError::Tokens { spent: 201, max: 200 });
    }

    #[test]
    fn test_multiple_records_sum_correctly() {
        let env = BudgetEnvelope::new(NanoUSD::from_nanos(5000), 500, 10);
        assert!(env.record_and_check(NanoUSD::from_nanos(1000), 100).is_ok());
        assert!(env.record_and_check(NanoUSD::from_nanos(2000), 200).is_ok());
        assert!(env.record_and_check(NanoUSD::from_nanos(500), 50).is_ok());
        assert_eq!(env.spent_cost().as_nanos(), 3500);
        assert_eq!(env.spent_tokens(), 350);
    }

    #[test]
    fn test_record_after_failure_still_accumulates() {
        let env = BudgetEnvelope::new(NanoUSD::from_nanos(100), 50, 5);
        assert!(env.record_and_check(NanoUSD::from_nanos(60), 30).is_ok());
        let _ = env.record_and_check(NanoUSD::from_nanos(60), 30); // exceeds
        assert_eq!(env.spent_cost().as_nanos(), 120); // still accumulated
        assert_eq!(env.spent_tokens(), 60);
    }
}
