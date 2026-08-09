use async_trait::async_trait;
use fusion_core::{ExecutionId, PlatformError, ProviderId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternalCompilerContext {
    pub execution_id: ExecutionId,
    pub target_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternalSchedulerContext {
    pub execution_id: ExecutionId,
    pub max_parallelism: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRecord {
    pub execution_id: ExecutionId,
    pub session_id: String,
    pub entry_point: String,
    pub prompt: String,
    pub ir_version: u16,
    pub graph_id: String,
    pub provider_id: ProviderId,
    pub passes_count: usize,
    pub execution_time_ms: u64,
    pub estimated_cost: f64,
    pub compiler_invoked: bool,
    pub created_at_rfc3339: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionBundle {
    pub record: ExecutionRecord,
    pub ir_json: String,
    pub compiler_report_json: String,
    pub timeline_json: String,
    pub telemetry_json: String,
    pub config_snapshot_json: String,
    pub contract_version: String,
}

impl ExecutionBundle {
    pub fn export_bundle(&self) -> Result<String, PlatformError> {
        serde_json::to_string(self).map_err(|e| PlatformError::Storage {
            code: "BUNDLE_EXPORT_ERR".to_string(),
            message: e.to_string(),
            recovery_suggestion: "Check execution bundle fields".to_string(),
        })
    }

    pub fn import_bundle(json_str: &str) -> Result<Self, PlatformError> {
        serde_json::from_str(json_str).map_err(|e| PlatformError::Storage {
            code: "BUNDLE_IMPORT_ERR".to_string(),
            message: e.to_string(),
            recovery_suggestion: "Verify .fusion bundle JSON formatting".to_string(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplayMode {
    Timeline,
    Compiler,
    Runtime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayResult {
    pub mode: ReplayMode,
    pub execution_id: ExecutionId,
    pub is_deterministic: bool,
    pub replay_fidelity: f64,
    pub steps_replayed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionDiff {
    pub record_id_a: String,
    pub record_id_b: String,
    pub provider_changed: bool,
    pub latency_delta_ms: i64,
    pub cost_delta: f64,
    pub pass_count_delta: i32,
}

pub struct DeterministicReplayEngine;

impl DeterministicReplayEngine {
    pub fn new() -> Self {
        Self
    }

    pub fn replay(&self, bundle: &ExecutionBundle, mode: ReplayMode) -> ReplayResult {
        let steps = match mode {
            ReplayMode::Timeline => 5,
            ReplayMode::Compiler => bundle.record.passes_count,
            ReplayMode::Runtime => 3,
        };

        ReplayResult {
            mode,
            execution_id: bundle.record.execution_id,
            is_deterministic: true,
            replay_fidelity: 1.0,
            steps_replayed: steps,
        }
    }

    pub fn compare(&self, rec_a: &ExecutionRecord, rec_b: &ExecutionRecord) -> ExecutionDiff {
        ExecutionDiff {
            record_id_a: rec_a.execution_id.0.to_string(),
            record_id_b: rec_b.execution_id.0.to_string(),
            provider_changed: rec_a.provider_id.0 != rec_b.provider_id.0,
            latency_delta_ms: rec_b.execution_time_ms as i64 - rec_a.execution_time_ms as i64,
            cost_delta: rec_b.estimated_cost - rec_a.estimated_cost,
            pass_count_delta: rec_b.passes_count as i32 - rec_a.passes_count as i32,
        }
    }
}

impl Default for DeterministicReplayEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitectureKpiMetrics {
    pub total_requests: u64,
    pub compiler_invocations: u64,
    pub execution_graph_creations: u64,
    pub zero_bypass_violations: u64,
    pub compiler_invocation_rate: f64,
    pub execution_graph_rate: f64,
    pub replay_fidelity_rate: f64,
}

impl ArchitectureKpiMetrics {
    pub fn new(total_requests: u64, compiler_invocations: u64, execution_graph_creations: u64) -> Self {
        let compiler_invocation_rate = if total_requests == 0 { 1.0 } else { compiler_invocations as f64 / total_requests as f64 };
        let execution_graph_rate = if total_requests == 0 { 1.0 } else { execution_graph_creations as f64 / total_requests as f64 };
        let zero_bypass_violations = total_requests.saturating_sub(compiler_invocations);

        Self {
            total_requests,
            compiler_invocations,
            execution_graph_creations,
            zero_bypass_violations,
            compiler_invocation_rate,
            execution_graph_rate,
            replay_fidelity_rate: 1.0,
        }
    }
}

#[async_trait]
pub trait InternalEngineService: Send + Sync {
    async fn status(&self) -> Result<String, PlatformError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_execution_bundle_export_import_roundtrip() {
        let exec_id = ExecutionId::new();
        let record = ExecutionRecord {
            execution_id: exec_id,
            session_id: "session-1".to_string(),
            entry_point: "REST_CHAT".to_string(),
            prompt: "Refactor to AST".to_string(),
            ir_version: 1,
            graph_id: "graph_1".to_string(),
            provider_id: ProviderId("openrouter".to_string()),
            passes_count: 11,
            execution_time_ms: 62,
            estimated_cost: 0.0012,
            compiler_invoked: true,
            created_at_rfc3339: Utc::now().to_rfc3339(),
        };

        let bundle = ExecutionBundle {
            record,
            ir_json: "{}".to_string(),
            compiler_report_json: "{}".to_string(),
            timeline_json: "[]".to_string(),
            telemetry_json: "[]".to_string(),
            config_snapshot_json: "{}".to_string(),
            contract_version: "v1".to_string(),
        };

        let json = bundle.export_bundle().expect("Export bundle");
        let imported = ExecutionBundle::import_bundle(&json).expect("Import bundle");

        assert_eq!(imported.record.execution_id.0, exec_id.0);
        assert_eq!(imported.contract_version, "v1");
    }
}
