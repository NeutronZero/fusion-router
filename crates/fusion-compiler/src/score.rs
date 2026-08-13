//! Pluggable route scoring (Phase 5).
//!
//! Replaces the stub `explain_route` (only `budget_score = Some(1.0)`) with
//! real multi-dimensional sub-scores: capability, health, latency, policy,
//! and budget. Scoring stays report-side metadata — WorkflowIR passes are
//! untouched.
//!
//! Defaults are static and offline; real HTTP probes can be injected later
//! behind the same traits without changing the report shape.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use fusion_types::WorkflowIR;

#[async_trait]
pub trait CapabilityScorer: Send + Sync {
    /// 0.0–1.0 capability of a provider for an intent.
    async fn score(&self, provider: &str, intent: &str) -> Option<f64>;
}

#[async_trait]
pub trait HealthScorer: Send + Sync {
    /// 0.0–1.0 health of a provider.
    async fn score(&self, provider: &str) -> Option<f64>;
}

#[async_trait]
pub trait LatencyScorer: Send + Sync {
    /// 0.0–1.0 latency score; higher = better (inverted raw p50).
    async fn score(&self, provider: &str) -> Option<f64>;
}

#[async_trait]
pub trait PolicyScorer: Send + Sync {
    /// 0.0–1.0 policy score for a provider against the compiled IR.
    async fn score(&self, provider: &str, ir: &WorkflowIR) -> Option<f64>;
}

/// Aggregate of pluggable scorers attached to a `CompilerEngine`.
pub struct ScoreSources {
    pub capability: Option<Arc<dyn CapabilityScorer>>,
    pub health: Option<Arc<dyn HealthScorer>>,
    pub latency: Option<Arc<dyn LatencyScorer>>,
    pub policy: Option<Arc<dyn PolicyScorer>>,
}

impl Default for ScoreSources {
    fn default() -> Self {
        Self {
            capability: Some(Arc::new(StaticCapabilityScorer::default())),
            health: Some(Arc::new(StaticHealthScorer::default())),
            latency: Some(Arc::new(StaticLatencyScorer::default())),
            policy: Some(Arc::new(StaticPolicyScorer::default())),
        }
    }
}

impl ScoreSources {
    pub fn empty() -> Self {
        Self {
            capability: None,
            health: None,
            latency: None,
            policy: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Static (offline) defaults
// ---------------------------------------------------------------------------

/// Static capability table with a small intent-keyword boost. Provider list
/// and scores are injectable so tests can differentiate totals.
pub struct StaticCapabilityScorer {
    table: HashMap<String, f64>,
    intent_boosts: Vec<(String, String, f64)>,
}

impl Default for StaticCapabilityScorer {
    fn default() -> Self {
        Self {
            table: HashMap::from([
                ("openrouter".to_string(), 0.9),
                ("zen".to_string(), 0.85),
                ("ollama".to_string(), 0.6),
            ]),
            intent_boosts: vec![
                ("code".to_string(), "openrouter".to_string(), 0.05),
                ("generate".to_string(), "openrouter".to_string(), 0.05),
                ("reason".to_string(), "zen".to_string(), 0.05),
                ("think".to_string(), "zen".to_string(), 0.05),
                ("quick".to_string(), "ollama".to_string(), 0.05),
                ("fast".to_string(), "ollama".to_string(), 0.05),
            ],
        }
    }
}

impl StaticCapabilityScorer {
    pub fn new(table: HashMap<String, f64>) -> Self {
        Self { table, intent_boosts: Vec::new() }
    }
}

#[async_trait]
impl CapabilityScorer for StaticCapabilityScorer {
    async fn score(&self, provider: &str, intent: &str) -> Option<f64> {
        let base = self.table.get(provider).copied()?;
        let boost: f64 = self
            .intent_boosts
            .iter()
            .filter(|(keyword, target, _)| target == provider && intent.to_lowercase().contains(keyword))
            .map(|(_, _, boost)| *boost)
            .sum();
        Some((base + boost).min(1.0))
    }
}

/// Static health table; unknown providers score `1.0` (opt-in healthy
/// hypothesis) — inject a table to model outages deterministically.
pub struct StaticHealthScorer {
    table: HashMap<String, f64>,
}

impl Default for StaticHealthScorer {
    fn default() -> Self {
        Self { table: HashMap::new() }
    }
}

impl StaticHealthScorer {
    pub fn new(table: HashMap<String, f64>) -> Self {
        Self { table }
    }
}

#[async_trait]
impl HealthScorer for StaticHealthScorer {
    async fn score(&self, provider: &str) -> Option<f64> {
        Some(self.table.get(provider).copied().unwrap_or(1.0))
    }
}

/// Inverse of configured p50 latency (higher p50 → lower score); providers
/// not present in the table score `None`.
pub struct StaticLatencyScorer {
    p50_ms: HashMap<String, f64>,
}

impl Default for StaticLatencyScorer {
    fn default() -> Self {
        Self {
            p50_ms: HashMap::from([
                ("zen".to_string(), 300.0),
                ("openrouter".to_string(), 500.0),
                ("ollama".to_string(), 800.0),
            ]),
        }
    }
}

impl StaticLatencyScorer {
    pub fn new(p50_ms: HashMap<String, f64>) -> Self {
        Self { p50_ms }
    }
}

#[async_trait]
impl LatencyScorer for StaticLatencyScorer {
    async fn score(&self, provider: &str) -> Option<f64> {
        let p50 = self.p50_ms.get(provider)?;
        let min = self.p50_ms.values().copied().fold(f64::INFINITY, f64::min);
        Some((min / p50).min(1.0))
    }
}

/// Static policy scorer: deny-listed providers score `0.0`, everything else
/// scores `1.0`. This is a *soft report-side* score — the hard deny gate is
/// `PolicyCompilerPass`.
pub struct StaticPolicyScorer {
    deny: HashSet<String>,
}

impl Default for StaticPolicyScorer {
    fn default() -> Self {
        Self { deny: HashSet::new() }
    }
}

impl StaticPolicyScorer {
    pub fn deny(providers: &[&str]) -> Self {
        Self { deny: providers.iter().map(|p| p.to_string()).collect() }
    }
}

#[async_trait]
impl PolicyScorer for StaticPolicyScorer {
    async fn score(&self, provider: &str, _ir: &WorkflowIR) -> Option<f64> {
        if self.deny.contains(provider) {
            Some(0.0)
        } else {
            Some(1.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn default_capability_differentiates_providers() {
        let scorer = StaticCapabilityScorer::default();
        let openrouter = scorer.score("openrouter", "Build a web app").await.unwrap();
        let zen = scorer.score("zen", "Build a web app").await.unwrap();
        let ollama = scorer.score("ollama", "Build a web app").await.unwrap();
        assert!(openrouter > zen && zen > ollama, "{openrouter} > {zen} > {ollama}");
        assert!(openrouter <= 1.0 && ollama > 0.0);
    }

    #[tokio::test]
    async fn intent_keyword_boosts_capability() {
        let scorer = StaticCapabilityScorer::default();
        let base = scorer.score("openrouter", "general question").await.unwrap();
        let boosted = scorer.score("openrouter", "Write code").await.unwrap();
        assert!(boosted > base);
        assert!(boosted <= 1.0);
    }

    #[tokio::test]
    async fn unknown_provider_capability_is_none() {
        let scorer = StaticCapabilityScorer::default();
        assert!(scorer.score("mystery-provider", "anything").await.is_none());
    }

    #[tokio::test]
    async fn latency_inverts_p50() {
        let scorer = StaticLatencyScorer::default();
        let zen = scorer.score("zen").await.unwrap();
        let openrouter = scorer.score("openrouter").await.unwrap();
        let ollama = scorer.score("ollama").await.unwrap();
        assert!(zen > openrouter && openrouter > ollama, "{zen} > {openrouter} > {ollama}");
        assert_eq!(zen, 1.0, "fastest provider gets 1.0");
    }

    #[tokio::test]
    async fn latency_missing_provider_is_none() {
        let scorer = StaticLatencyScorer::default();
        assert!(scorer.score("mystery-provider").await.is_none());
    }

    #[tokio::test]
    async fn policy_denylist_zeroes_provider() {
        let scorer = StaticPolicyScorer::deny(&["openrouter"]);
        let ir = WorkflowIR {
            plan_id: uuid::Uuid::new_v4(),
            nodes: vec![],
            edges: vec![],
            metadata: fusion_types::IRMetadata {
                policy_applied: vec![],
                estimated_cost: 0.0,
                estimated_tokens: 0,
            },
        };
        assert_eq!(scorer.score("openrouter", &ir).await, Some(0.0));
        assert_eq!(scorer.score("zen", &ir).await, Some(1.0));
    }
}