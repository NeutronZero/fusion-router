use crate::release::gate::{GateExecution, GateId, GateResult};
use crate::release::policy::{EnvironmentPolicy, PolicyDefinition, ReleaseEnvironment};
use crate::release::waiver::{WaiverEvaluation, WaiverSet};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct EvaluationContext {
    pub environment: ReleaseEnvironment,
    pub policy: PolicyDefinition,
    pub waivers: WaiverSet,
    pub evaluation_time: DateTime<Utc>,
}

impl EvaluationContext {
    pub fn new(
        environment: ReleaseEnvironment,
        policy: PolicyDefinition,
        waivers: WaiverSet,
    ) -> Self {
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
    /// Human-readable justification for a non-approving decision (e.g. an
    /// unconfigured environment or named required failures).
    #[serde(default)]
    pub reason: Option<String>,
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
                GateExecution::ExecutionError(err) => {
                    // The runner normalizes errors into failed results before
                    // evaluation; reaching this branch means a caller bypassed
                    // that contract, so the error must not vanish silently.
                    tracing::warn!(
                        error = %err,
                        "evaluator dropped GateExecution::ExecutionError (caller bypassed runner normalization)"
                    );
                    None
                }
            })
            .collect();

        // Gate integrity: an environment with NO explicit policy can never
        // approve. The previous `unwrap_or_default()` produced an empty policy
        // under which every failed gate classified as "ignored" — approving
        // releases on fabricated evidence. None now means Blocked.
        let env_policy = ctx.policy.get_environment_policy(&ctx.environment);
        let unconfigured_environment = env_policy.is_none();

        let classified = match env_policy {
            Some(policy) => EvidenceClassifier::classify(&results, policy),
            None => {
                // Without a policy nothing may be ignored: failures are treated
                // as required failures so they remain visible in the summary.
                let mut fallback = ClassifiedEvidence::default();
                for result in &results {
                    if result.passed {
                        fallback.passed.push(result.gate_id);
                    } else {
                        fallback.required_failed.push(result.gate_id);
                    }
                }
                fallback
            }
        };

        let mut remaining_required_failures = Vec::new();
        let mut waived_failures = Vec::new();

        for failed_gate in classified.required_failed {
            if let Some(waiver) =
                ctx.waivers
                    .find_active_waiver(failed_gate, None, ctx.evaluation_time)
            {
                waived_failures.push(WaiverEvaluation {
                    waiver: waiver.clone(),
                    active: true,
                    gate: failed_gate,
                });
            } else {
                remaining_required_failures.push(failed_gate);
            }
        }

        let decision = if results.is_empty() {
            // No gate evidence at all — the evaluation cannot support an
            // approval decision.
            ReleaseDecision::Blocked
        } else if unconfigured_environment {
            // Unknown/unconfigured environment: only an explicit policy whose
            // rules pass may approve.
            ReleaseDecision::Blocked
        } else if !remaining_required_failures.is_empty() {
            ReleaseDecision::Blocked
        } else if !waived_failures.is_empty() {
            ReleaseDecision::ApprovedWithWaivers
        } else {
            ReleaseDecision::Approved
        };

        let reason = match decision {
            ReleaseDecision::Approved => None,
            _ => {
                let mut causes: Vec<String> = Vec::new();
                if results.is_empty() {
                    causes.push("no gate evidence was produced".to_string());
                }
                if unconfigured_environment {
                    causes.push(format!(
                        "environment '{}' has no configured release policy; it can never be approved without an explicit policy",
                        ctx.environment
                    ));
                }
                if !remaining_required_failures.is_empty() {
                    let names: Vec<String> = remaining_required_failures
                        .iter()
                        .map(|g| format!("{g:?}"))
                        .collect();
                    causes.push(format!(
                        "{} required gate(s) failed: {}",
                        names.len(),
                        names.join(", ")
                    ));
                }
                Some(causes.join("; "))
            }
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
            reason,
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
    use crate::release::waiver::Waiver;
    use chrono::TimeZone;
    use std::time::Duration;

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
        let ctx =
            EvaluationContext::new(ReleaseEnvironment::Production, policy, WaiverSet::default());
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
        let ctx =
            EvaluationContext::new(ReleaseEnvironment::Production, policy, WaiverSet::default());
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
        let waiver_set = WaiverSet {
            waivers: vec![waiver],
        };
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
        let ctx =
            EvaluationContext::new(ReleaseEnvironment::Production, policy, WaiverSet::default());
        let results = vec![
            mock_execution(GateId::Sdk1, true),
            mock_execution(GateId::Provider1, false), // Advisory gate fails
        ];

        let eval = PolicyEvaluator::evaluate(&ctx, &results);
        assert_eq!(eval.decision, ReleaseDecision::Approved);
        assert_eq!(eval.summary.advisory_failed, 1);
        assert!(eval.advisory_failures.contains(&GateId::Provider1));
    }

    #[test]
    fn test_evaluator_empty_evidence_blocked() {
        let policy = PolicyDefinition::default_policy();
        let ctx =
            EvaluationContext::new(ReleaseEnvironment::Production, policy, WaiverSet::default());
        let results = vec![];

        let eval = PolicyEvaluator::evaluate(&ctx, &results);
        assert_eq!(
            eval.decision,
            ReleaseDecision::Blocked,
            "no evidence must not Approve"
        );
    }

    #[test]
    fn test_evaluator_unknown_environment_blocked_even_when_all_pass() {
        // "canary" has no entry in the default policy — it must never approve,
        // even with flawless gate evidence.
        let policy = PolicyDefinition::default_policy();
        let ctx = EvaluationContext::new(
            ReleaseEnvironment::from_str("canary"),
            policy,
            WaiverSet::default(),
        );
        let results = vec![
            mock_execution(GateId::Sdk1, true),
            mock_execution(GateId::Replay1, true),
            mock_execution(GateId::Upgrade1, true),
            mock_execution(GateId::Determinism1, true),
            mock_execution(GateId::Plugin1, true),
        ];

        let eval = PolicyEvaluator::evaluate(&ctx, &results);
        assert_eq!(eval.decision, ReleaseDecision::Blocked);
        assert_eq!(eval.summary.required_failed, 0);
        let reason = eval.reason.expect("Blocked needs a clear reason");
        assert!(
            reason.contains("canary") && reason.contains("no configured release policy"),
            "reason must name the unconfigured environment: {reason}"
        );
    }

    #[test]
    fn test_evaluator_unknown_environment_with_failures_blocks_and_counts_them() {
        let policy = PolicyDefinition::default_policy();
        let ctx = EvaluationContext::new(
            ReleaseEnvironment::Custom("nightly".into()),
            policy,
            WaiverSet::default(),
        );
        let results = vec![
            mock_execution(GateId::Sdk1, true),
            mock_execution(GateId::Plugin1, false),
        ];

        let eval = PolicyEvaluator::evaluate(&ctx, &results);
        assert_eq!(eval.decision, ReleaseDecision::Blocked);
        assert!(
            eval.required_failures.contains(&GateId::Plugin1),
            "failures under an unconfigured environment must not be classified as ignored"
        );
        assert!(eval.reason.is_some());
    }

    #[test]
    fn test_evaluator_configured_environment_still_approves_clean_evidence() {
        // Staging is explicitly configured; its semantics are unchanged.
        let policy = PolicyDefinition::default_policy();
        let ctx = EvaluationContext::new(ReleaseEnvironment::Staging, policy, WaiverSet::default());
        let results = vec![
            mock_execution(GateId::Sdk1, true),
            mock_execution(GateId::Upgrade1, true),
            mock_execution(GateId::Plugin1, true),
            mock_execution(GateId::Replay1, false), // advisory in staging
            mock_execution(GateId::Connector1, false), // advisory in staging
        ];

        let eval = PolicyEvaluator::evaluate(&ctx, &results);
        assert_eq!(eval.decision, ReleaseDecision::Approved);
        assert_eq!(eval.summary.advisory_failed, 2);
        assert!(eval.reason.is_none());
    }

    #[test]
    fn test_evaluator_required_execution_error_blocks() {
        use crate::release::gate::GateError;
        use crate::release::policy::EnvironmentPolicy;

        let policy = PolicyDefinition {
            name: "audit-fix".into(),
            environments: [(
                "production".to_string(),
                EnvironmentPolicy {
                    require: vec![GateId::Replay1],
                    advisory: vec![],
                },
            )]
            .into_iter()
            .collect(),
        };
        let ctx =
            EvaluationContext::new(ReleaseEnvironment::Production, policy, WaiverSet::default());
        let results = vec![
            mock_execution(GateId::Sdk1, true),
            GateExecution::Success(GateResult {
                gate_id: GateId::Replay1,
                passed: false,
                summary: GateError::ToolNotAvailable("determinism backend not available".into())
                    .to_string(),
                details: vec![],
                duration: Duration::from_millis(10),
            }),
        ];

        let eval = PolicyEvaluator::evaluate(&ctx, &results);
        assert_eq!(eval.decision, ReleaseDecision::Blocked);
        assert!(eval.required_failures.contains(&GateId::Replay1));
    }
}
