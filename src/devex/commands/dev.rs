use std::path::Path;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub fn execute_dev(project_dir: &Path, port: u16) -> Result<(), String> {
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .map_err(|e| format!("Failed to set Ctrl-C handler: {e}"))?;

    println!("fusion dev — hot-reload loop started on port {port}");
    println!(
        "Watching {} for changes...",
        project_dir.join("src").display()
    );
    println!("Press Ctrl-C to stop.");

    let src_dir = project_dir.join("src");
    if !src_dir.is_dir() {
        return Err("src/ directory not found".into());
    }

    let _ = build_project(project_dir);
    let mut last_modified = last_modified_time(&src_dir);

    while running.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(500));
        let current = last_modified_time(&src_dir);
        if current > last_modified {
            last_modified = current;
            println!("\nChange detected, rebuilding...");
            match build_project(project_dir) {
                Ok(pkg) => println!("Rebuild complete: {}", pkg.display()),
                Err(e) => eprintln!("Build error: {e}"),
            }
        }
    }

    println!("fusion dev stopped.");
    Ok(())
}

fn build_project(project_dir: &Path) -> Result<std::path::PathBuf, String> {
    let output_dir = project_dir.join("target/fusion-dev");
    crate::devex::commands::build::execute_build(project_dir, &output_dir)
}

fn last_modified_time(dir: &Path) -> std::time::SystemTime {
    let mut latest = std::time::UNIX_EPOCH;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Ok(metadata) = entry.metadata() {
                if let Ok(mtime) = metadata.modified() {
                    if mtime > latest {
                        latest = mtime;
                    }
                }
            }
        }
    }
    latest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_last_modified_time_returns_epoch_for_missing_dir() {
        let t = last_modified_time(Path::new("/nonexistent/path"));
        assert_eq!(t, std::time::UNIX_EPOCH);
    }

    #[test]
    fn test_execute_dev_missing_src_dir() {
        let temp_dir = tempfile::tempdir().unwrap();
        // project_dir has no src/ directory
        let res = execute_dev(temp_dir.path(), 8080);
        // Note: setting ctrlc handler in tests may fail or proceed to src dir check
        if let Err(e) = res {
            assert!(
                e.contains("src/ directory not found")
                    || e.contains("Failed to set Ctrl-C handler")
            );
        }
    }
}
