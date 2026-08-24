pub mod builder;
pub mod manifest;
pub mod prelude;
pub mod schema;

pub use builder::CapabilityBuilder;
pub use manifest::CapabilityManifestBuilder;
pub use prelude::*;
pub use schema::SchemaBuilder;

/// Re-exports for macro-generated code.
/// Not part of the public API — use prelude instead.
#[doc(hidden)]
pub mod __reexports {
    pub use semver;
    pub use serde_json;
}
