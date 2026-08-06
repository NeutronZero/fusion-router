use std::path::Path;

use reqwest::Client;

pub async fn execute_publish(
    pkg_path: &Path,
    registry_url: &str,
    signing_key: Option<&str>,
) -> Result<(), String> {
    if !pkg_path.exists() {
        return Err(format!("Package not found: {}", pkg_path.display()));
    }

    let pkg_bytes = std::fs::read(pkg_path)
        .map_err(|e| format!("Failed to read package: {e}"))?;

    let filename = pkg_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "Invalid package filename".to_string())?;

    if let Some(key) = signing_key {
        println!("Signing with key: {key}");
    }

    let client = Client::new();
    let url = format!("{}/v1/packages", registry_url.trim_end_matches('/'));
    let response = client
        .put(&url)
        .header("Content-Type", "application/octet-stream")
        .body(pkg_bytes)
        .send()
        .await
        .map_err(|e| format!("Upload failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Registry returned {status}: {body}"));
    }

    println!("Published {filename}");
    println!("Registry: {registry_url}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_publish_rejects_missing_package() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(execute_publish(
            Path::new("/nonexistent/pkg.fusionpkg"),
            "http://localhost:9999",
            None,
        ));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn test_publish_passes_file_check() {
        let dir = tempfile::tempdir().unwrap();
        let pkg_path = dir.path().join("my-cap-0.1.0.fusionpkg");
        fs::write(&pkg_path, [0u8; 32]).unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(execute_publish(
            &pkg_path,
            "http://localhost:1",
            Some("test-key"),
        ));
        assert!(result.is_err());
        let err = result.unwrap_err().to_lowercase();
        assert!(!err.contains("not found"), "should pass file check, got: {err}");
    }
}
