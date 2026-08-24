//! Capability Subsystem (`src/capability/mod.rs`)
//!
//! Provides the registry, descriptor, and permission types for the Capability Platform.
//! Registry types are canonical in `fusion_kernel::capability::registry` —
//! this module re-exports them for backward compatibility.

pub mod permission;

// Re-exports from the canonical crate location
pub use fusion_kernel::capability::{CapabilityRegistry, InMemoryCapabilityRegistry};
