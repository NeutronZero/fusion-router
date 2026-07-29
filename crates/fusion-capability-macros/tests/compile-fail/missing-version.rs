use fusion_capability_macros::capability;

#[capability(
    id = "test.missing.version",
    description = "missing version"
)]
struct MissingVersion;
