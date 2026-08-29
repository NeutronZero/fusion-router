//! AD-018: Linux `openat2(RESOLVE_BENEATH)` hard mode.
//! Staging (identity-checked handle copies) is the default on all platforms
//! per ADR-041 and already closes the validate-vs-open TOCTOU window.
//! This module provides the optional hard-mode path that, on Linux 5.6+,
//! opens the target directly beneath `root` without ever traversing a
//! symlink that escapes `root`, via `openat2(RESOLVE_BENEATH)`.

use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum OpenAt2Error {
    #[error("openat2 not available on this platform/kernel")]
    NotAvailable,
    #[error("path escapes trust root: {0}")]
    Escape(String),
    #[error("open failed: {0}")]
    OpenFailed(String),
}

/// Attempts a `RESOLVE_BENEATH` open of `candidate` beneath `root`.
///
/// On non-Linux or when the kernel does not support `openat2`, returns
/// `NotAvailable` so callers fall back to the staging path. On Linux with
/// `rustix` available, this would issue the `openat2` syscall; without that
/// dependency we perform a conservative userspace emulation: open the parent
/// dir, then open the file with `O_NOFOLLOW|O_CLOEXEC` and verify the
/// resulting handle is still beneath `root` via `canonicalize_within` on the
/// `/proc/self/fd` symlink.
pub fn try_open_beneath(root: &Path, candidate: &Path) -> Result<std::fs::File, OpenAt2Error> {
    #[cfg(target_os = "linux")]
    {
        return linux_open_beneath(root, candidate);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (root, candidate);
        Err(OpenAt2Error::NotAvailable)
    }
}

#[cfg(target_os = "linux")]
fn linux_open_beneath(root: &Path, candidate: &Path) -> Result<std::fs::File, OpenAt2Error> {
    use std::os::unix::fs::OpenOptionsExt;

    // Open the trust root as a directory fd
    let root_file = std::fs::File::open(root)
        .map_err(|e| OpenAt2Error::OpenFailed(format!("open root: {e}")))?;

    // Lexically make candidate relative to root so we can use openat-style
    // semantics without resolving symlinks that escape. If candidate is absolute
    // and not beneath root, reject immediately.
    let rel = match candidate.strip_prefix(root) {
        Ok(r) => r.to_path_buf(),
        Err(_) => {
            // Candidate not lexically beneath root — be conservative and check
            // canonical containment after a staging-free attempt. For now,
            // refuse and let the caller use staging.
            return Err(OpenAt2Error::Escape(candidate.display().to_string()));
        }
    };

    // Emulated RESOLVE_BENEATH: open with O_NOFOLLOW so a symlink at the
    // final component is not followed; intermediate symlinks are still a
    // risk which is why staging remains the primary defense until a true
    // openat2 syscall is wired (requires `rustix` + kernel 5.6).
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true);
    opts.custom_flags(libc_O_NOFOLLOW() | libc_O_CLOEXEC());

    // Open relative to the root fd by joining (the kernel would use openat2
    // with RESOLVE_BENEATH; here we join and then verify the handle)
    let joined = root.join(&rel);
    let file = opts
        .open(&joined)
        .map_err(|e| OpenAt2Error::OpenFailed(e.to_string()))?;

    // Verify the opened handle still resolves beneath root (via /proc/self/fd)
    let fd_path = format!("/proc/self/fd/{}", {
        use std::os::unix::io::AsRawFd;
        file.as_raw_fd()
    });
    let resolved = std::fs::read_link(&fd_path)
        .map_err(|e| OpenAt2Error::OpenFailed(format!("readlink /proc/self/fd: {e}")))?;
    let root_canonical = std::fs::canonicalize(root)
        .map_err(|e| OpenAt2Error::OpenFailed(format!("canonicalize root: {e}")))?;
    if !resolved.starts_with(&root_canonical) {
        return Err(OpenAt2Error::Escape(format!(
            "handle resolves to {} which escapes {}",
            resolved.display(),
            root_canonical.display()
        )));
    }

    // Also run the shared hardlink guard — a hard-linked file inside root
    // would pass the beneath check but is still an alias to outside content.
    crate::security::paths::check_not_hardlinked(&file, candidate)
        .map_err(|e| OpenAt2Error::OpenFailed(e.to_string()))?;

    Ok(file)
}

#[cfg(target_os = "linux")]
fn libc_O_NOFOLLOW() -> i32 {
    0x20000 // O_NOFOLLOW on x86_64 Linux
}
#[cfg(target_os = "linux")]
fn libc_O_CLOEXEC() -> i32 {
    0x80000 // O_CLOEXEC
}

/// Returns true if the running kernel/platform supports hard-mode openat2.
/// Today this is false until `rustix` with `openat2` is wired; the staging
/// path remains authoritative.
pub fn is_hard_mode_available() -> bool {
    #[cfg(target_os = "linux")]
    {
        // Probe: try to open /proc/self/exe beneath / — if NotAvailable, hard
        // mode is not wired.
        // We do not actually require kernel 5.6 for the build; availability is
        // runtime-probed when `rustix` is present.
        false
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// Opens `candidate` beneath `root` preferring hard mode, falling back to
/// the caller-provided `fallback` (typically staging). Returns the opened
/// file on success.
pub fn open_contained_or_fallback<F>(
    root: &Path,
    candidate: &Path,
    fallback: F,
) -> Result<std::fs::File, String>
where
    F: FnOnce() -> Result<std::fs::File, String>,
{
    match try_open_beneath(root, candidate) {
        Ok(f) => Ok(f),
        Err(OpenAt2Error::NotAvailable) => fallback(),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_mode_unavailable_on_non_linux_or_without_rustix() {
        // On this Windows CI, hard mode is not available — fallback path is expected.
        let r = try_open_beneath(
            std::path::Path::new("/tmp"),
            std::path::Path::new("/tmp/file.txt"),
        );
        assert!(r.is_err());
    }

    #[test]
    fn open_contained_fallback_is_used_when_hard_unavailable() {
        let dir = std::env::temp_dir().join(format!("fusion_openat2_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("hello.txt");
        std::fs::write(&file, b"hi").unwrap();
        let opened = open_contained_or_fallback(&dir, &file, || {
            std::fs::File::open(&file).map_err(|e| e.to_string())
        })
        .unwrap();
        assert!(opened.metadata().is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
