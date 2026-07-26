use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ArtifactKind {
    Debate,
    Consensus,
    Reflection,
    Generic,
}

pub trait Artifact: Send + Sync + std::fmt::Debug {
    fn clone_box(&self) -> Box<dyn Artifact>;
    fn version(&self) -> u16;
    fn kind(&self) -> ArtifactKind;
    fn artifact_type(&self) -> &'static str {
        match self.kind() {
            ArtifactKind::Debate => "debate",
            ArtifactKind::Consensus => "consensus",
            ArtifactKind::Reflection => "reflection",
            ArtifactKind::Generic => "generic",
        }
    }
}

impl Clone for Box<dyn Artifact> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}
