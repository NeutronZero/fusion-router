pub mod builder;
pub mod manifest;
pub mod schema;
pub mod prelude;

pub use builder::CapabilityBuilder;
pub use manifest::CapabilityManifestBuilder;
pub use schema::SchemaBuilder;
pub use prelude::*;

/// Re-exports for macro-generated code.
/// Not part of the public API — use prelude instead.
#[doc(hidden)]
pub mod __reexports {
    pub use fusion_capability_macros as __macros;
    pub use semver;
    pub use serde_json;
}
