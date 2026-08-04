use std::fs;
use std::path::Path;

#[test]
fn test_architecture_manifest_is_frozen() {
    let manifest_path = Path::new("docs/architecture/manifest.yaml");
    assert!(manifest_path.exists(), "docs/architecture/manifest.yaml must exist");

    let content = fs::read_to_string(manifest_path).expect("Read manifest.yaml");
    assert!(content.contains("architecture_version: AF-005"), "Must be AF-005 architecture version");
    assert!(content.contains("repository_structure_freeze: frozen"), "Repository layout must be frozen");
    assert!(content.contains("status: frozen"), "Architecture status must be frozen");
}

#[test]
fn test_architectural_invariants_exist() {
    let invariants_path = Path::new("docs/architecture/invariants.md");
    assert!(invariants_path.exists(), "docs/architecture/invariants.md must exist");

    let content = fs::read_to_string(invariants_path).expect("Read invariants.md");
    for i in 1..=10 {
        assert!(content.contains(&format!("Invariant {i}:")), "Must document Invariant {i}");
    }
}

#[test]
fn test_adrs_are_complete() {
    for adr_num in 1..=8 {
        let adr_pattern = format!("ADR-00{adr_num}");
        let adr_dir = Path::new("docs/adr");
        assert!(adr_dir.exists(), "docs/adr directory must exist");
        
        let found = fs::read_dir(adr_dir)
            .expect("Read adr dir")
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains(&adr_pattern));
        
        assert!(found, "ADR-00{adr_num} must exist in docs/adr/");
    }
}

#[test]
fn test_governance_specs_exist() {
    let gov_files = [
        "docs/governance/architecture-process.md",
        "docs/governance/adr-process.md",
        "docs/governance/release-policy.md",
    ];

    for file in &gov_files {
        assert!(Path::new(file).exists(), "Governance spec {file} must exist");
    }
}

#[test]
fn test_3_tier_workspace_members_exist() {
    let foundation = ["crates/fusion-core", "crates/fusion-kernel", "crates/fusion-api-internal"];
    let engine = ["crates/fusion-planner", "crates/fusion-compiler", "crates/fusion-scheduler", "crates/fusion-runtime"];
    let platform = ["crates/fusion-infrastructure", "crates/fusion-api-public", "crates/fusion-studio-api", "crates/fusion-worker-protocol", "crates/fusion-worker"];
    let app = ["apps/fusion-server"];

    for p in foundation.iter().chain(engine.iter()).chain(platform.iter()).chain(app.iter()) {
        let cargo_toml = Path::new(p).join("Cargo.toml");
        assert!(cargo_toml.exists(), "Workspace member {p}/Cargo.toml must exist");
    }
}

#[test]
fn test_beta_acceptance_suite_exists() {
    assert!(Path::new("tests/beta_first_run.rs").exists(), "tests/beta_first_run.rs must exist");
    assert!(Path::new("tests/beta_provider_setup.rs").exists(), "tests/beta_provider_setup.rs must exist");
    assert!(Path::new("tests/beta_chat.rs").exists(), "tests/beta_chat.rs must exist");
    assert!(Path::new("tests/beta_inspector.rs").exists(), "tests/beta_inspector.rs must exist");
    assert!(Path::new("tests/beta_integration.rs").exists(), "tests/beta_integration.rs must exist");
    assert!(Path::new("tests/beta_dashboard.rs").exists(), "tests/beta_dashboard.rs must exist");
    assert!(Path::new("tests/beta_health.rs").exists(), "tests/beta_health.rs must exist");
    assert!(Path::new("tests/beta_replay.rs").exists(), "tests/beta_replay.rs must exist");
}

#[test]
fn test_compatibility_suite_exists() {
    assert!(Path::new("tests/compatibility_v1.rs").exists(), "tests/compatibility_v1.rs must exist");
    assert!(Path::new("tests/performance_slo.rs").exists(), "tests/performance_slo.rs must exist");
}

#[test]
fn test_documentation_platform_exists() {
    assert!(Path::new("docs/user/index.md").exists(), "docs/user/index.md must exist");
    assert!(Path::new("docs/operator/index.md").exists(), "docs/operator/index.md must exist");
    assert!(Path::new("docs/developer/handbook.md").exists(), "docs/developer/handbook.md must exist");
    assert!(Path::new("docs/tutorials/index.md").exists(), "docs/tutorials/index.md must exist");
    assert!(Path::new("docs/cookbook/index.md").exists(), "docs/cookbook/index.md must exist");
    assert!(Path::new("docs/governance/v1-readiness-report.md").exists(), "docs/governance/v1-readiness-report.md must exist");
}
