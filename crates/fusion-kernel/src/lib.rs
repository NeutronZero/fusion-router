//! `fusion-kernel`
//!
//! Core kernel types and runtime capability data structures.

pub mod capability;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::broadcast;
use uuid::Uuid;

use fusion_core::{ExecutionId, PlatformStatus};

pub trait DomainEvent: Send + Sync + std::fmt::Debug {
    fn id(&self) -> Uuid;
    fn occurred_at(&self) -> DateTime<Utc>;
    fn aggregate_id(&self) -> String;
    fn version(&self) -> u16;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KernelEvent {
    ExecutionStarted { id: Uuid, execution_id: ExecutionId },
    ExecutionCompleted { id: Uuid, execution_id: ExecutionId },
    NodeStarted { id: Uuid, execution_id: ExecutionId, node_id: String },
    NodeFinished { id: Uuid, execution_id: ExecutionId, node_id: String },
    JobStatusUpdated { id: Uuid, job_id: String, state: String },
    PlatformStatusChanged { id: Uuid, status: PlatformStatus },
}

impl DomainEvent for KernelEvent {
    fn id(&self) -> Uuid {
        match self {
            KernelEvent::ExecutionStarted { id, .. } => *id,
            KernelEvent::ExecutionCompleted { id, .. } => *id,
            KernelEvent::NodeStarted { id, .. } => *id,
            KernelEvent::NodeFinished { id, .. } => *id,
            KernelEvent::JobStatusUpdated { id, .. } => *id,
            KernelEvent::PlatformStatusChanged { id, .. } => *id,
        }
    }

    fn occurred_at(&self) -> DateTime<Utc> {
        Utc::now()
    }

    fn aggregate_id(&self) -> String {
        match self {
            KernelEvent::ExecutionStarted { execution_id, .. } => execution_id.0.to_string(),
            KernelEvent::ExecutionCompleted { execution_id, .. } => execution_id.0.to_string(),
            KernelEvent::NodeStarted { execution_id, .. } => execution_id.0.to_string(),
            KernelEvent::NodeFinished { execution_id, .. } => execution_id.0.to_string(),
            KernelEvent::JobStatusUpdated { job_id, .. } => job_id.clone(),
            KernelEvent::PlatformStatusChanged { .. } => "platform".to_string(),
        }
    }

    fn version(&self) -> u16 {
        1
    }
}

pub struct EventBus {
    sender: broadcast::Sender<KernelEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    pub fn publish(&self, event: KernelEvent) -> Result<usize, broadcast::error::SendError<KernelEvent>> {
        self.sender.send(event)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<KernelEvent> {
        self.sender.subscribe()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionProfile {
    Fast,
    Balanced,
    Cheap,
    Coding,
    Research,
    Vision,
    Reasoning,
    Creative,
    Offline,
}

pub struct CapabilityCatalog {
    catalog: HashMap<String, Vec<String>>,
}

impl CapabilityCatalog {
    pub fn new() -> Self {
        let mut catalog = HashMap::new();
        catalog.insert("Vision".to_string(), vec!["ImageInput".to_string()]);
        catalog.insert("JSON".to_string(), vec!["StructuredOutput".to_string()]);
        catalog.insert("ToolCalling".to_string(), vec!["FunctionCalling".to_string()]);
        catalog.insert("Reasoning".to_string(), vec!["ChainOfThought".to_string()]);
        catalog.insert("Streaming".to_string(), vec!["SSE".to_string()]);
        catalog.insert("Embeddings".to_string(), vec!["VectorEmbedding".to_string()]);
        catalog.insert("Audio".to_string(), vec!["AudioInputOutput".to_string()]);
        catalog.insert("ImageGen".to_string(), vec!["Diffusion".to_string()]);
        catalog.insert("Video".to_string(), vec!["VideoInput".to_string()]);
        catalog.insert("MCP".to_string(), vec!["ModelContextProtocol".to_string()]);
        Self { catalog }
    }

    pub fn supports(&self, capability: &str) -> bool {
        self.catalog.contains_key(capability)
    }
}

impl Default for CapabilityCatalog {
    fn default() -> Self {
        Self::new()
    }
}

pub struct CapabilitySystem {
    capabilities: HashMap<String, Vec<String>>,
}

impl CapabilitySystem {
    pub fn new() -> Self {
        let mut capabilities = HashMap::new();
        capabilities.insert("Reasoning".to_string(), vec!["LLM".to_string()]);
        capabilities.insert("Vision".to_string(), vec!["ImageInput".to_string()]);
        capabilities.insert("ToolUse".to_string(), vec!["FunctionCalling".to_string()]);
        capabilities.insert("Artifacts".to_string(), vec!["FileWrite".to_string()]);
        capabilities.insert("Search".to_string(), vec!["WebSearch".to_string()]);
        capabilities.insert("LongContext".to_string(), vec!["128kTokens".to_string()]);
        Self { capabilities }
    }

    pub fn supports(&self, capability: &str) -> bool {
        self.capabilities.contains_key(capability)
    }
}

impl Default for CapabilitySystem {
    fn default() -> Self {
        Self::new()
    }
}

pub struct SystemCatalog {
    pub providers: Vec<String>,
    pub models: Vec<String>,
}

impl SystemCatalog {
    pub fn new() -> Self {
        Self {
            providers: vec!["openrouter".to_string(), "zen".to_string(), "ollama".to_string()],
            models: vec!["gpt-4o".to_string(), "claude-3-5-sonnet".to_string(), "llama3".to_string()],
        }
    }
}

impl Default for SystemCatalog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_registry_and_execution_profiles() {
        let reg = CapabilityCatalog::new();
        assert!(reg.supports("Vision"));
        assert!(reg.supports("MCP"));
        assert!(reg.supports("ToolCalling"));

        let profile = ExecutionProfile::Balanced;
        assert_eq!(profile, ExecutionProfile::Balanced);
    }
}
