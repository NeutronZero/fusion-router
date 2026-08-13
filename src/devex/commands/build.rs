use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use flate2::write::GzEncoder;
use flate2::Compression;

pub fn execute_build(project_dir: &Path, output_dir: &Path) -> Result<PathBuf, String> {
    let manifest_path = project_dir.join("Cargo.toml");
    if !manifest_path.exists() {
        return Err("No Cargo.toml found in project directory".into());
    }

    let wasm_file = match find_wasm_file(project_dir) {
        Ok(f) => f,
        Err(_) => {
            let status = Command::new("cargo")
                .args(["build", "--target", "wasm32-wasi", "--release"])
                .current_dir(project_dir)
                .status()
                .map_err(|e| format!("Failed to run cargo build: {e}"))?;

            if !status.success() {
                return Err("cargo build failed".into());
            }

            find_wasm_file(project_dir)?
        }
    };

    let wasm_path = project_dir
        .join("target")
        .join("wasm32-wasi")
        .join("release")
        .join(&wasm_file);

    let stripped = try_strip_wasm(&wasm_path);

    let optimized = try_wasm_opt(&stripped, project_dir);

    fs::create_dir_all(output_dir)
        .map_err(|e| format!("Failed to create output dir: {e}"))?;

    let manifest_path = project_dir.join("manifest.toml");
    if !manifest_path.exists() {
        return Err("manifest.toml not found in project directory".into());
    }

    let manifest_content =
        fs::read_to_string(&manifest_path).map_err(|e| format!("Failed to read manifest: {e}"))?;
    let pkg_name = extract_package_name(&manifest_content, project_dir);
    let pkg_version = extract_package_version(&project_dir.join("Cargo.toml"));

    let pkg_filename = format!("{pkg_name}-{pkg_version}.fusionpkg");
    let pkg_path = output_dir.join(&pkg_filename);

    let wasm_bytes = fs::read(&optimized).map_err(|e| format!("Failed to read WASM: {e}"))?;
    let attestation = "{}";

    let file = fs::File::create(&pkg_path).map_err(|e| format!("Failed to create package: {e}"))?;
    let encoder = GzEncoder::new(file, Compression::best());
    let mut archive = tar::Builder::new(encoder);

    let mut manifest_file =
        fs::File::open(&manifest_path).map_err(|e| format!("Failed to open manifest: {e}"))?;
    archive
        .append_file("manifest.toml", &mut manifest_file)
        .map_err(|e| format!("Failed to add manifest: {e}"))?;

    let mut wasm_header = tar::Header::new_gnu();
    wasm_header.set_path("module.wasm").map_err(|e| format!("Failed to set header path: {e}"))?;
    wasm_header.set_size(wasm_bytes.len() as u64);
    wasm_header.set_mode(0o444);
    wasm_header.set_cksum();
    archive
        .append_data(&mut wasm_header, "module.wasm", &wasm_bytes[..])
        .map_err(|e| format!("Failed to add module: {e}"))?;

    let mut attestation_header = tar::Header::new_gnu();
    attestation_header.set_path("attestation.json").map_err(|e| format!("Failed to set header path: {e}"))?;
    attestation_header.set_size(attestation.len() as u64);
    attestation_header.set_mode(0o444);
    attestation_header.set_cksum();
    archive
        .append_data(&mut attestation_header, "attestation.json", attestation.as_bytes())
        .map_err(|e| format!("Failed to add attestation: {e}"))?;

    let encoder = archive.into_inner().map_err(|e| format!("Failed to finalize archive: {e}"))?;
    encoder.finish().map_err(|e| format!("Failed to write archive: {e}"))?;

    println!("Created package: {}", pkg_path.display());
    Ok(pkg_path)
}

fn try_strip_wasm(path: &Path) -> PathBuf {
    let tmp = path.with_extension("stripped.wasm");
    let status = Command::new("wasm-strip")
        .args([path.to_string_lossy().as_ref(), tmp.to_string_lossy().as_ref()])
        .status();
    match status {
        Ok(s) if s.success() => tmp,
        _ => path.to_path_buf(),
    }
}

fn try_wasm_opt(path: &Path, project_dir: &Path) -> PathBuf {
    let tmp = project_dir.join("target/optimized.wasm");
    let status = Command::new("wasm-opt")
        .args(["-O2", path.to_string_lossy().as_ref(), "-o", tmp.to_string_lossy().as_ref()])
        .status();
    match status {
        Ok(s) if s.success() => tmp,
        _ => path.to_path_buf(),
    }
}

fn find_wasm_file(project_dir: &Path) -> Result<String, String> {
    let release_dir = project_dir.join("target/wasm32-wasi/release");
    if !release_dir.is_dir() {
        return Err("WASM release directory not found".into());
    }
    for entry in fs::read_dir(&release_dir).map_err(|e| format!("Cannot read release dir: {e}"))? {
        let entry = entry.map_err(|e| format!("Cannot read entry: {e}"))?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "wasm") {
            return Ok(entry.file_name().to_string_lossy().to_string());
        }
    }
    Err("No .wasm file found in target/wasm32-wasi/release".into())
}

fn extract_package_name(manifest_toml: &str, project_dir: &Path) -> String {
    for line in manifest_toml.lines() {
        if let Some(name) = line.strip_prefix("name = ") {
            return name.trim_matches('"').to_string();
        }
    }
    if let Ok(cargo) = fs::read_to_string(project_dir.join("Cargo.toml")) {
        for line in cargo.lines() {
            if let Some(name) = line.strip_prefix("name = ") {
                return name.trim_matches('"').to_string();
            }
        }
    }
    "unknown".to_string()
}

fn extract_package_version(cargo_toml: &Path) -> String {
    if let Ok(content) = fs::read_to_string(cargo_toml) {
        for line in content.lines() {
            if let Some(ver) = line.strip_prefix("version = ") {
                return ver.trim_matches('"').to_string();
            }
        }
    }
    "0.0.0".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_build_produces_fusionpkg() {
        let dir = tempfile::tempdir().unwrap();

        let cargo_toml = r#"[package]
name = "test-cap"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]
"#;
        fs::write(dir.path().join("Cargo.toml"), cargo_toml).unwrap();

        let manifest_toml = r#"[package]
name = "test-cap"
version = "0.1.0"
"#;
        fs::write(dir.path().join("manifest.toml"), manifest_toml).unwrap();

        let wasm_dir = dir.path().join("target/wasm32-wasi/release");
        fs::create_dir_all(&wasm_dir).unwrap();
        let fake_wasm = &[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        fs::write(wasm_dir.join("test_cap.wasm"), fake_wasm).unwrap();

        let output_dir = dir.path().join("output");
        let result = execute_build(dir.path(), &output_dir);
        assert!(result.is_ok(), "build failed: {:?}", result.err());

        let pkg_path = result.unwrap();
        assert!(pkg_path.exists());
        assert_eq!(pkg_path.extension().unwrap(), "fusionpkg");
        assert!(pkg_path.to_string_lossy().contains("test-cap-0.1.0"));
    }
}
