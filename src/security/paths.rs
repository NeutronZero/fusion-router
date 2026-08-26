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
    #[error("path is hard-linked from outside the trust root ({nlink} links): {path}")]
    Hardlink { path: String, nlink: u64 },
    #[error("hard-link count could not be determined for {0}; refusing (fail closed)")]
    LinkCountUnavailable(String),
}

/// Canonicalizes `candidate` and verifies the result lies within the
/// canonicalized `root`. Returns the canonical candidate on success.
///
/// NOTE (hardlinks): `fs::canonicalize` resolves symlinks but cannot detect a
/// hard link planted INSIDE the root pointing at content created elsewhere —
/// both names are equally canonical. Callers that stage or copy file contents
/// must additionally reject files whose link count exceeds one; see
/// [`link_count`] and its use in shell-tool staging.
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

/// Async wrapper that offloads the blocking `canonicalize` syscalls to
/// `spawn_blocking` so Tokio workers are not stalled under high file-tool
/// load. Prefer this from async handlers; the sync `canonicalize_within`
/// remains for sync contexts and tests.
pub async fn canonicalize_within_async(
    root: PathBuf,
    candidate: PathBuf,
) -> Result<PathBuf, PathError> {
    tokio::task::spawn_blocking(move || canonicalize_within(&root, &candidate))
        .await
        .map_err(|e| PathError::CandidateMissing(format!("spawn_blocking join failed: {e}")))?
}

/// Number of directory entries (hard links) referencing the same file as
/// `meta`, when the platform exposes it through stable APIs.
///
/// - Unix: `nlink` from the inode metadata (`MetadataExt`).
/// - Windows: stable `std` does not expose NTFS link counts
///   (`MetadataExt::number_of_links` is behind unstable
///   `windows_by_handle`); use [`handle_link_count`] on an opened file
///   instead.
#[allow(unused_variables)]
pub fn link_count(meta: &std::fs::Metadata) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(meta.nlink())
    }
    #[cfg(not(unix))]
    {
        None
    }
}

#[cfg(windows)]
#[repr(C)]
struct ByHandleFileInformation {
    file_attributes: u32,
    creation_time_lo: u32,
    creation_time_hi: u32,
    last_access_lo: u32,
    last_access_hi: u32,
    last_write_lo: u32,
    last_write_hi: u32,
    volume_serial_number: u32,
    file_size_high: u32,
    file_size_low: u32,
    number_of_links: u32,
    file_index_high: u32,
    file_index_low: u32,
}

#[cfg(windows)]
extern "system" {
    fn GetFileInformationByHandle(handle: isize, info: *mut ByHandleFileInformation) -> i32;
}

/// Number of hard links to the file behind an already-opened handle.
///
/// - Unix: `nlink` from the handle metadata (`MetadataExt`).
/// - Windows: `GetFileInformationByHandle(...).nNumberOfLinks` via kernel32
///   (stable `std` does not expose it; see [`link_count`]).
///
/// Using the opened handle (rather than a fresh stat) keeps the count tied to
/// the exact file whose bytes are about to be read.
pub fn handle_link_count(file: &std::fs::File) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        file.metadata().ok().map(|m| m.nlink())
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        let mut info = ByHandleFileInformation {
            file_attributes: 0,
            creation_time_lo: 0,
            creation_time_hi: 0,
            last_access_lo: 0,
            last_access_hi: 0,
            last_write_lo: 0,
            last_write_hi: 0,
            volume_serial_number: 0,
            file_size_high: 0,
            file_size_low: 0,
            number_of_links: 0,
            file_index_high: 0,
            file_index_low: 0,
        };
        let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as isize, &mut info) };
        if ok != 0 {
            Some(info.number_of_links as u64)
        } else {
            None
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = file;
        None
    }
}

/// Rejects `path` when it carries more than one hard link (see
/// [`canonicalize_within`]'s hardlink note). Opens the file and checks the
/// handle's link count so the verdict applies to the exact inode read later.
pub fn ensure_not_hardlinked(path: &Path) -> Result<(), PathError> {
    let file = std::fs::File::open(path)
        .map_err(|_| PathError::CandidateMissing(path.display().to_string()))?;
    let meta = file
        .metadata()
        .map_err(|_| PathError::CandidateMissing(path.display().to_string()))?;
    if !meta.is_file() {
        return Ok(());
    }
    check_not_hardlinked(&file, path)
}

/// Shared rejection policy over an opened handle's link count.
///
/// When the platform cannot report a link count (e.g. the Win32 query
/// fails), the check FAILS CLOSED instead of assuming single-linked:
/// an undetectable hard link is exactly the alias this guard exists to
/// catch (review M9).
pub(crate) fn check_not_hardlinked(file: &std::fs::File, display: &Path) -> Result<(), PathError> {
    match handle_link_count(file) {
        Some(nlink) if nlink > 1 => Err(PathError::Hardlink {
            path: display.display().to_string(),
            nlink,
        }),
        Some(_) => Ok(()),
        None => Err(PathError::LinkCountUnavailable(display.display().to_string())),
    }
}

/// Stable file identity from an opened handle: `(volume serial, file index)`
/// on Windows; `(dev, ino)` on Unix. Returns `None` when the platform query
/// fails. Used to strengthen validate-vs-open TOCTOU comparisons on Windows,
/// where timestamp+size identity is collidable (review M9).
#[allow(unused_variables)]
pub fn handle_file_id(file: &std::fs::File) -> Option<(u32, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = file.metadata().ok()?;
        Some((meta.dev(), meta.ino()))
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        let mut info = ByHandleFileInformation {
            file_attributes: 0,
            creation_time_lo: 0,
            creation_time_hi: 0,
            last_access_lo: 0,
            last_access_hi: 0,
            last_write_lo: 0,
            last_write_hi: 0,
            volume_serial_number: 0,
            file_size_high: 0,
            file_size_low: 0,
            number_of_links: 0,
            file_index_high: 0,
            file_index_low: 0,
        };
        let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as isize, &mut info) };
        if ok == 0 {
            return None;
        }
        Some((
            info.volume_serial_number,
            ((info.file_index_high as u64) << 32) | info.file_index_low as u64,
        ))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = file;
        None
    }
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
        assert!(matches!(
            err,
            PathError::CandidateMissing(_) | PathError::Escape(_)
        ));
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

    #[test]
    fn test_hardlinked_file_rejected_by_link_count() {
        // `hard_link` is stable on unix and windows: two names, one inode.
        let root = temp_dir();
        std::fs::create_dir_all(&root).unwrap();
        let file = write(&root, "input.txt", "data");
        let alias = root.join("alias.txt");
        std::fs::hard_link(&file, &alias).expect("hard link creation must work on this FS");

        for path in [&file, &alias] {
            let err = ensure_not_hardlinked(path).unwrap_err();
            match err {
                PathError::Hardlink { nlink, .. } => assert!(nlink >= 2),
                other => panic!("expected Hardlink, got {other:?}"),
            }
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_single_linked_file_passes_hardlink_check() {
        let root = temp_dir();
        let file = write(&root, "solo.txt", "only-name");
        assert!(ensure_not_hardlinked(&file).is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_missing_file_fails_closed_in_hardlink_check() {
        let ghost = temp_dir().join("does-not-exist.txt");
        assert!(matches!(
            ensure_not_hardlinked(&ghost),
            Err(PathError::CandidateMissing(_))
        ));
    }
}
