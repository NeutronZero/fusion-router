//! Intentionally small prelude for capability plugin authors.

pub use fusion_capability_macros::capability;

pub use fusion_plugin_api::{
    CapabilityPlugin,
    CapabilityContract,
    CapabilityId,
};

pub use crate::{
    CapabilityBuilder,
    CapabilityManifestBuilder,
};
