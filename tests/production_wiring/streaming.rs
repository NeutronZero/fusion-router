#[test]
fn test_direct_streaming_flag_default() {
    let direct_allowed = std::env::var("FUSION_EXPERIMENTAL_DIRECT_STREAM")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);
    assert!(
        !direct_allowed,
        "FUSION_EXPERIMENTAL_DIRECT_STREAM must be disabled by default"
    );
}
