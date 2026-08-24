//! Capability symbol resolution for the planner.
//!
//! Ported from the monolith's `src/planner/resolver/capability/resolver.rs`
//! (Phase 2B & 2C). Matches intent requirements to frozen contracts, expands
//! transitive dependencies, and defends every resolution path with policy
//! checks (H13 / ADR-034).

pub mod resolver;

pub use resolver::{
    CapabilityPlannerCache, CapabilityResolver, PolicyContext, RequirementSet,
    ResolvedCapabilitySet, ResolverError, VersionConstraint,
};
