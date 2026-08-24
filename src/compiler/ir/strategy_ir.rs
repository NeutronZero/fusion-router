use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DebateRole {
    pub name: String,
    pub model: String,
    pub stance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StrategyIR {
    Single,
    Consensus {
        count: u32,
        #[serde(default)]
        members: Vec<String>,
    },
    Reflection {
        max_cycles: u32,
    },
    Debate {
        roles: Vec<DebateRole>,
    },
    ReAct {
        max_iterations: u32,
    },
    Chain {
        stages: Vec<StrategyIR>,
    },
    Custom {
        name: String,
        config: serde_json::Value,
    },
}
