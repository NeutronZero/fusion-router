use std::path::Path;

use crate::devex::scaffold::PluginScaffolder;

pub fn execute_new(name: &str, path: &Path) -> Result<(), String> {
    let scaffolder = PluginScaffolder::new();
    scaffolder
        .scaffold_capability(path, name)
        .map_err(|e| format!("Failed to scaffold capability: {e}"))?;
    println!("Created capability project '{name}' at {}", path.join(name).display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_new_scaffolds_project_structure() {
        let dir = tempfile::tempdir().unwrap();
        let name = "test-cap";
        execute_new(name, dir.path()).unwrap();

        let project = dir.path().join(name);
        assert!(project.join("Cargo.toml").exists());
        assert!(project.join("src/lib.rs").exists());
        assert!(project.join("manifest.toml").exists());
        assert!(project.join("tests/integration.rs").exists());

        let cargo = fs::read_to_string(project.join("Cargo.toml")).unwrap();
        assert!(cargo.contains("crate-type = [\"cdylib\"]"));
        assert!(cargo.contains(name));

        let lib = fs::read_to_string(project.join("src/lib.rs")).unwrap();
        assert!(lib.contains("#[capability("));
        assert!(lib.contains(name));
    }
}
