//! Resource management — budget quotas and spend tracking.
//!
//! Ported from the monolith's `src/resource/mod.rs`. The trait is simplified
//! to the budget-check interface needed by `BudgetOptimisationPass`. The
//! monolith's `DefaultResourceManager` implements this trait and retains its
//! extra methods (`try_reserve`, `release`, `record_usage`) as inherent methods.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

/// Budget quota — daily cost and token limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quota {
    pub max_daily_cost: f64,
    pub max_daily_tokens: u64,
}

/// Resource manager trait — answers "can this execution be afforded?"
///
/// Simplified from the monolith's 7-method trait to the 4 methods needed
/// by `BudgetOptimisationPass`. The monolith's `DefaultResourceManager`
/// implements this trait and keeps extra methods as inherent methods.
#[async_trait]
pub trait ResourceManager: Send + Sync {
    /// Returns true if the estimated cost and tokens fit within remaining quota.
    async fn can_afford(&self, estimated_cost: f64, estimated_tokens: u64) -> bool;

    /// Returns the budget quota.
    fn quota(&self) -> &Quota;

    /// Returns total cost spent so far (in dollars).
    fn spent_cost(&self) -> f64;

    /// Returns total tokens spent so far.
    fn spent_tokens(&self) -> u64;
}

/// Test-double resource manager for crate-level tests.
/// Always returns `can_afford() = true` — tests budget *plumbing*,
/// not budget *logic* (real accounting stays in the monolith).
pub struct StubResourceManager {
    quota: Quota,
    cost: AtomicU64,
    tokens: AtomicU64,
}

impl StubResourceManager {
    pub fn new(max_daily_cost: f64, max_daily_tokens: u64) -> Self {
        Self {
            quota: Quota { max_daily_cost, max_daily_tokens },
            cost: AtomicU64::new(0),
            tokens: AtomicU64::new(0),
        }
    }

    /// Simulate spend for testing accumulated-state scenarios.
    pub fn simulate_spend(&self, cost_millicosts: u64, tokens: u64) {
        self.cost.fetch_add(cost_millicosts, Ordering::Relaxed);
        self.tokens.fetch_add(tokens, Ordering::Relaxed);
    }
}

#[async_trait]
impl ResourceManager for StubResourceManager {
    async fn can_afford(&self, _estimated_cost: f64, _estimated_tokens: u64) -> bool {
        true // Stub always allows — plumbing test, not budget-logic test
    }

    fn quota(&self) -> &Quota {
        &self.quota
    }

    fn spent_cost(&self) -> f64 {
        self.cost.load(Ordering::Acquire) as f64 / 1000.0
    }

    fn spent_tokens(&self) -> u64 {
        self.tokens.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_always_allows() {
        let stub = StubResourceManager::new(100.0, 1_000_000);
        assert!(stub.spent_cost() == 0.0);
        assert!(stub.spent_tokens() == 0);
    }
}
