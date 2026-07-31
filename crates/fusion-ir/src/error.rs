use crate::validate::{ValidationError, ValidationReport};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WorkflowIrError {
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("validation error: {0}")]
    Validation(#[from] ValidationError),
    #[error("workflow is not valid: {0:?}")]
    InvalidWorkflow(ValidationReport),
}
