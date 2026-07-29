use fusion_capability_macros::capability;

#[capability(
    id = "test.bad",
    description = "bad version",
    version = "not-a-version"
)]
struct BadVersion;
