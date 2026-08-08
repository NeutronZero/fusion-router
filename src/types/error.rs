use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineStage {
    ContextAssembly,
    RequirementsExtraction,
    EvidenceSnapshot,
    Planning,
    Compilation,
    ResourceReservation,
    Scheduling,
    Execution,
    TelemetryRecording,
    ResponseBuilding,
}

impl fmt::Display for PipelineStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ContextAssembly => write!(f, "ContextAssembly"),
            Self::RequirementsExtraction => write!(f, "RequirementsExtraction"),
            Self::EvidenceSnapshot => write!(f, "EvidenceSnapshot"),
            Self::Planning => write!(f, "Planning"),
            Self::Compilation => write!(f, "Compilation"),
            Self::ResourceReservation => write!(f, "ResourceReservation"),
            Self::Scheduling => write!(f, "Scheduling"),
            Self::Execution => write!(f, "Execution"),
            Self::TelemetryRecording => write!(f, "TelemetryRecording"),
            Self::ResponseBuilding => write!(f, "ResponseBuilding"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum RouterError {
    StageFailure {
        stage: PipelineStage,
        request_id: Uuid,
        message: String,
    },
    ResourceExhausted {
        request_id: Uuid,
        details: String,
    },
    CapacityExceeded {
        request_id: Uuid,
        details: String,
    },
    ClientCancelled {
        request_id: Uuid,
    },
    BudgetExceeded {
        stage: PipelineStage,
        request_id: Uuid,
        detail: String,
    },
    MaxIterationsExceeded {
        stage: PipelineStage,
        request_id: Uuid,
        current: u64,
        max: u32,
    },
    Internal {
        request_id: Uuid,
        message: String,
    },
}

impl fmt::Display for RouterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StageFailure { stage, request_id, message } => {
                write!(f, "[{}] Stage '{}' failed: {}", request_id, stage, message)
            }
            Self::ResourceExhausted { request_id, details } => {
                write!(f, "[{}] Resource quota exhausted: {}", request_id, details)
            }
            Self::CapacityExceeded { request_id, details } => {
                write!(f, "[{}] Router capacity exceeded: {}", request_id, details)
            }
            Self::ClientCancelled { request_id } => {
                write!(f, "[{}] Client disconnected or cancelled request", request_id)
            }
            Self::BudgetExceeded { request_id, detail, .. } => {
                write!(f, "[{}] Budget exceeded: {}", request_id, detail)
            }
            Self::MaxIterationsExceeded { request_id, current, max, .. } => {
                write!(f, "[{}] Max iterations exceeded: {} > {}", request_id, current, max)
            }
            Self::Internal { request_id, message } => {
                write!(f, "[{}] Internal error: {}", request_id, message)
            }
        }
    }
}

impl std::error::Error for RouterError {}

impl RouterError {
    pub fn request_id(&self) -> Uuid {
        match self {
            Self::StageFailure { request_id, .. } => *request_id,
            Self::ResourceExhausted { request_id, .. } => *request_id,
            Self::CapacityExceeded { request_id, .. } => *request_id,
            Self::ClientCancelled { request_id } => *request_id,
            Self::BudgetExceeded { request_id, .. } => *request_id,
            Self::MaxIterationsExceeded { request_id, .. } => *request_id,
            Self::Internal { request_id, .. } => *request_id,
        }
    }

    pub fn stage(&self) -> Option<PipelineStage> {
        match self {
            Self::StageFailure { stage, .. } => Some(*stage),
            Self::ResourceExhausted { .. } => Some(PipelineStage::ResourceReservation),
            Self::CapacityExceeded { .. } => Some(PipelineStage::Scheduling),
            Self::ClientCancelled { .. } => Some(PipelineStage::Execution),
            Self::BudgetExceeded { stage, .. } => Some(*stage),
            Self::MaxIterationsExceeded { stage, .. } => Some(*stage),
            Self::Internal { .. } => None,
        }
    }

    pub fn status_code(&self) -> axum::http::StatusCode {
        match self {
            Self::StageFailure { .. } => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Self::ResourceExhausted { .. } => axum::http::StatusCode::TOO_MANY_REQUESTS,
            Self::CapacityExceeded { .. } => axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Self::ClientCancelled { .. } => axum::http::StatusCode::BAD_REQUEST,
            Self::BudgetExceeded { .. } => axum::http::StatusCode::TOO_MANY_REQUESTS,
            Self::MaxIterationsExceeded { .. } => axum::http::StatusCode::TOO_MANY_REQUESTS,
            Self::Internal { .. } => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Client-safe message. Internal provider/transport detail stays in the
    /// server logs (`Display`); the response must not leak paths, provider
    /// internals, or configuration.
    pub fn user_message(&self) -> String {
        match self {
            Self::StageFailure { .. } => {
                "request failed during pre-execution processing".to_string()
            }
            Self::ResourceExhausted { .. } => "daily resource quota exhausted".to_string(),
            Self::CapacityExceeded { .. } => {
                "router capacity temporarily exceeded; retry later".to_string()
            }
            Self::ClientCancelled { .. } => "request cancelled".to_string(),
            Self::BudgetExceeded { .. } => "budget exceeded for this request".to_string(),
            Self::MaxIterationsExceeded { .. } => {
                "workflow exceeded its iteration limit".to_string()
            }
            Self::Internal { .. } => "internal error".to_string(),
        }
    }
}
