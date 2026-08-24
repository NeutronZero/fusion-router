//! Phase 4A1 — `PolicyDiagnostic` (`src/policy/diagnostics.rs`)
//!
//! Structured compiler diagnostics for policy syntax, validation, and rule conflicts.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyDiagnostic {
    pub severity: DiagnosticSeverity,
    pub location: String,
    pub rule: Option<String>,
    pub message: String,
}

impl PolicyDiagnostic {
    pub fn error(
        location: impl Into<String>,
        rule: Option<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity: DiagnosticSeverity::Error,
            location: location.into(),
            rule,
            message: message.into(),
        }
    }

    pub fn warning(
        location: impl Into<String>,
        rule: Option<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity: DiagnosticSeverity::Warning,
            location: location.into(),
            rule,
            message: message.into(),
        }
    }
}
