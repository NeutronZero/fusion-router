use std::fmt;

#[derive(Debug, Clone)]
pub enum ValidationSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone)]
pub struct ConfigValidationError {
    pub field: String,
    pub message: String,
    pub value: Option<String>,
    pub severity: ValidationSeverity,
}

impl fmt::Display for ConfigValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{:?}] {}: {}", self.severity, self.field, self.message)
    }
}

#[derive(Debug)]
pub enum ReloadError {
    Parse(String),
    Validation(Vec<ConfigValidationError>),
    Subscriber { name: String, reason: String },
    ConnectorError(String),
}

impl fmt::Display for ReloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReloadError::Parse(msg) => write!(f, "parse error: {msg}"),
            ReloadError::Validation(errors) => {
                write!(f, "validation failed ({} errors)", errors.len())
            }
            ReloadError::Subscriber { name, reason } => {
                write!(f, "subscriber '{name}' rejected: {reason}")
            }
            ReloadError::ConnectorError(msg) => write!(f, "connector error: {msg}"),
        }
    }
}

impl std::error::Error for ReloadError {}
