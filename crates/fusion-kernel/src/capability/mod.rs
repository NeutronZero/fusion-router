//! Capability data structures shared by the kernel and planner.
//!
//! These are planner infrastructure types: the dependency/conflict DAG and
//! the (trait-based) capability registry. They live here so that
//! `fusion-planner`'s resolver (a consumer of both) does not need to
//! depend on any monolith-internal crate.

pub mod graph;
pub mod registry;

pub use graph::{CapabilityGraph, CapabilityNode, ConflictEdge, DependencyEdge};
pub use registry::{
    CapabilityDescriptor, CapabilityRegistry, CapabilitySource, InMemoryCapabilityRegistry,
    RegistryError,
};