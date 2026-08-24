use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CompilationContext {
    pub target_environment: String,
    pub available_models: Vec<String>,
    pub max_parallelism: u32,
    pub extra: HashMap<String, serde_json::Value>,
}

impl CompilationContext {
    pub fn new() -> Self {
        Self {
            target_environment: "default".into(),
            available_models: Vec::new(),
            max_parallelism: 16,
            extra: HashMap::new(),
        }
    }
}
