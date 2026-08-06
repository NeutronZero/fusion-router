//! Law 10: canonicalized path containment.
//!
//! All path validation in the system (file tools, plugin extraction, shell
//! argument policy) must canonicalize within its trust root before trusting
//! a candidate path. Naive `starts_with` on uncanonicalized paths is
//! bypassable with `..`, absolute-path splicing, and symlinks.

use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error("trust root does not exist: {0}")]
    RootMissing(String),
    #[error("candidate path does not exist or is inaccessible: {0}")]
    CandidateMissing(String),
    #[error("path escapes trust root: {0}")]
    Escape(String),
}

/// Canonicalizes `candidate` and verifies the result lies within the
/// canonicalized `root`. Returns the canonical candidate on success.
pub fn canonicalize_within(root: &Path, candidate: &Path) -> Result<PathBuf, PathError> {
    let root_canonical = std::fs::canonicalize(root)
        .map_err(|_| PathError::RootMissing(root.display().to_string()))?;
    let candidate_canonical = std::fs::canonicalize(candidate)
        .map_err(|_| PathError::CandidateMissing(candidate.display().to_string()))?;
    if !candidate_canonical.starts_with(&root_canonical) {
        return Err(PathError::Escape(candidate.display().to_string()));
    }
    Ok(candidate_canonical)
}

/// True when `candidate` canonicalizes inside `root` (see `canonicalize_within`).
// Kept as the boolean convenience wrapper over `canonicalize_within`; used by
// tests today and by future policy callers that don't need the error detail.
#[allow(dead_code)]
pub fn is_path_within(root: &Path, candidate: &Path) -> bool {
    canonicalize_within(root, candidate).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!("_fusion_paths_{}", uuid::Uuid::new_v4()))
    }

    fn write(dir: &Path, rel: &str, content: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let p = dir.join(rel);
        std::fs::write(&p, content).unwrap();
        p
    }

    #[test]
    fn test_in_root_path_ok() {
        let root = temp_dir();
        let file = write(&root, "a.txt", "x");
        let canonical = canonicalize_within(&root, &file).unwrap();
        assert!(canonical.starts_with(std::fs::canonicalize(&root).unwrap()));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_parent_traversal_rejected() {
        let root = temp_dir();
        write(&root, "a.txt", "x");
        // ../escapes from within the root
        let escape = root.join("..").join("escape.txt");
        let err = canonicalize_within(&root, &escape).unwrap_err();
        assert!(matches!(err, PathError::CandidateMissing(_) | PathError::Escape(_)));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_absolute_path_splice_rejected() {
        let root = temp_dir();
        std::fs::create_dir_all(&root).unwrap();
        let outside = write(root.parent().unwrap(), "outside.txt", "x");
        let err = canonicalize_within(&root, &outside).unwrap_err();
        assert!(matches!(err, PathError::Escape(_)));
        let _ = std::fs::remove_file(&outside);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn test_symlink_escape_rejected() {
        use std::os::unix::fs::symlink;
        let root = temp_dir();
        std::fs::create_dir_all(&root).unwrap();
        let outside = write(root.parent().unwrap(), "secret.txt", "s");
        let link = root.join("link.txt");
        symlink(&outside, &link).unwrap();
        let err = canonicalize_within(&root, &link).unwrap_err();
        assert!(matches!(err, PathError::Escape(_)));
        let _ = std::fs::remove_file(&outside);
        let _ = std::fs::remove_dir_all(&root);
    }
}
