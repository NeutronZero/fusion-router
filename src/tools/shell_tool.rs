use async_trait::async_trait;
use serde_json::Value;
use tokio::process::Command;

use super::Tool;
use crate::security::paths::canonicalize_within;

const MAX_ARGS: usize = 32;
const MAX_ARG_LEN: usize = 1024;
const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
const STAGING_ROOT_NAME: &str = "fusion-shell-stage";
const STALE_STAGE_SECS: u64 = 3600;

/// Commands whose arguments are treated as file paths by convention. For
/// these, every non-flag argument must canonicalize inside an allowed read
/// directory (Law 10) unless `allow_unrestricted_args` is set.
const FILE_READING_COMMANDS: &[&str] = &[
    "cat", "head", "tail", "grep", "wc", "sort", "uniq", "cut", "sed", "awk", "less", "more",
    "fold", "nl", "tac", "strings", "diff", "file",
];

/// 0-based positions of the positional (non-flag) arguments that are command
/// text (patterns, scripts, programs) rather than file paths. Validating
/// these as canonical paths would wrongly reject legitimate invocations like
/// `grep "foo" file` or `awk '{print $1}' file`.
fn command_non_path_positions(cmd: &str) -> &'static [usize] {
    match cmd {
        "grep" => &[0],
        "sed" => &[0],
        "awk" => &[0],
        _ => &[],
    }
}

/// Flags whose value is a file path for the file-reading command set.
const PATH_FLAGS: &[&str] = &["-f", "--file", "--include", "--exclude", "--label", "-o"];

/// If `arg` is a path-taking flag, returns how its value is supplied:
/// `Some(Some(value))` for inline (`--file=x`), `Some(None)` when the value is
/// the next argument (`-f x`), `None` when the argument is not a path flag.
fn path_flag_value(arg: &str) -> Option<Option<String>> {
    if let Some(eq) = arg.find('=') {
        let (flag, value) = arg.split_at(eq);
        let value = &value[1..];
        if PATH_FLAGS.contains(&flag) {
            return Some(Some(value.to_string()));
        }
        return None;
    }
    if PATH_FLAGS.contains(&arg) {
        return Some(None);
    }
    None
}

/// One path-bearing argument slot: its argv index, the value that must resolve
/// inside an allowed root, and Ã¢â‚¬â€ for inline flag values Ã¢â‚¬â€ the `--flag=` prefix
/// to reattach when the value is rewritten.
struct PathSlot {
    index: usize,
    value: String,
    inline_prefix: Option<String>,
}

fn plan_path_slots(cmd: &str, args: &[String]) -> Vec<PathSlot> {
    let non_path_positions = command_non_path_positions(cmd);
    let mut slots: Vec<PathSlot> = Vec::new();
    let mut positional_index = 0usize;
    let mut skip_next = false;
    for (i, arg) in args.iter().enumerate() {
        if skip_next {
            skip_next = false;
            slots.push(PathSlot {
                index: i,
                value: arg.clone(),
                inline_prefix: None,
            });
            continue;
        }
        if let Some(value) = path_flag_value(arg) {
            match value {
                Some(v) => slots.push(PathSlot {
                    index: i,
                    value: v,
                    inline_prefix: Some(arg[..arg.find('=').unwrap() + 1].to_string()),
                }),
                None => skip_next = true,
            }
            continue;
        }
        if arg.starts_with('-') {
            continue;
        }
        if non_path_positions.contains(&positional_index) {
            positional_index += 1;
            continue;
        }
        positional_index += 1;
        slots.push(PathSlot {
            index: i,
            value: arg.clone(),
            inline_prefix: None,
        });
    }
    slots
}

/// Host-controlled per-invocation directory holding validated snapshots of
/// every path argument (ADR-041). Dropped at the end of `execute`, so staged
/// copies never outlive the child.
#[derive(Debug)]
pub struct StagingSession {
    dir: Option<std::path::PathBuf>,
}

impl StagingSession {
    fn create() -> std::io::Result<Self> {
        let dir = std::env::temp_dir()
            .join(STAGING_ROOT_NAME)
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir: Some(dir) })
    }

    /// Copies `canonical` (an already-validated canonical path) into the
    /// staging directory and returns the staged absolute path.
    ///
    /// The open happens FIRST; the handle is then identity-checked against a
    /// fresh stat of `canonical` so any swap between validation and open is
    /// detected instead of read. Content always comes from the opened handle.
    fn stage_canonical_file(
        &mut self,
        canonical: &std::path::Path,
        max_bytes: usize,
    ) -> Result<String, String> {
        use std::io::Read;
        let dir = self.dir.as_ref().ok_or("staging session already closed")?;

        let mut handle = std::fs::File::open(canonical).map_err(|e| format!("open failed: {e}"))?;
        let handle_meta = handle
            .metadata()
            .map_err(|e| format!("handle metadata failed: {e}"))?;
        if !handle_meta.is_file() {
            return Err("validated path is not a regular file".into());
        }
        if handle_meta.len() as usize > max_bytes {
            return Err(format!(
                "file is {} bytes, exceeding max_staged_input_bytes {max_bytes}",
                handle_meta.len()
            ));
        }
        let path_meta = std::fs::metadata(canonical)
            .map_err(|e| format!("re-stat of validated path failed: {e}"))?;
        if !same_file_identity(&handle_meta, &path_meta) {
            return Err(
                "path changed between validation and open (TOCTOU guard); refusing".to_string(),
            );
        }

        let ext = canonical
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{e}"))
            .unwrap_or_default();
        let dest = dir.join(format!("{}{}", uuid::Uuid::new_v4(), ext));

        let mut out = std::fs::File::create(&dest).map_err(|e| format!("staged create: {e}"))?;
        let mut buf = [0u8; 8192];
        let mut total = 0usize;
        loop {
            let n = handle
                .read(&mut buf)
                .map_err(|e| format!("read failed: {e}"))?;
            if n == 0 {
                break;
            }
            total += n;
            if total > max_bytes {
                drop(out);
                let _ = std::fs::remove_file(&dest);
                return Err(format!(
                    "file grew past max_staged_input_bytes {max_bytes} during copy"
                ));
            }
            use std::io::Write;
            out.write_all(&buf[..n])
                .map_err(|e| format!("staged write: {e}"))?;
        }

        Ok(dest.to_string_lossy().into_owned())
    }
}

impl Drop for StagingSession {
    fn drop(&mut self) {
        if let Some(dir) = self.dir.take() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

#[cfg(unix)]
fn same_file_identity(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    a.dev() == b.dev() && a.ino() == b.ino()
}

#[cfg(windows)]
fn same_file_identity(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    a.creation_time() == b.creation_time()
        && a.len() == b.len()
        && a.last_write_time() == b.last_write_time()
}

#[cfg(not(any(unix, windows)))]
fn same_file_identity(_a: &std::fs::Metadata, _b: &std::fs::Metadata) -> bool {
    true
}

/// Removes staging directories left behind by crashed processes (ADR-041).
pub fn sweep_stale_staging_dirs() {
    let root = std::env::temp_dir().join(STAGING_ROOT_NAME);
    let Ok(entries) = std::fs::read_dir(&root) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        let age = meta
            .modified()
            .ok()
            .and_then(|m| m.elapsed().ok())
            .unwrap_or_default();
        if age.as_secs() > STALE_STAGE_SECS {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Shell path-argument policy (ADR-041).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellPathMode {
    /// Validate, then copy the validated file into a host-controlled staging
    /// directory and rewrite argv to the staged copy. Closes the
    /// validate-vs-open TOCTOU window by construction.
    Stage,
    /// Pass the original path after validation (legacy behavior).
    Direct,
}

impl ShellPathMode {
    pub fn from_config(value: &str) -> Self {
        match value {
            "direct" => Self::Direct,
            _ => Self::Stage,
        }
    }
}

pub struct ShellCommandTool {
    allowed_commands: Vec<String>,
    timeout_secs: u64,
    allowed_read_directories: Vec<String>,
    allow_unrestricted_args: bool,
    path_mode: ShellPathMode,
    max_staged_input_bytes: usize,
}

impl ShellCommandTool {
    pub fn new(
        allowed_commands: Vec<String>,
        timeout_secs: u64,
        allowed_read_directories: Vec<String>,
        allow_unrestricted_args: bool,
    ) -> Self {
        sweep_stale_staging_dirs();
        Self {
            allowed_commands,
            timeout_secs,
            allowed_read_directories,
            allow_unrestricted_args,
            path_mode: ShellPathMode::Stage,
            max_staged_input_bytes: crate::config::default_max_staged_input_bytes(),
        }
    }

    pub fn with_path_policy(mut self, mode: ShellPathMode, max_staged_input_bytes: usize) -> Self {
        self.path_mode = mode;
        self.max_staged_input_bytes = max_staged_input_bytes.max(1);
        self
    }

    fn validate_args(args: &[String]) -> Result<(), String> {
        if args.len() > MAX_ARGS {
            return Err(format!(
                "too many arguments: {} (max {})",
                args.len(),
                MAX_ARGS
            ));
        }
        for arg in args {
            if arg.len() > MAX_ARG_LEN {
                return Err(format!(
                    "argument exceeds max length of {MAX_ARG_LEN} bytes"
                ));
            }
            if arg.contains('\0') || arg.contains('\n') || arg.contains('\r') {
                return Err("arguments must not contain NUL or newline characters".to_string());
            }
        }
        Ok(())
    }

    /// Per-command argument policy (WP 3.2 / finding C3, ADR-041): for known
    /// file-reading commands, every non-flag argument is treated as a file
    /// path and must canonicalize inside an allowed read directory. Values of
    /// known path-taking flags (`-f x`, `--file=x`) are covered too.
    ///
    /// In `Stage` mode the validated files are additionally copied into a
    /// host-controlled staging directory and argv is rewritten to the staged
    /// copies, so the child never opens an attacker-swappable path.
    fn apply_path_policy(
        &self,
        cmd: &str,
        args: &[String],
    ) -> Result<(Vec<String>, Option<StagingSession>), String> {
        if self.allow_unrestricted_args {
            return Ok((args.to_vec(), None));
        }
        if !FILE_READING_COMMANDS.contains(&cmd) {
            return Ok((args.to_vec(), None));
        }
        if self.allowed_read_directories.is_empty() {
            return Err(format!(
                "command '{}' reads files but no allowed_read_directories are configured",
                cmd
            ));
        }

        let slots = plan_path_slots(cmd, args);
        match self.path_mode {
            ShellPathMode::Direct => {
                for slot in &slots {
                    self.resolve_within(cmd, &slot.value)?;
                }
                Ok((args.to_vec(), None))
            }
            ShellPathMode::Stage => {
                let mut session = StagingSession::create()
                    .map_err(|e| format!("staging directory creation failed: {e}"))?;
                let mut out = args.to_vec();
                for slot in &slots {
                    let canonical = self.resolve_within(cmd, &slot.value)?;
                    let staged = session
                        .stage_canonical_file(&canonical, self.max_staged_input_bytes)
                        .map_err(|e| {
                            format!(
                                "staging failed for '{}' on command '{}': {e}",
                                slot.value, cmd
                            )
                        })?;
                    out[slot.index] = match &slot.inline_prefix {
                        Some(prefix) => format!("{prefix}{staged}"),
                        None => staged,
                    };
                }
                Ok((out, Some(session)))
            }
        }
    }

    /// Compat wrapper used by the unit tests: validate only, no rewriting.
    #[cfg(test)]
    fn validate_path_args(&self, cmd: &str, args: &[String]) -> Result<(), String> {
        self.apply_path_policy(cmd, args).map(|_| ())
    }

    fn resolve_within(&self, cmd: &str, candidate: &str) -> Result<std::path::PathBuf, String> {
        let canonical = self
            .allowed_read_directories
            .iter()
            .find_map(|dir| {
                canonicalize_within(std::path::Path::new(dir), std::path::Path::new(candidate)).ok()
            })
            .ok_or_else(|| {
                format!(
                    "argument '{}' for command '{}' is outside allowed read directories {:?}",
                    candidate, cmd, self.allowed_read_directories
                )
            })?;
        let meta = std::fs::metadata(&canonical)
            .map_err(|e| format!("validated path '{}' unreadable: {e}", canonical.display()))?;
        if !meta.is_file() {
            return Err(format!(
                "argument '{}' for command '{}' is not a regular file",
                candidate, cmd
            ));
        }
        Ok(canonical)
    }

    pub fn validate_command(&self, cmd: &str) -> Result<(), String> {
        let cmd_clean = cmd.trim().to_lowercase();
        let file_name = std::path::Path::new(&cmd_clean)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&cmd_clean);
        let file_stem = std::path::Path::new(&cmd_clean)
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or(&cmd_clean);

        const REJECTED_SHELLS: &[&str] = &[
            "cmd",
            "cmd.exe",
            "sh",
            "bash",
            "powershell",
            "powershell.exe",
            "pwsh",
            "zsh",
        ];

        if REJECTED_SHELLS.contains(&cmd_clean.as_str())
            || REJECTED_SHELLS.contains(&file_name)
            || REJECTED_SHELLS.contains(&file_stem)
        {
            return Err(format!(
                "Execution of shell interpreter binary '{}' is strictly prohibited",
                cmd
            ));
        }

        if !self.allowed_commands.iter().any(|a| a == cmd) {
            return Err(format!(
                "Command '{}' is not in allowed list: {:?}",
                cmd, self.allowed_commands
            ));
        }

        Ok(())
    }
}

#[async_trait]
impl Tool for ShellCommandTool {
    fn name(&self) -> &str {
        "shell_command"
    }

    fn description(&self) -> &str {
        "Executes a shell command from an allowed list. Only pre-configured commands are permitted."
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Command to execute (must be in allowed list)"
                },
                "args": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Command arguments"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, String> {
        let cmd = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing 'command' argument".to_string())?;

        self.validate_command(cmd)?;

        let cmd_args: Vec<String> = args
            .get("args")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        Self::validate_args(&cmd_args)?;
        let (cmd_args, _staging) = self.apply_path_policy(cmd, &cmd_args)?;

        let mut child = Command::new(cmd)
            .args(&cmd_args)
            // Never inherit the server's environment: the child would
            // otherwise carry API keys (OPENROUTER_API_KEY, ...) it could
            // echo back in its output.
            .env_clear()
            // Kill the OS process when the `Child` is dropped, so a timeout
            // does not leave an orphaned process running.
            .kill_on_drop(true)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("Command execution error: {}", e))?;

        let stdout_pipe = child.stdout.take();
        let stderr_pipe = child.stderr.take();

        // Read each stream up to MAX_OUTPUT_BYTES + 1 so a chatty child can
        // never buffer unbounded memory in the host before truncation.
        let stdout_task = tokio::spawn(read_capped(stdout_pipe));
        let stderr_task = tokio::spawn(read_capped(stderr_pipe));

        let status = match tokio::time::timeout(
            std::time::Duration::from_secs(self.timeout_secs),
            child.wait(),
        )
        .await
        {
            Ok(Ok(status)) => status,
            Ok(Err(e)) => return Err(format!("Command wait error: {}", e)),
            // The wait future was dropped, dropping `child`; `kill_on_drop`
            // reaps the spawned process.
            Err(_) => {
                stdout_task.abort();
                stderr_task.abort();
                return Err(format!(
                    "Command '{}' timed out after {}s (process killed)",
                    cmd, self.timeout_secs
                ));
            }
        };

        let (stdout_bytes, stdout_truncated) = stdout_task
            .await
            .map_err(|e| format!("stdout reader failed: {e}"))?
            .map_err(|e| format!("stdout read failed: {e}"))?;
        let (stderr_bytes, stderr_truncated) = stderr_task
            .await
            .map_err(|e| format!("stderr reader failed: {e}"))?
            .map_err(|e| format!("stderr read failed: {e}"))?;

        let stdout = String::from_utf8_lossy(&stdout_bytes).to_string();
        let stderr = String::from_utf8_lossy(&stderr_bytes).to_string();
        let stdout = maybe_terminate(&stdout);
        let stderr = maybe_terminate(&stderr);

        Ok(serde_json::json!({
            "stdout": stdout,
            "stderr": stderr,
            "stdout_truncated": stdout_truncated,
            "stderr_truncated": stderr_truncated,
            "exit_code": status.code().unwrap_or(-1),
        }))
    }
}

/// Reads a stream to completion, retaining at most `MAX_OUTPUT_BYTES` bytes;
/// returns the retained bytes and whether content beyond the cap was dropped.
async fn read_capped<R>(pipe: Option<R>) -> std::io::Result<(Vec<u8>, bool)>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;
    let mut pipe = match pipe {
        Some(p) => p,
        None => return Ok((Vec::new(), false)),
    };
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 8192];
    let mut truncated = false;
    loop {
        let n = pipe.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        if buf.len() >= MAX_OUTPUT_BYTES {
            truncated = true;
            continue;
        }
        let take = (MAX_OUTPUT_BYTES - buf.len()).min(n);
        buf.extend_from_slice(&chunk[..take]);
        if take < n {
            truncated = true;
        }
    }
    Ok((buf, truncated))
}

fn maybe_terminate(text: &str) -> String {
    if text.len() <= MAX_OUTPUT_BYTES {
        return text.to_string();
    }
    let mut truncated = text[..MAX_OUTPUT_BYTES].to_string();
    truncated.push_str("\n... [output truncated]");
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(commands: Vec<&str>, timeout: u64) -> ShellCommandTool {
        ShellCommandTool::new(
            commands.into_iter().map(String::from).collect(),
            timeout,
            vec![".".into()],
            false,
        )
    }

    fn tool_with_dirs(
        commands: Vec<&str>,
        dirs: Vec<&str>,
        unrestricted: bool,
    ) -> ShellCommandTool {
        ShellCommandTool::new(
            commands.into_iter().map(String::from).collect(),
            5,
            dirs.into_iter().map(String::from).collect(),
            unrestricted,
        )
    }

    #[tokio::test]
    async fn test_shell_tool_blocked_command() {
        let tool = tool(vec!["ls", "echo"], 5);
        let result = tool
            .execute(serde_json::json!({
                "command": "rm -rf /"
            }))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not in allowed list"));
    }

    #[tokio::test]
    async fn test_shell_tool_rejected_shell_interpreter_binaries() {
        let tool = tool(
            vec![
                "cmd",
                "sh",
                "bash",
                "powershell",
                "powershell.exe",
                "pwsh",
                "zsh",
            ],
            5,
        );

        for bin in &[
            "cmd",
            "cmd.exe",
            "sh",
            "bash",
            "powershell",
            "powershell.exe",
            "pwsh",
            "zsh",
        ] {
            let res = tool.validate_command(bin);
            assert!(res.is_err(), "binary '{}' should be rejected", bin);
            assert!(res.unwrap_err().contains("strictly prohibited"));
        }
    }

    #[tokio::test]
    async fn test_shell_tool_allowed_command() {
        let tool = tool(vec!["echo"], 5);
        #[cfg(not(windows))]
        let (cmd, args): (&str, Vec<&str>) = ("echo", vec!["hello world"]);
        #[cfg(windows)]
        let (cmd, _args): (&str, Vec<&str>) = ("echo", vec!["hello world"]);

        assert!(tool.validate_command(cmd).is_ok());

        // Even if OS cannot find echo binary directly without shell on Windows, validation must pass.
        // On non-windows, command execution completes successfully.
        #[cfg(not(windows))]
        {
            let result = tool
                .execute(serde_json::json!({
                    "command": cmd,
                    "args": args
                }))
                .await;
            assert!(result.is_ok());
            let val = result.unwrap();
            assert!(val["stdout"].as_str().unwrap_or("").contains("hello"));
        }
    }

    #[tokio::test]
    async fn test_shell_tool_missing_args() {
        let tool = tool(vec!["echo"], 5);
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_args_rejects_nul_bytes() {
        let result = ShellCommandTool::validate_args(&["payload\0--delete".to_string()]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("NUL"));
    }

    #[test]
    fn test_validate_args_rejects_too_many() {
        let args: Vec<String> = (0..(MAX_ARGS + 1)).map(|i| format!("arg{i}")).collect();
        let result = ShellCommandTool::validate_args(&args);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("too many arguments"));
    }

    #[test]
    fn test_validate_args_rejects_oversized() {
        let result = ShellCommandTool::validate_args(&["x".repeat(MAX_ARG_LEN + 1)]);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_args_accepts_normal_args() {
        assert!(ShellCommandTool::validate_args(&["hello".into(), "world".into()]).is_ok());
    }

    #[test]
    fn test_cat_parent_traversal_rejected() {
        let tool = tool_with_dirs(vec!["cat"], vec!["."], false);
        let result = tool.validate_path_args("cat", &["../secret".to_string()]);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("outside allowed read directories"));
    }

    #[test]
    fn test_cat_absolute_escape_rejected() {
        let tool = tool_with_dirs(vec!["cat"], vec!["."], false);
        let result = tool.validate_path_args("cat", &["/etc/passwd".to_string()]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cat_missing_path_rejected() {
        let tool = tool_with_dirs(vec!["cat"], vec!["."], false);
        let result = tool.validate_path_args("cat", &["no_such_file_anywhere".to_string()]);
        assert!(
            result.is_err(),
            "non-canonicalizable paths must be rejected"
        );
    }

    #[test]
    fn test_cat_path_within_allowed_dir_ok() {
        let tmp = std::env::temp_dir();
        let unique = format!("_fusion_cat_ok_{}.txt", uuid::Uuid::new_v4());
        let full = tmp.join(&unique);
        std::fs::write(&full, "x").unwrap();
        let result = tool_with_dirs(vec!["cat"], vec![tmp.to_str().unwrap()], false)
            .validate_path_args("cat", &[full.to_str().unwrap().to_string()]);
        let _ = std::fs::remove_file(&full);
        assert!(result.is_ok());
    }

    #[test]
    fn test_flags_are_not_path_checked() {
        let tmp = std::env::temp_dir();
        let unique = format!("_fusion_cat_n_{}.txt", uuid::Uuid::new_v4());
        let full = tmp.join(&unique);
        std::fs::write(&full, "x").unwrap();
        let result = tool_with_dirs(vec!["cat"], vec![tmp.to_str().unwrap()], false)
            .validate_path_args(
                "cat",
                &["-n".to_string(), full.to_str().unwrap().to_string()],
            );
        let _ = std::fs::remove_file(&full);
        assert!(result.is_ok());
    }

    #[test]
    fn test_non_file_commands_not_path_checked() {
        let tool = tool_with_dirs(vec!["echo"], vec!["."], false);
        assert!(tool
            .validate_path_args("echo", &["../anything".to_string()])
            .is_ok());
    }

    #[test]
    fn test_unrestricted_args_skips_path_policy() {
        let tool = tool_with_dirs(vec!["cat"], vec!["."], true);
        assert!(tool
            .validate_path_args("cat", &["../secret".to_string()])
            .is_ok());
    }

    #[test]
    fn test_stage_mode_rewrites_path_args_to_staged_copies() {
        let root = std::env::temp_dir().join(format!("_fusion_stage_rw_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let data = root.join("input.txt");
        std::fs::write(&data, "staged-content-marker").unwrap();

        let tool = ShellCommandTool::new(
            vec!["cat".into()],
            5,
            vec![root.to_str().unwrap().to_string()],
            false,
        );
        let (rewritten, session) = tool
            .apply_path_policy("cat", &[data.to_str().unwrap().to_string()])
            .expect("staging must succeed");

        assert_ne!(
            rewritten[0],
            data.to_str().unwrap(),
            "argv must be rewritten"
        );
        let staged = std::fs::read_to_string(&rewritten[0]).unwrap();
        assert_eq!(staged, "staged-content-marker");
        drop(session);
        assert!(
            !std::fs::exists(&rewritten[0]).unwrap_or(false),
            "staged copy must be removed when the session drops"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_direct_mode_keeps_original_argv() {
        let root = std::env::temp_dir().join(format!("_fusion_direct_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let data = root.join("input.txt");
        std::fs::write(&data, "x").unwrap();

        let tool = ShellCommandTool::new(
            vec!["cat".into()],
            5,
            vec![root.to_str().unwrap().to_string()],
            false,
        )
        .with_path_policy(ShellPathMode::Direct, 1024);
        let (rewritten, session) = tool
            .apply_path_policy("cat", &[data.to_str().unwrap().to_string()])
            .unwrap();
        assert_eq!(rewritten[0], data.to_str().unwrap());
        assert!(session.is_none(), "direct mode must not stage");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_stage_mode_rejects_oversized_input() {
        let root = std::env::temp_dir().join(format!("_fusion_big_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let data = root.join("big.txt");
        std::fs::write(&data, vec![b'a'; 4096]).unwrap();

        let tool = ShellCommandTool::new(
            vec!["cat".into()],
            5,
            vec![root.to_str().unwrap().to_string()],
            false,
        )
        .with_path_policy(ShellPathMode::Stage, 1024);
        let err = tool
            .apply_path_policy("cat", &[data.to_str().unwrap().to_string()])
            .unwrap_err();
        assert!(err.contains("max_staged_input_bytes"), "{err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_staging_session_removes_directory_on_drop() {
        let session = StagingSession::create().unwrap();
        let dir = session.dir.clone().unwrap();
        assert!(dir.is_dir());
        drop(session);
        assert!(!dir.exists(), "session drop must remove the staging dir");
    }

    #[cfg(unix)]
    #[test]
    fn test_race_symlink_swap_never_reads_outside_root() {
        use std::os::unix::fs::symlink;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let root = std::env::temp_dir().join(format!("_fusion_race_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let good = root.join("good.txt");
        std::fs::write(&good, "GOOD").unwrap();
        let secret_path = root
            .parent()
            .unwrap()
            .join(format!("secret_{}.txt", uuid::Uuid::new_v4()));
        std::fs::write(&secret_path, "SECRET").unwrap();
        let link = root.join("link.txt");
        symlink(&good, &link).unwrap();

        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let secret_for_thread = secret_path.clone();
        let link_for_thread = link.clone();
        let flipper = std::thread::spawn(move || {
            let mut flip = false;
            while !stop2.load(Ordering::Relaxed) {
                let target = if flip {
                    good.as_path()
                } else {
                    secret_for_thread.as_path()
                };
                let _ = std::fs::remove_file(&link_for_thread);
                let _ = symlink(target, &link_for_thread);
                flip = !flip;
            }
        });

        let tool = ShellCommandTool::new(
            vec!["cat".into()],
            5,
            vec![root.to_str().unwrap().to_string()],
            false,
        );
        for _ in 0..200 {
            match tool.apply_path_policy("cat", &[link.to_str().unwrap().to_string()]) {
                Ok((rewritten, session)) => {
                    let content = std::fs::read_to_string(&rewritten[0]).unwrap();
                    assert_eq!(content, "GOOD", "staged copy leaked outside-root bytes");
                    drop(session);
                }
                Err(e) => {
                    // Fail-closed rejections are acceptable; reading SECRET is not.
                    assert!(
                        e.contains("outside allowed")
                            || e.contains("TOCTOU")
                            || e.contains("unreadable"),
                        "{e}"
                    );
                }
            }
        }
        stop.store(true, Ordering::Relaxed);
        flipper.join().unwrap();
        let _ = std::fs::remove_file(&secret_path);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_flag_inline_file_value_outside_dirs_rejected() {
        let tool = tool_with_dirs(vec!["sed"], vec!["."], false);
        let result = tool.validate_path_args("sed", &["--file=/etc/evil.sed".to_string()]);
        assert!(result.is_err(), "--file=/etc/... must be path-checked");
    }

    #[test]
    fn test_flag_next_arg_file_value_outside_dirs_rejected() {
        let tool = tool_with_dirs(vec!["sed"], vec!["."], false);
        let result =
            tool.validate_path_args("sed", &["-f".to_string(), "/etc/evil.sed".to_string()]);
        assert!(result.is_err(), "`sed -f /etc/x` must be path-checked");
    }

    #[test]
    fn test_flag_file_value_within_dirs_ok() {
        let tmp = std::env::temp_dir();
        let unique = format!("_fusion_flag_ok_{}.txt", uuid::Uuid::new_v4());
        let full = tmp.join(&unique);
        std::fs::write(&full, "pattern").unwrap();
        let result = tool_with_dirs(vec!["grep"], vec![tmp.to_str().unwrap()], false)
            .validate_path_args(
                "grep",
                &[
                    "pattern".to_string(),
                    "-f".to_string(),
                    full.to_str().unwrap().to_string(),
                ],
            );
        let _ = std::fs::remove_file(&full);
        assert!(result.is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn test_symlink_escape_rejected() {
        use std::os::unix::fs::symlink;
        let root =
            std::env::temp_dir().join(format!("_fusion_shell_root_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let secret = root
            .parent()
            .unwrap()
            .join(format!("_fusion_shell_secret_{}", uuid::Uuid::new_v4()));
        std::fs::write(&secret, "s").unwrap();
        let link = root.join("link.txt");
        symlink(&secret, &link).unwrap();

        let result = tool_with_dirs(vec!["cat"], vec![root.to_str().unwrap()], false)
            .validate_path_args("cat", &[link.to_str().unwrap().to_string()]);
        let _ = std::fs::remove_file(&secret);
        let _ = std::fs::remove_dir_all(&root);
        assert!(result.is_err(), "symlink escape must be rejected");
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn test_shell_timeout_enforced() {
        let tool = ShellCommandTool::new(vec!["sleep".to_string()], 1, vec![".".into()], false);
        let result = tool
            .execute(serde_json::json!({
                "command": "sleep",
                "args": ["5"]
            }))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("timed out"));
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn test_shell_tool_args_passed_verbatim_without_shell() {
        let tool = tool(vec!["echo"], 5);
        let marker = format!("injected_marker_{}", uuid::Uuid::new_v4());

        let result = tool
            .execute(serde_json::json!({
                "command": "echo",
                "args": [format!("a; touch {marker}")]
            }))
            .await;

        assert!(result.is_ok(), "execution failed: {:?}", result.err());
        let val = result.unwrap();
        let stdout = val["stdout"].as_str().unwrap_or("");
        assert!(
            stdout.contains("a; touch"),
            "args must be passed verbatim without shell interpretation, got: {stdout}"
        );
        assert!(
            !std::path::Path::new(&marker).exists(),
            "semicolon in an argument must not execute a second command"
        );
        let _ = std::fs::remove_file(&marker);
    }
}
