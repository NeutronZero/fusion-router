//! Capability Subsystem (`src/capability/mod.rs`)
//!
//! Provides the registry, descriptor, and permission types for the Capability Platform.

pub mod permission;
pub mod registry;

// Re-exports for downstream consumers — used in future tasks
#[allow(unused_imports)]
pub use registry::{
    CapabilityRegistry,
    CapabilityDescriptor,
    CapabilitySource,
    InMemoryCapabilityRegistry,
    RegistryError,
};
