use fusion_core::{ExecutionId, JobId, PlatformError, ProviderId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProviderRequest {
    pub name: String,
    pub api_key: String,
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderResponse {
    pub name: String,
    pub enabled: bool,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderUpdatedEvent {
    pub name: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    CreateProvider(CreateProviderRequest),
    SaveConfig { author: String, config_json: String },
    ExecuteWorkflow { intent: String },
    CancelJob { job_id: JobId },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandResult {
    ProviderCreated { provider_id: ProviderId },
    ConfigSaved { version_id: i64 },
    WorkflowExecutionStarted { execution_id: ExecutionId },
    JobCancelled { job_id: JobId },
}

pub struct CommandBus;

impl CommandBus {
    pub fn new() -> Self {
        Self
    }

    pub fn dispatch(&self, command: Command) -> Result<CommandResult, PlatformError> {
        match command {
            Command::CreateProvider(req) => {
                if req.name.trim().is_empty() {
                    return Err(PlatformError::Security {
                        code: "INVALID_PROVIDER_NAME".to_string(),
                        message: "Provider name cannot be empty".to_string(),
                        recovery_suggestion: "Provide a valid provider identifier".to_string(),
                    });
                }
                Ok(CommandResult::ProviderCreated {
                    provider_id: ProviderId(req.name.to_lowercase()),
                })
            }
            Command::SaveConfig { author: _, config_json: _ } => {
                Ok(CommandResult::ConfigSaved { version_id: 1 })
            }
            Command::ExecuteWorkflow { intent: _ } => {
                Ok(CommandResult::WorkflowExecutionStarted {
                    execution_id: ExecutionId::new(),
                })
            }
            Command::CancelJob { job_id } => {
                Ok(CommandResult::JobCancelled { job_id })
            }
        }
    }
}

impl Default for CommandBus {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Query {
    GetProviders,
    GetConfigHistory,
    GetPlatformHealth,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueryResult {
    Providers(Vec<ProviderResponse>),
    ConfigHistoryCount(usize),
    PlatformHealth { status: String },
}

pub struct QueryBus;

impl QueryBus {
    pub fn new() -> Self {
        Self
    }

    pub fn execute(&self, query: Query) -> Result<QueryResult, PlatformError> {
        match query {
            Query::GetProviders => Ok(QueryResult::Providers(vec![ProviderResponse {
                name: "openrouter".to_string(),
                enabled: true,
                status: "Active".to_string(),
            }])),
            Query::GetConfigHistory => Ok(QueryResult::ConfigHistoryCount(1)),
            Query::GetPlatformHealth => Ok(QueryResult::PlatformHealth {
                status: "Ready".to_string(),
            }),
        }
    }
}

impl Default for QueryBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cqrs_command_bus_dispatch() {
        let bus = CommandBus::new();
        let cmd = Command::CreateProvider(CreateProviderRequest {
            name: "OpenRouter".to_string(),
            api_key: "sk-test-key".to_string(),
            base_url: None,
        });

        let res = bus.dispatch(cmd).expect("Dispatch command");
        match res {
            CommandResult::ProviderCreated { provider_id } => {
                assert_eq!(provider_id.0, "openrouter");
            }
            _ => panic!("Unexpected result"),
        }
    }

    #[test]
    fn test_cqrs_query_bus_execute() {
        let bus = QueryBus::new();
        let res = bus.execute(Query::GetPlatformHealth).expect("Execute query");
        match res {
            QueryResult::PlatformHealth { status } => {
                assert_eq!(status, "Ready");
            }
            _ => panic!("Unexpected query result"),
        }
    }
}
