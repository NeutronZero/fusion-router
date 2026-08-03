use async_trait::async_trait;
use serde_json::Value;
use tokio::process::Command;

use super::Tool;

pub struct ShellCommandTool {
    allowed_commands: Vec<String>,
    timeout_secs: u64,
}

impl ShellCommandTool {
    pub fn new(allowed_commands: Vec<String>, timeout_secs: u64) -> Self {
        Self { allowed_commands, timeout_secs }
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

    #[tokio::test]
    async fn test_shell_tool_blocked_command() {
        let tool = ShellCommandTool::new(
            vec!["ls".to_string(), "echo".to_string()],
            5,
        );
        let result = tool.execute(serde_json::json!({
            "command": "rm -rf /"
        })).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not in allowed list"));
    }

    #[tokio::test]
    async fn test_shell_tool_rejected_shell_interpreter_binaries() {
        let tool = ShellCommandTool::new(
            vec!["cmd".to_string(), "sh".to_string(), "bash".to_string(), "powershell".to_string(), "powershell.exe".to_string(), "pwsh".to_string(), "zsh".to_string()],
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
        let tool = ShellCommandTool::new(
            vec!["echo".to_string()],
            5,
        );
        #[cfg(not(windows))]
        let (cmd, args): (&str, Vec<&str>) = ("echo", vec!["hello world"]);
        #[cfg(windows)]
        let (cmd, args): (&str, Vec<&str>) = ("echo", vec!["hello world"]);

        assert!(tool.validate_command(cmd).is_ok());

        let result = tool.execute(serde_json::json!({
            "command": cmd,
            "args": args
        })).await;

        // Even if OS cannot find echo binary directly without shell on Windows, validation must pass.
        // On non-windows, command execution completes successfully.
        #[cfg(not(windows))]
        {
            assert!(result.is_ok());
            let val = result.unwrap();
            assert!(val["stdout"].as_str().unwrap_or("").contains("hello"));
        }
    }

    #[tokio::test]
    async fn test_shell_tool_missing_args() {
        let tool = ShellCommandTool::new(
            vec!["echo".to_string()],
            5,
        );
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }
}
