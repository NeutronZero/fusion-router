use fusion_capability_macros::capability;

#[capability(
    id = "test.bad.perm",
    description = "bad permission",
    version = "0.1.0"
)]
#[permission(UnknownVariant)]
struct BadPermission;
