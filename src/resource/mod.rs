use crate::types::{ExecutionGraph, NanoUSD, Quota};
use async_trait::async_trait;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

pub mod budget;
pub mod cancelling_stream;
pub mod guard;
pub mod kernel_adapter;
pub mod stream_meter;

pub use budget::BudgetEnvelope;
pub use guard::ResourceGuard;

#[async_trait]
pub trait ResourceManager: Send + Sync {
    async fn can_afford(&self, graph: &ExecutionGraph) -> bool;
    async fn try_reserve(&self, graph: &ExecutionGraph) -> bool;
    async fn release(&self, graph: &ExecutionGraph) -> anyhow::Result<()>;
    fn quota(&self) -> &Quota;
    fn spent_cost(&self) -> NanoUSD;
    fn spent_tokens(&self) -> u64;
    /// Records actual measured usage (e.g. from a stream meter) so quota
    /// accounting reflects reality rather than only estimates. No-op by
    /// default for implementers that only track estimates.
    async fn record_usage(&self, _cost: NanoUSD, _tokens: u64) {}
}

pub struct DefaultResourceManager {
    quota: Quota,
    used_cost: AtomicU64,
    used_tokens: AtomicU64,
    reserve_lock: Mutex<()>,
}

impl DefaultResourceManager {
    pub fn new(quota: Quota) -> Self {
        Self {
            quota,
            used_cost: AtomicU64::new(0),
            used_tokens: AtomicU64::new(0),
            reserve_lock: Mutex::new(()),
        }
    }

    /// Single critical section for admission: fails when the saturated sum
    /// would exceed either ceiling.
    fn try_apply(&self, cost: u64, tokens: u64) -> bool {
        let _guard = self.reserve_lock.lock();
        let current_cost = self.used_cost.load(Ordering::Acquire);
        let current_tokens = self.used_tokens.load(Ordering::Acquire);
        let new_cost = current_cost.saturating_add(cost);
        let new_tokens = current_tokens.saturating_add(tokens);
        if new_cost > self.quota.max_daily_cost.as_nanos()
            || new_tokens > self.quota.max_daily_tokens
        {
            return false;
        }
        self.used_cost.store(new_cost, Ordering::Release);
        self.used_tokens.store(new_tokens, Ordering::Release);
        true
    }

    /// Single critical section for usage accrual (actual measured spend).
    fn add_usage(&self, cost: u64, tokens: u64) {
        let _guard = self.reserve_lock.lock();
        let current_cost = self.used_cost.load(Ordering::Acquire);
        let current_tokens = self.used_tokens.load(Ordering::Acquire);
        self.used_cost
            .store(current_cost.saturating_add(cost), Ordering::Release);
        self.used_tokens
            .store(current_tokens.saturating_add(tokens), Ordering::Release);
    }

    /// Single critical section for release; saturates at zero so a
    /// double-release can never wrap and brick the quota.
    fn subtract(&self, cost: u64, tokens: u64) {
        let _guard = self.reserve_lock.lock();
        let current_cost = self.used_cost.load(Ordering::Acquire);
        let current_tokens = self.used_tokens.load(Ordering::Acquire);
        self.used_cost
            .store(current_cost.saturating_sub(cost), Ordering::Release);
        self.used_tokens
            .store(current_tokens.saturating_sub(tokens), Ordering::Release);
    }
}

#[async_trait]
impl ResourceManager for DefaultResourceManager {
    async fn can_afford(&self, graph: &ExecutionGraph) -> bool {
        let cost = graph.metadata.estimated_cost.as_nanos();
        let tokens = graph.metadata.estimated_tokens;
        let _guard = self.reserve_lock.lock();
        let current_cost = self.used_cost.load(Ordering::Acquire);
        let current_tokens = self.used_tokens.load(Ordering::Acquire);
        let max_cost = self.quota.max_daily_cost.as_nanos();
        let max_tokens = self.quota.max_daily_tokens;
        (current_cost.saturating_add(cost) <= max_cost)
            && (current_tokens.saturating_add(tokens) <= max_tokens)
    }

    async fn try_reserve(&self, graph: &ExecutionGraph) -> bool {
        let cost = graph.metadata.estimated_cost.as_nanos();
        let tokens = graph.metadata.estimated_tokens;
        self.try_apply(cost, tokens)
    }

    async fn release(&self, graph: &ExecutionGraph) -> anyhow::Result<()> {
        let cost = graph.metadata.estimated_cost.as_nanos();
        let tokens = graph.metadata.estimated_tokens;
        self.subtract(cost, tokens);
        Ok(())
    }

    async fn record_usage(&self, cost: NanoUSD, tokens: u64) {
        self.add_usage(cost.as_nanos(), tokens);
    }

    fn quota(&self) -> &Quota {
        &self.quota
    }

    fn spent_cost(&self) -> NanoUSD {
        NanoUSD::from_nanos(self.used_cost.load(Ordering::Acquire))
    }

    fn spent_tokens(&self) -> u64 {
        self.used_tokens.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::GraphMetadata;
    use std::sync::Arc;
    use uuid::Uuid;

    fn quota_with(max_cost: NanoUSD, max_tokens: u64) -> Quota {
        Quota {
            max_daily_cost: max_cost,
            max_daily_tokens: max_tokens,
            max_concurrent: 10,
            provider_limits: std::collections::HashMap::new(),
        }
    }

    fn graph_with(cost: NanoUSD, tokens: u64) -> ExecutionGraph {
        ExecutionGraph {
            graph_id: Uuid::new_v4(),
            nodes: vec![],
            edges: vec![],
            metadata: GraphMetadata {
                policy_version: 0,
                estimated_cost: cost,
                estimated_tokens: tokens,
                max_depth: 1,
                node_count: 0,
            },
            total_tokens: tokens,
            total_cost: cost,
            primitive_graph_hash: 0,
        }
    }

    #[tokio::test]
    async fn double_release_saturates_at_zero() {
        let manager =
            DefaultResourceManager::new(quota_with(NanoUSD::from_nanos(1_000_000_000), 1000));
        let graph = graph_with(NanoUSD::from_nanos(500_000), 200);
        assert!(manager.try_reserve(&graph).await);
        assert_eq!(manager.spent_tokens(), 200);

        manager.release(&graph).await.unwrap();
        // Second release must not underflow/wrap (which would brick quota).
        manager.release(&graph).await.unwrap();
        assert_eq!(manager.spent_tokens(), 0);
        assert_eq!(manager.spent_cost().as_nanos(), 0);

        // Quota is fully usable again after saturation.
        assert!(manager.try_reserve(&graph).await);
        assert_eq!(manager.spent_tokens(), 200);
    }

    #[tokio::test]
    async fn reserve_overflow_fails_instead_of_panic() {
        let manager =
            DefaultResourceManager::new(quota_with(NanoUSD::from_nanos(1_000_000_000), 1000));
        let huge = graph_with(NanoUSD::from_nanos(u64::MAX), u64::MAX);
        assert!(!manager.try_reserve(&huge).await);
        assert!(!manager.can_afford(&huge).await);
        assert_eq!(manager.spent_tokens(), 0);
        assert_eq!(manager.spent_cost().as_nanos(), 0);

        // Releasing a never-reserved oversized graph also saturates safely.
        manager.release(&huge).await.unwrap();
        assert_eq!(manager.spent_tokens(), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_reserves_and_releases_lose_no_updates() {
        const TASKS: usize = 8;
        const ATTEMPTS: usize = 50;
        const UNIT_TOKENS: u64 = 10;
        const UNIT_COST_NANOS: u64 = 1_000;

        let manager = Arc::new(DefaultResourceManager::new(quota_with(
            NanoUSD::from_nanos(1_000_000_000),
            TASKS as u64 * ATTEMPTS as u64 * UNIT_TOKENS,
        )));
        let graph = Arc::new(graph_with(
            NanoUSD::from_nanos(UNIT_COST_NANOS),
            UNIT_TOKENS,
        ));

        let successes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut handles = Vec::with_capacity(TASKS);
        for _ in 0..TASKS {
            let manager = manager.clone();
            let graph = graph.clone();
            let successes = successes.clone();
            handles.push(tokio::spawn(async move {
                for _ in 0..ATTEMPTS {
                    if manager.try_reserve(&graph).await {
                        successes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    }
                }
            }));
        }
        for handle in handles {
            handle.await.unwrap();
        }

        let total_successes = successes.load(std::sync::atomic::Ordering::SeqCst);
        let expected_tokens = total_successes as u64 * UNIT_TOKENS;
        assert!(
            expected_tokens <= manager.quota().max_daily_tokens,
            "test demand must fit quota"
        );
        assert_eq!(
            manager.spent_tokens(),
            expected_tokens,
            "lost updates would leave spent below the success count"
        );
        assert_eq!(
            manager.spent_cost().as_nanos(),
            total_successes as u64 * UNIT_COST_NANOS
        );

        let mut release_handles = Vec::with_capacity(total_successes);
        for _ in 0..total_successes {
            let manager = manager.clone();
            let graph = graph.clone();
            release_handles.push(tokio::spawn(async move {
                manager.release(&graph).await.unwrap();
            }));
        }
        for handle in release_handles {
            handle.await.unwrap();
        }
        assert_eq!(manager.spent_tokens(), 0);
        assert_eq!(manager.spent_cost().as_nanos(), 0);
    }

    #[tokio::test]
    async fn record_usage_is_serialized_and_accurate() {
        const TASKS: usize = 8;
        const ITERATIONS: u64 = 100;

        let manager = Arc::new(DefaultResourceManager::new(quota_with(
            NanoUSD::ONE_DOLLAR,
            u64::MAX,
        )));
        let mut handles = Vec::with_capacity(TASKS);
        for _ in 0..TASKS {
            let manager = manager.clone();
            handles.push(tokio::spawn(async move {
                for _ in 0..ITERATIONS {
                    manager.record_usage(NanoUSD::from_nanos(7), 3).await;
                }
            }));
        }
        for handle in handles {
            handle.await.unwrap();
        }
        assert_eq!(
            manager.spent_cost().as_nanos(),
            TASKS as u64 * ITERATIONS * 7
        );
        assert_eq!(manager.spent_tokens(), TASKS as u64 * ITERATIONS * 3);
    }
}
