# Comprehensive Analysis: Requirement R2 (Shell Command Hardening) & R3 (Rate Limiter Guard)

## Executive Summary
This document presents the detailed architectural and security investigation for Requirements **R2 (Shell Command Hardening)** and **R3 (Rate Limiter Guard)** in `fusion-router`.

- **R2 Finding**: `ShellCommandTool` relies on an allowed list of executable names (`allowed_commands`). However, the default configuration includes `"cmd"`, and the tool does not restrict shell interpreter binaries (`cmd`, `sh`, `bash`, `powershell`). When `"cmd"` is permitted, users can pass `command: "cmd"` and `args: ["/c", "<arbitrary command>"]`, which executes arbitrary shell commands via Windows `cmd.exe` command line execution, bypassing all allowed-command policy controls.
- **R3 Finding**: `RateLimiter::start_cleanup` constructs `interval = Duration::from_secs(self.config.cleanup_interval_secs)`. If `cleanup_interval_secs` is `0`, `tokio::time::sleep(Duration::from_secs(0)).await` returns instantly in a tight `loop`, causing a 100% CPU busy-spin loop spawning blocking tasks continuously.

---

## Part 1: Requirement R2 — Shell Command Hardening

### 1. Codebase Locations & Architecture
- **Tool Implementation**: `src/tools/shell_tool.rs` (`ShellCommandTool`)
- **Configuration Definition**: `src/config/mod.rs` (`ToolsConfig`, `default_allowed_shell_commands`)
- **Default Config File**: `config/default.yaml` (`tools.allowed_shell_commands`)
- **Existing Security Tests**: `tests/security.rs` (`test_shell_injection`)

### 2. Root Cause Analysis & Vulnerability Details

#### Code Inspection (`src/tools/shell_tool.rs`)
```rust
pub struct ShellCommandTool {
    allowed_commands: Vec<String>,
    timeout_secs: u64,
}

impl Tool for ShellCommandTool {
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
        .await...
    }
}
```

#### Vulnerability Mechanisms
1. **Inclusion of Shell Interpreters (`cmd`) in Allowed List**:
   - `config/default.yaml` (line 101) and `src/config/mod.rs` (`default_allowed_shell_commands()`) include `"cmd"` in the allowed list: `vec!["ls", "echo", "cat", "cmd"]`.
   - When `"cmd"` (or `cmd.exe`) is allowed, an attacker can pass:
     ```json
     {
       "command": "cmd",
       "args": ["/c", "whoami & dir & calc.exe"]
     }
     ```
   - The validation check `self.allowed_commands.iter().any(|a| a == cmd)` checks if `"cmd" == "cmd"`, which evaluates to `true`.
   - `Command::new("cmd").args(["/c", "..."])` executes `cmd.exe /c ...`. `cmd.exe` parses the argument string and executes arbitrary commands, completely defeating the purpose of command whitelist enforcement.

2. **Lack of Shell Binary Blacklisting / Hardening in `ShellCommandTool`**:
   - `ShellCommandTool` does not check if the binary being invoked is a shell interpreter (`cmd`, `cmd.exe`, `sh`, `bash`, `powershell`, `powershell.exe`, `pwsh`, `zsh`).
   - `ShellCommandTool` does not validate path components in `cmd` (e.g. relative or absolute paths, or shell arguments).

3. **Flawed Unit and Integration Tests**:
   - `tests/security.rs` lines 63 & 78-79 explicitly configure `allowed = vec!["cmd".to_string(), "echo".to_string()]` on Windows and run `("cmd", vec!["/c", "echo", "hello"])`.
   - `src/tools/shell_tool.rs` line 101 also uses `"cmd"` in `test_shell_tool_allowed_command`.

### 3. Recommended Fix Strategy for R2
1. **Remove `"cmd"` from Default Allowed Commands**:
   - In `src/config/mod.rs`, update `default_allowed_shell_commands()` to return `vec!["ls".into(), "echo".into(), "cat".into()]` (removing `"cmd"`).
   - In `config/default.yaml`, remove `cmd` from `tools.allowed_shell_commands`.

2. **Hardening `ShellCommandTool` Logic**:
   - Add a strict prohibition against invoking shell interpreters (`cmd`, `cmd.exe`, `sh`, `bash`, `powershell`, `pwsh`, `zsh`) even if explicitly added to configuration.
   - Sanitize `command`: Ensure `cmd` does not contain path separators (`/`, `\`), shell metacharacters, or path traversal elements.

3. **Update Test Suites**:
   - Update `tests/security.rs` and `src/tools/shell_tool.rs` unit tests to test allowed execution using safe binaries (e.g. `echo` or platform-appropriate non-shell binaries) instead of `cmd /c`.
   - Add security tests ensuring shell interpreter commands like `cmd` or `cmd /c` are explicitly rejected.

---

## Part 2: Requirement R3 — Rate Limiter Guard

### 1. Codebase Locations & Architecture
- **Rate Limiter Implementation**: `src/middleware/rate_limit.rs` (`RateLimiter`, `start_cleanup`)
- **Configuration Definition**: `src/config/mod.rs` (`RateLimitingConfig`, `default_cleanup_interval`)
- **Default Config File**: `config/default.yaml` (`rate_limiting.cleanup_interval_secs`)

### 2. Root Cause Analysis & CPU Busy-Spin Details

#### Code Inspection (`src/middleware/rate_limit.rs`)
```rust
pub fn start_cleanup(&self) {
    if self.cleanup_started.compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed).is_err() {
        return;
    }

    let buckets = self.buckets.clone();
    let interval = Duration::from_secs(self.config.cleanup_interval_secs);
    tokio::spawn(async move {
        loop {
            sleep(interval).await;
            let buckets = buckets.clone();
            if let Err(e) = tokio::task::spawn_blocking(move || {
                let cutoff = Instant::now() - Duration::from_secs(interval.as_secs() * 2);
                buckets.retain(|_, b| b.last_access > cutoff);
            })
            .await
            {
                tracing::warn!(error = %e, "Rate limiter cleanup panicked, restarting");
            }
        }
    });
}
```

#### Why CPU Busy-Spins Occur
1. **Zero-Duration Sleep (`Duration::from_secs(0)`)**:
   - If `self.config.cleanup_interval_secs` is `0` (configured via YAML, env, or struct initialization `RateLimitingConfig { cleanup_interval_secs: 0, .. }`), `interval` becomes `Duration::from_secs(0)`.
   - In Tokio, `sleep(Duration::from_secs(0)).await` completes immediately without yielding execution for any measurable time.
   - The loop `loop { sleep(0).await; spawn_blocking(...).await; }` executes continuously at high speed.

2. **Resource Exhaustion**:
   - Every iteration calls `tokio::task::spawn_blocking`, offloading DashMap iteration to the blocking thread pool.
   - This consumes 100% CPU on an async runtime worker thread, leading to high CPU usage, lock contention on `DashMap`, and potential thread starvation.

3. **Inadequate Runtime Guards**:
   - Although `AppConfig::validate()` in `src/config/mod.rs` checks `if self.rate_limiting.cleanup_interval_secs == 0`, `RateLimiter::start_cleanup()` does not enforce a lower bound locally. Programmatic callers or unvalidated config instances bypass this check.

### 3. Recommended Fix Strategy for R3
1. **Enforce Non-Zero Minimum Interval Floor in `start_cleanup`**:
   - In `src/middleware/rate_limit.rs`, enforce a non-zero minimum floor for `interval` (e.g., minimum 1 second or 5 seconds):
     ```rust
     let interval_secs = self.config.cleanup_interval_secs.max(1);
     let interval = Duration::from_secs(interval_secs);
     ```
2. **Harden Constructor & Config Validation**:
   - Ensure `RateLimiter::new` or `start_cleanup` logs a warning if `cleanup_interval_secs == 0` and clamps it to a safe minimum value (e.g. `1` or `10` seconds).
3. **Add Unit Test for Zero Cleanup Interval**:
   - Add a unit test in `src/middleware/rate_limit.rs` that initializes `RateLimiter` with `cleanup_interval_secs = 0`, calls `start_cleanup()`, and verifies that it does not cause CPU busy-spinning or panic.

---

## Verification Plan & Impact Assessment
- **Build Verification**: `cargo check --all-targets`
- **Clippy Verification**: `cargo clippy --all-targets --all-features -- -D warnings`
- **Test Verification**: `cargo test --all-features`
- **Security Specific Verification**: `cargo test --test security`
