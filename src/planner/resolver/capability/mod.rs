pub mod lowerer;
pub mod resolver;

// Graph types are canonical in fusion_kernel — re-export for backward compatibility
#[allow(unused_imports)]
pub use fusion_kernel::capability::{CapabilityGraph, CapabilityNode, DependencyEdge, ConflictEdge};
#[allow(unused_imports)]
pub use lowerer::CapabilityGraphLowerer;
#[allow(unused_imports)]
pub use resolver::{CapabilityResolver, CapabilityPlannerCache, RequirementSet, ResolvedCapabilitySet, ResolverError};
