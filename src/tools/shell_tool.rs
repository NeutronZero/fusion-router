use async_trait::async_trait;
use serde_json::Value;
use tokio::process::Command;

use super::Tool;
use crate::security::paths::canonicalize_within;

const MAX_ARGS: usize = 32;
const MAX_ARG_LEN: usize = 1024;

/// Commands whose arguments are treated as file paths by convention. For
/// these, every non-flag argument must canonicalize inside an allowed read
/// directory (Law 10) unless `allow_unrestricted_args` is set.
const FILE_READING_COMMANDS: &[&str] = &[
    "cat", "head", "tail", "grep", "wc", "sort", "uniq", "cut", "sed", "awk",
    "less", "more", "fold", "nl", "tac", "strings", "diff", "file",
];

pub struct ShellCommandTool {
    allowed_commands: Vec<String>,
    timeout_secs: u64,
    allowed_read_directories: Vec<String>,
    allow_unrestricted_args: bool,
}

impl ShellCommandTool {
    pub fn new(
        allowed_commands: Vec<String>,
        timeout_secs: u64,
        allowed_read_directories: Vec<String>,
        allow_unrestricted_args: bool,
    ) -> Self {
        Self {
            allowed_commands,
            timeout_secs,
            allowed_read_directories,
            allow_unrestricted_args,
        }
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
            if arg.contains('\0') {
                return Err("arguments must not contain NUL bytes".to_string());
            }
        }
        Ok(())
    }

    /// Per-command argument policy (WP 3.2 / finding C3): for known
    /// file-reading commands, every non-flag argument is treated as a file
    /// path and must canonicalize inside an allowed read directory — a
    /// `cat ../secret` chain cannot read outside the sandbox.
    fn validate_path_args(&self, cmd: &str, args: &[String]) -> Result<(), String> {
        if self.allow_unrestricted_args {
            return Ok(());
        }
        if !FILE_READING_COMMANDS.contains(&cmd) {
            return Ok(());
        }
        if self.allowed_read_directories.is_empty() {
            return Err(format!(
                "command '{}' reads files but no allowed_read_directories are configured",
                cmd
            ));
        }
        for arg in args {
            if arg.starts_with('-') {
                continue;
            }
            let candidate = std::path::PathBuf::from(arg);
            let within = self
                .allowed_read_directories
                .iter()
                .any(|dir| {
                    canonicalize_within(std::path::Path::new(dir), &candidate).is_ok()
                });
            if !within {
                return Err(format!(
                    "argument '{}' for command '{}' is outside allowed read directories {:?}",
                    arg, cmd, self.allowed_read_directories
                ));
            }
        }
        Ok(())
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
            "cmd", "cmd.exe", "sh", "bash", "powershell", "powershell.exe", "pwsh", "zsh",
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
        let cmd = args.get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing 'command' argument".to_string())?;

        self.validate_command(cmd)?;

        let cmd_args: Vec<String> = args.get("args")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
            })
            .unwrap_or_default();

        Self::validate_args(&cmd_args)?;
        self.validate_path_args(cmd, &cmd_args)?;

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(self.timeout_secs),
            Command::new(cmd).args(&cmd_args).output(),
        )
        .await
        .map_err(|_| format!("Command '{}' timed out after {}s", cmd, self.timeout_secs))?
        .map_err(|e| format!("Command execution error: {}", e))?;

        Ok(serde_json::json!({
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
            "exit_code": output.status.code().unwrap_or(-1),
        }))
    }
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

    fn tool_with_dirs(commands: Vec<&str>, dirs: Vec<&str>, unrestricted: bool) -> ShellCommandTool {
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
        let result = tool.execute(serde_json::json!({
            "command": "rm -rf /"
        })).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not in allowed list"));
    }

    #[tokio::test]
    async fn test_shell_tool_rejected_shell_interpreter_binaries() {
        let tool = tool(
            vec!["cmd", "sh", "bash", "powershell", "powershell.exe", "pwsh", "zsh"],
            5,
        );

        for bin in &["cmd", "cmd.exe", "sh", "bash", "powershell", "powershell.exe", "pwsh", "zsh"] {
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
            let result = tool.execute(serde_json::json!({
                "command": cmd,
                "args": args
            })).await;
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
        assert!(result.unwrap_err().contains("outside allowed read directories"));
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
        assert!(result.is_err(), "non-canonicalizable paths must be rejected");
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
            .validate_path_args("cat", &["-n".to_string(), full.to_str().unwrap().to_string()]);
        let _ = std::fs::remove_file(&full);
        assert!(result.is_ok());
    }

    #[test]
    fn test_non_file_commands_not_path_checked() {
        let tool = tool_with_dirs(vec!["echo"], vec!["."], false);
        assert!(tool.validate_path_args("echo", &["../anything".to_string()]).is_ok());
    }

    #[test]
    fn test_unrestricted_args_skips_path_policy() {
        let tool = tool_with_dirs(vec!["cat"], vec!["."], true);
        assert!(tool.validate_path_args("cat", &["../secret".to_string()]).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn test_symlink_escape_rejected() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!("_fusion_shell_root_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let secret = root.parent().unwrap().join(format!("_fusion_shell_secret_{}", uuid::Uuid::new_v4()));
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
        let tool = ShellCommandTool::new(
            vec!["sleep".to_string()],
            1,
            vec![".".into()],
            false,
        );
        let result = tool.execute(serde_json::json!({
            "command": "sleep",
            "args": ["5"]
        })).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("timed out"));
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn test_shell_tool_args_passed_verbatim_without_shell() {
        let tool = tool(vec!["echo"], 5);
        let marker = format!("injected_marker_{}", uuid::Uuid::new_v4());

        let result = tool.execute(serde_json::json!({
            "command": "echo",
            "args": [format!("a; touch {marker}")]
        })).await;

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
