use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use crate::release::evaluator::PolicyEvaluation;
use crate::release::gate::GateResult;
use crate::release::policy::ReleaseEnvironment;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseAssessment {
    pub assessment_id: String,
    pub timestamp: DateTime<Utc>,
    pub environment: ReleaseEnvironment,
    pub policy_evaluation: PolicyEvaluation,
    pub gate_results: Vec<GateResult>,
}

impl ReleaseAssessment {
    pub fn new(
        environment: ReleaseEnvironment,
        policy_evaluation: PolicyEvaluation,
        gate_results: Vec<GateResult>,
    ) -> Self {
        let timestamp = Utc::now();
        let payload = format!("{}:{}:{:?}", environment, timestamp, policy_evaluation.decision);
        let assessment_id = compute_assessment_id(&payload);

        Self {
            assessment_id,
            timestamp,
            environment,
            policy_evaluation,
            gate_results,
        }
    }
}

pub fn compute_assessment_id(payload: &str) -> String {
    let mut hasher = DefaultHasher::new();
    payload.hash(&mut hasher);
    let hash = hasher.finish();
    format!("asm-{:012x}", hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_assessment_id_deterministic() {
        let id1 = compute_assessment_id("test-payload-1");
        let id2 = compute_assessment_id("test-payload-1");
        let id3 = compute_assessment_id("test-payload-2");

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
        assert!(id1.starts_with("asm-"));
    }
}
