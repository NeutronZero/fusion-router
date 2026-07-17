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

        if !self.allowed_commands.iter().any(|a| a == cmd) {
            return Err(format!(
                "Command '{}' is not in allowed list: {:?}",
                cmd, self.allowed_commands
            ));
        }

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
    async fn test_shell_tool_allowed_command() {
        let tool = ShellCommandTool::new(
            vec!["cmd".to_string(), "echo".to_string()],
            5,
        );
        // Use cmd.exe /c echo hello world on Windows
        let result = tool.execute(serde_json::json!({
            "command": "cmd",
            "args": ["/c", "echo", "hello world"]
        })).await;
        assert!(result.is_ok());
        let val = result.unwrap();
        assert!(val["stdout"].as_str().unwrap_or("").contains("hello"));
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
