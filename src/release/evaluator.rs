use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use crate::release::gate::{GateExecution, GateId, GateResult};
use crate::release::policy::{EnvironmentPolicy, PolicyDefinition, ReleaseEnvironment};
use crate::release::waiver::{WaiverEvaluation, WaiverSet};

#[derive(Debug, Clone)]
pub struct EvaluationContext {
    pub environment: ReleaseEnvironment,
    pub policy: PolicyDefinition,
    pub waivers: WaiverSet,
    pub evaluation_time: DateTime<Utc>,
}

impl EvaluationContext {
    pub fn new(environment: ReleaseEnvironment, policy: PolicyDefinition, waivers: WaiverSet) -> Self {
        Self {
            environment,
            policy,
            waivers,
            evaluation_time: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseDecision {
    Approved,
    ApprovedWithWaivers,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PolicySummary {
    pub total_gates: usize,
    pub passed: usize,
    pub required_failed: usize,
    pub waived: usize,
    pub advisory_failed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyEvaluation {
    pub environment: ReleaseEnvironment,
    pub decision: ReleaseDecision,
    pub summary: PolicySummary,
    pub required_failures: Vec<GateId>,
    pub waived_failures: Vec<WaiverEvaluation>,
    pub advisory_failures: Vec<GateId>,
    pub passed_gates: Vec<GateId>,
}

pub struct EvidenceClassifier;

#[derive(Debug, Default)]
pub struct ClassifiedEvidence {
    pub passed: Vec<GateId>,
    pub required_failed: Vec<GateId>,
    pub advisory_failed: Vec<GateId>,
    pub ignored: Vec<GateId>,
}

impl EvidenceClassifier {
    pub fn classify(results: &[GateResult], policy: &EnvironmentPolicy) -> ClassifiedEvidence {
        let mut classified = ClassifiedEvidence::default();

        for result in results {
            let gate_id = result.gate_id;
            let is_required = policy.require.contains(&gate_id);
            let is_advisory = policy.advisory.contains(&gate_id);

            if result.passed {
                classified.passed.push(gate_id);
            } else if is_required {
                classified.required_failed.push(gate_id);
            } else if is_advisory {
                classified.advisory_failed.push(gate_id);
            } else {
                classified.ignored.push(gate_id);
            }
        }

        classified
    }
}

pub struct PolicyEvaluator;

impl PolicyEvaluator {
    pub fn evaluate(ctx: &EvaluationContext, executions: &[GateExecution]) -> PolicyEvaluation {
        let results: Vec<GateResult> = executions
            .iter()
            .filter_map(|e| match e {
                GateExecution::Success(res) => Some(res.clone()),
                GateExecution::ExecutionError(_) => None,
            })
            .collect();

        let env_policy = ctx.policy.get_environment_policy(&ctx.environment)
            .cloned()
            .unwrap_or_default();

        let classified = EvidenceClassifier::classify(&results, &env_policy);

        let mut remaining_required_failures = Vec::new();
        let mut waived_failures = Vec::new();

        for failed_gate in classified.required_failed {
            if let Some(waiver) = ctx.waivers.find_active_waiver(failed_gate, None, ctx.evaluation_time) {
                waived_failures.push(WaiverEvaluation {
                    waiver: waiver.clone(),
                    active: true,
                    gate: failed_gate,
                });
            } else {
                remaining_required_failures.push(failed_gate);
            }
        }

        let decision = if !remaining_required_failures.is_empty() {
            ReleaseDecision::Blocked
        } else if !waived_failures.is_empty() {
            ReleaseDecision::ApprovedWithWaivers
        } else {
            ReleaseDecision::Approved
        };

        let summary = PolicySummary {
            total_gates: executions.len(),
            passed: classified.passed.len(),
            required_failed: remaining_required_failures.len(),
            waived: waived_failures.len(),
            advisory_failed: classified.advisory_failed.len(),
        };

        PolicyEvaluation {
            environment: ctx.environment.clone(),
            decision,
            summary,
            required_failures: remaining_required_failures,
            waived_failures,
            advisory_failures: classified.advisory_failed,
            passed_gates: classified.passed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use chrono::TimeZone;
    use crate::release::waiver::Waiver;

    fn mock_execution(gate_id: GateId, passed: bool) -> GateExecution {
        GateExecution::Success(GateResult {
            gate_id,
            passed,
            summary: format!("{gate_id} passed={passed}"),
            details: vec![],
            duration: Duration::from_millis(10),
        })
    }

    #[test]
    fn test_evaluator_all_passed() {
        let policy = PolicyDefinition::default_policy();
        let ctx = EvaluationContext::new(ReleaseEnvironment::Production, policy, WaiverSet::default());
        let results = vec![
            mock_execution(GateId::Sdk1, true),
            mock_execution(GateId::Replay1, true),
            mock_execution(GateId::Upgrade1, true),
            mock_execution(GateId::Determinism1, true),
            mock_execution(GateId::Plugin1, true),
        ];

        let eval = PolicyEvaluator::evaluate(&ctx, &results);
        assert_eq!(eval.decision, ReleaseDecision::Approved);
        assert_eq!(eval.summary.required_failed, 0);
        assert_eq!(eval.summary.waived, 0);
    }

    #[test]
    fn test_evaluator_required_failure_blocked() {
        let policy = PolicyDefinition::default_policy();
        let ctx = EvaluationContext::new(ReleaseEnvironment::Production, policy, WaiverSet::default());
        let results = vec![
            mock_execution(GateId::Sdk1, true),
            mock_execution(GateId::Plugin1, false), // Required gate fails
        ];

        let eval = PolicyEvaluator::evaluate(&ctx, &results);
        assert_eq!(eval.decision, ReleaseDecision::Blocked);
        assert!(eval.required_failures.contains(&GateId::Plugin1));
    }

    #[test]
    fn test_evaluator_required_failure_waived() {
        let policy = PolicyDefinition::default_policy();
        let waiver = Waiver {
            id: "waiver-1".into(),
            gate: GateId::Plugin1,
            artifact: None,
            reason: "testing waiver".into(),
            expires: Utc.with_ymd_and_hms(2030, 1, 1, 0, 0, 0).unwrap(),
            approved_by: "architecture".into(),
        };
        let waiver_set = WaiverSet { waivers: vec![waiver] };
        let ctx = EvaluationContext::new(ReleaseEnvironment::Production, policy, waiver_set);

        let results = vec![
            mock_execution(GateId::Sdk1, true),
            mock_execution(GateId::Plugin1, false), // Fails, but waived
        ];

        let eval = PolicyEvaluator::evaluate(&ctx, &results);
        assert_eq!(eval.decision, ReleaseDecision::ApprovedWithWaivers);
        assert_eq!(eval.summary.waived, 1);
        assert_eq!(eval.summary.required_failed, 0);
    }

    #[test]
    fn test_evaluator_advisory_failure_approved() {
        let policy = PolicyDefinition::default_policy();
        let ctx = EvaluationContext::new(ReleaseEnvironment::Production, policy, WaiverSet::default());
        let results = vec![
            mock_execution(GateId::Sdk1, true),
            mock_execution(GateId::Provider1, false), // Advisory gate fails
        ];

        let eval = PolicyEvaluator::evaluate(&ctx, &results);
        assert_eq!(eval.decision, ReleaseDecision::Approved);
        assert_eq!(eval.summary.advisory_failed, 1);
        assert!(eval.advisory_failures.contains(&GateId::Provider1));
    }
}
