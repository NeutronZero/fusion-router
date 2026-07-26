//! Phase 7C — `UnifiedDiagnosticsEnvelope` (`src/telemetry/unified_diagnostics.rs`)
//!
//! Aggregates compiler, policy, runtime, and session diagnostics into a common diagnostic envelope.

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::policy::diagnostics::PolicyDiagnostic;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiagnosticCategory {
    Compiler,
    Policy,
    Runtime,
    Session,
    Trigger,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedDiagnostic {
    pub category: DiagnosticCategory,
    pub severity: String,
    pub source: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedDiagnosticsEnvelope {
    pub envelope_id: Uuid,
    pub timestamp_ms: u64,
    pub diagnostics: Vec<UnifiedDiagnostic>,
}

impl UnifiedDiagnosticsEnvelope {
    pub fn new() -> Self {
        Self {
            envelope_id: Uuid::new_v4(),
            timestamp_ms: 1000,
            diagnostics: Vec::new(),
        }
    }

    pub fn add_policy_diagnostic(&mut self, diag: PolicyDiagnostic) {
        self.diagnostics.push(UnifiedDiagnostic {
            category: DiagnosticCategory::Policy,
            severity: format!("{:?}", diag.severity),
            source: diag.location,
            message: diag.message,
        });
    }
}

impl Default for UnifiedDiagnosticsEnvelope {
    fn default() -> Self {
        Self::new()
    }
}
