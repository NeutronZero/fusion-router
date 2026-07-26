use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ArtifactKind {
    Debate,
    Consensus,
    Reflection,
    Generic,
}

pub trait Artifact: Send + Sync {
    fn version(&self) -> u16;
    fn kind(&self) -> ArtifactKind;
}
