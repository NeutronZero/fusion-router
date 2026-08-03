## 2024-05-18 - [Path Traversal via Absolute Path joining in Rust's PathBuf]
**Vulnerability:** Path traversal in `FilesystemArchiveBackend`'s `store`, `load`, and `exists` methods using absolute paths or `..`.
**Learning:** Rust's `PathBuf::join` replaces the current path completely if the appended path is absolute (e.g. starts with `/` or `C:\`). This means `dir.join(user_input)` is highly vulnerable if `user_input` isn't properly sanitized, as a malicious user could provide an absolute path like `/etc/passwd` to escape `dir`.
**Prevention:** Sanitize inputs before passing them to `PathBuf::join`. In our fix, we explicitly rejected `assessment_id` containing `/`, `\`, or `..`.
