/// Thin test helper — re-exports production fixture loader.
pub use fusion_router::release::fixture::*;
pub use fusion_router::release::fixture_loader::{discover_fixtures, load_fixture_manifest, FixtureLoader};

use fusion_router::release::gate::GateError;
use std::path::Path;

/// Convenience wrapper: create a FixtureLoader from a test directory path.
pub fn test_loader(root: &Path) -> FixtureLoader {
    FixtureLoader::new(root.to_path_buf())
}
