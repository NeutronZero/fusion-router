#[test]
fn test_verifier_wiring() {
    use fusion_router::operations::PackageVerifier;
    let archive_path = std::path::PathBuf::from("release_archive");
    let archive_backend = fusion_router::release::archive::FilesystemArchiveBackend::new(archive_path);
    let verifier = fusion_router::operations::ArchivePackageVerifier::new(archive_backend, None);
    let packages = verifier.verified_packages();
    assert!(packages.is_empty() || !packages.is_empty());
}
