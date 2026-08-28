//! Resource management — budget quotas and spend tracking.
//!
//! Ported from the monolith's `src/resource/mod.rs`. This is the canonical
//! 7-method `ResourceManager` trait — the monolith's `DefaultResourceManager`
//! will implement this trait (with extra inherent methods) at production cutover.

use async_trait::async_trait;
use fusion_core::NanoUSD;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

/// Budget quota — daily cost and token limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quota {
    pub max_daily_cost: NanoUSD,
    pub max_daily_tokens: u64,
}

/// Resource manager trait — 7 methods matching the monolith's interface.
///
/// This is the canonical definition. The monolith's `DefaultResourceManager`
/// will implement this trait at production cutover. Extra methods (`try_reserve`,
/// `release`, `record_usage`) are needed by the executor and stream meter, not
/// just the compiler pass — they're part of the trait because the trait represents
/// the full resource management interface, not just the budget check subset.
#[async_trait]
pub trait ResourceManager: Send + Sync {
    /// Returns true if the estimated cost and tokens fit within remaining quota.
    async fn can_afford(&self, estimated_cost: NanoUSD, estimated_tokens: u64) -> bool;

    /// Atomically reserves budget for an execution. Returns false if insufficient.
    async fn try_reserve(&self, estimated_cost: NanoUSD, estimated_tokens: u64) -> bool;

    /// Releases previously reserved budget.
    async fn release(&self, estimated_cost: NanoUSD, estimated_tokens: u64) -> anyhow::Result<()>;

    /// Returns the budget quota.
    fn quota(&self) -> &Quota;

    /// Returns total cost spent so far in NanoUSD.
    fn spent_cost(&self) -> NanoUSD;

    /// Returns total tokens spent so far.
    fn spent_tokens(&self) -> u64;

    /// Records actual measured usage (e.g. from a stream meter) so quota
    /// accounting reflects reality rather than only estimates. No-op by default.
    async fn record_usage(&self, _cost_nanos: NanoUSD, _tokens: u64) {}
}

/// Test-double resource manager for crate-level tests.
#[derive(Debug)]
pub struct StubResourceManager {
    quota: Quota,
    cost: AtomicU64,
    tokens: AtomicU64,
    reserve_lock: std::sync::Mutex<()>,
}

impl StubResourceManager {
    pub fn new(quota: Quota) -> Self {
        Self {
            quota,
            cost: AtomicU64::new(0),
            tokens: AtomicU64::new(0),
            reserve_lock: std::sync::Mutex::new(()),
        }
    }

    /// Simulate spend for testing accumulated-state scenarios.
    pub fn simulate_spend(&self, cost_nanos: u64, tokens: u64) {
        self.cost.fetch_add(cost_nanos, Ordering::Relaxed);
        self.tokens.fetch_add(tokens, Ordering::Relaxed);
    }
}

impl Default for StubResourceManager {
    fn default() -> Self {
        Self::new(Quota {
            max_daily_cost: NanoUSD::ZERO,
            max_daily_tokens: 0,
        })
    }
}

#[async_trait]
impl ResourceManager for StubResourceManager {
    /// Advisory check: reads current spend atomically but **without** holding
    /// `reserve_lock`. Two concurrent `can_afford` calls may both see available
    /// budget then both proceed to `try_reserve`, which will reject one. Callers
    /// must treat this as a hint and handle `try_reserve` failure gracefully.
    async fn can_afford(&self, estimated_cost: NanoUSD, estimated_tokens: u64) -> bool {
        let cost_nanos = estimated_cost.as_nanos();
        let current_cost = self.cost.load(Ordering::Acquire);
        let current_tokens = self.tokens.load(Ordering::Acquire);
        let max_cost = self.quota.max_daily_cost.as_nanos();
        let max_tokens = self.quota.max_daily_tokens;
        // Saturating sums mirror the monolith's `DefaultResourceManager` so a
        // hostile overflow cannot wrap `current_cost + cost_nanos` to a falsely
        // affordable small value.
        (current_cost.saturating_add(cost_nanos) <= max_cost)
            && (current_tokens.saturating_add(estimated_tokens) <= max_tokens)
    }

    async fn try_reserve(&self, estimated_cost: NanoUSD, estimated_tokens: u64) -> bool {
        let cost_nanos = estimated_cost.as_nanos();
        let max_cost = self.quota.max_daily_cost.as_nanos();
        let max_tokens = self.quota.max_daily_tokens;

        // Use a single critical section so concurrent reserves cannot both
        // pass the check and double-count over quota (TOCTOU). Saturating
        // sums avoid overflow on adversarial inputs.
        let _guard = match self.reserve_lock.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let current_cost = self.cost.load(Ordering::Acquire);
        let current_tokens = self.tokens.load(Ordering::Acquire);
        if current_cost.saturating_add(cost_nanos) > max_cost
            || current_tokens.saturating_add(estimated_tokens) > max_tokens
        {
            return false;
        }
        self.cost
            .store(current_cost.saturating_add(cost_nanos), Ordering::Release);
        self.tokens.store(
            current_tokens.saturating_add(estimated_tokens),
            Ordering::Release,
        );
        true
    }

    async fn release(&self, estimated_cost: NanoUSD, estimated_tokens: u64) -> anyhow::Result<()> {
        let cost_nanos = estimated_cost.as_nanos();
        // Saturating subtraction: a double-release (or release of a never-
        // reserved oversized estimate) must never wrap `used` to `u64::MAX`.
        let current_cost = self.cost.load(Ordering::Acquire);
        let current_tokens = self.tokens.load(Ordering::Acquire);
        self.cost
            .fetch_sub(cost_nanos.min(current_cost), Ordering::Relaxed);
        self.tokens
            .fetch_sub(estimated_tokens.min(current_tokens), Ordering::Relaxed);
        Ok(())
    }

    fn quota(&self) -> &Quota {
        &self.quota
    }

    fn spent_cost(&self) -> NanoUSD {
        NanoUSD::from_nanos(self.cost.load(Ordering::Acquire))
    }

    fn spent_tokens(&self) -> u64 {
        self.tokens.load(Ordering::Acquire)
    }

    async fn record_usage(&self, cost_nanos: NanoUSD, tokens: u64) {
        self.cost
            .fetch_add(cost_nanos.as_nanos(), Ordering::AcqRel);
        self.tokens.fetch_add(tokens, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_tracks_spend() {
        let stub = StubResourceManager::new(Quota {
            max_daily_cost: NanoUSD::from_nanos(100_000_000_000),
            max_daily_tokens: 1_000_000,
        });
        assert_eq!(stub.spent_cost(), NanoUSD::ZERO);
        assert_eq!(stub.spent_tokens(), 0);

        stub.simulate_spend(50_000, 100_000);
        assert_eq!(stub.spent_cost(), NanoUSD::from_nanos(50_000));
        assert_eq!(stub.spent_tokens(), 100_000);
    }

    #[tokio::test]
    async fn stub_can_afford_checks_quota() {
        let stub = StubResourceManager::new(Quota {
            max_daily_cost: NanoUSD::ONE_DOLLAR,
            max_daily_tokens: 1000,
        }); // $1, 1000 tokens
        assert!(stub.can_afford(NanoUSD::from_nanos(500_000_000), 500).await); // Under quota
        assert!(
            !stub
                .can_afford(NanoUSD::from_nanos(1_100_000_000), 500)
                .await
        ); // Over cost quota
        assert!(
            !stub
                .can_afford(NanoUSD::from_nanos(500_000_000), 1100)
                .await
        ); // Over token quota
    }
}
