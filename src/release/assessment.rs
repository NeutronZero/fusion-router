use crate::release::evaluator::PolicyEvaluation;
use crate::release::gate::GateResult;
use crate::release::policy::ReleaseEnvironment;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

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
        let payload = format!(
            "{}:{}:{:?}",
            environment, timestamp, policy_evaluation.decision
        );
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
    use crate::release::gate::{GateId, GateResult};
    use crate::release::policy::ReleaseEnvironment;

    fn passing_result() -> GateResult {
        GateResult {
            gate_id: GateId::Sdk1,
            passed: true,
            summary: "sdk gate passed".into(),
            details: vec![],
            duration: std::time::Duration::from_secs(0),
        }
    }

    #[test]
    fn test_compute_assessment_id_deterministic() {
        let id1 = compute_assessment_id("test-payload-1");
        let id2 = compute_assessment_id("test-payload-1");
        let id3 = compute_assessment_id("test-payload-2");

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
        assert!(id1.starts_with("asm-"));
    }

    #[test]
    fn test_compute_assessment_id_format() {
        let id = compute_assessment_id("payload");
        let suffix = id.strip_prefix("asm-").unwrap();
        assert_eq!(
            suffix.len(),
            16,
            "suffix must be 16 hex digits, got: {suffix}"
        );
        assert!(suffix.chars().all(|c| c.is_ascii_hexdigit()));
    }

    fn eval(decision: crate::release::evaluator::ReleaseDecision) -> PolicyEvaluation {
        PolicyEvaluation {
            environment: ReleaseEnvironment::Development,
            decision,
            summary: Default::default(),
            reason: None,
            required_failures: vec![],
            waived_failures: vec![],
            advisory_failures: vec![],
            passed_gates: vec![],
        }
    }

    #[test]
    fn test_release_assessment_new_builds_id() {
        let assessment = ReleaseAssessment::new(
            ReleaseEnvironment::Development,
            eval(crate::release::evaluator::ReleaseDecision::Approved),
            vec![passing_result()],
        );

        assert!(assessment.assessment_id.starts_with("asm-"));
        assert_eq!(assessment.environment, ReleaseEnvironment::Development);
        assert_eq!(assessment.gate_results.len(), 1);
        assert!(assessment.gate_results[0].passed);
    }

    #[test]
    fn test_assessment_id_differs_by_decision() {
        let a = ReleaseAssessment::new(
            ReleaseEnvironment::Development,
            eval(crate::release::evaluator::ReleaseDecision::Approved),
            vec![],
        );
        let b = ReleaseAssessment::new(
            ReleaseEnvironment::Development,
            eval(crate::release::evaluator::ReleaseDecision::Blocked),
            vec![],
        );

        assert_ne!(a.assessment_id, b.assessment_id);
    }
}
