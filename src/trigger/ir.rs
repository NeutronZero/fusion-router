//! Phase 6B — `TriggerIR` (`src/trigger/ir.rs`)

use serde::{Deserialize, Serialize};
use crate::trigger::types::TriggerDeclaration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerIR {
    pub triggers: Vec<TriggerDeclaration>,
}

impl TriggerIR {
    pub fn new(triggers: Vec<TriggerDeclaration>) -> Self {
        Self { triggers }
    }
}
