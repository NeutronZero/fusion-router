use fusion_capability_macros::capability;

#[capability(
    id = "test.unknown",
    description = "unknown attr",
    version = "0.1.0",
    unknown_field = "value"
)]
struct UnknownAttr;
