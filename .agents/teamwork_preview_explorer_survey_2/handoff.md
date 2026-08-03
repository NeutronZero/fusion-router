# Handoff Report: Requirement R2 & R3 Investigation

## 1. Observation
- **Requirement R2 (Shell Command Hardening)**:
  - File `src/tools/shell_tool.rs` implements `ShellCommandTool`. Lines 51-56 validate `cmd` against `self.allowed_commands`.
  - File `config/default.yaml` line 101 and `src/config/mod.rs` line 233 include `"cmd"` in `allowed_shell_commands`.
  - File `tests/security.rs` line 63-89 contains `test_shell_injection`, which relies on `allowed = vec!["cmd".to_string(), "echo".to_string()]` and tests running `"cmd"` with `args: ["/c", ...]`.
  - Under Windows, `"cmd"` executes `cmd.exe /c <args>`, permitting execution of arbitrary commands.
- **Requirement R3 (Rate Limiter Guard)**:
  - File `src/middleware/rate_limit.rs` lines 40-61 implements `RateLimiter::start_cleanup`. Line 46 sets `interval = Duration::from_secs(self.config.cleanup_interval_secs)`.
  - Line 49 calls `sleep(interval).await`. If `cleanup_interval_secs` is `0`, `sleep(Duration::from_secs(0))` returns immediately.
  - The `loop` on line 48 repeatedly spawns blocking tasks without yielding, causing a 100% CPU busy-spin loop.

## 2. Logic Chain
- For R2:
  - `ShellCommandTool` executes commands via `tokio::process::Command::new(cmd).args(&cmd_args)`.
  - `allowed_commands` check only verifies `cmd` string equality.
  - If `cmd` is `"cmd"`, `Command::new("cmd").args(["/c", "arbitrary_command"])` invokes Windows Command Prompt to interpret arbitrary command strings.
  - Therefore, allowing shell interpreters like `cmd`, `sh`, `bash`, `powershell` in `allowed_commands` bypasses all command restriction checks.
- For R3:
  - `tokio::time::sleep(Duration::ZERO).await` in Tokio completes instantly.
  - `RateLimiter::start_cleanup` executes `loop { sleep(interval).await; tokio::task::spawn_blocking(...).await; }`.
  - With `cleanup_interval_secs = 0`, Tokio does not delay the loop iteration, causing infinite CPU consumption and continuous thread pool task spawning.

## 3. Caveats
- `ShellCommandTool` is designed to run whitelisted commands. Removing `"cmd"` and restricting shell binary invocation is required, but platform-specific tests (e.g. `echo` on Windows vs Linux) must be adjusted accordingly.
- `AppConfig::validate()` already checks `cleanup_interval_secs == 0` for config files, but `RateLimiter` internal logic must defensively clamp `interval` to a non-zero floor (e.g. `.max(1)`) so that programmatic runtime instantiation cannot trigger CPU spinning.

## 4. Conclusion
- R2 can be fixed by removing `"cmd"` from default allowed shell commands in `src/config/mod.rs` and `config/default.yaml`, hardening `ShellCommandTool` to reject shell interpreter binaries (`cmd`, `sh`, `bash`, `powershell`, `pwsh`, `zsh`), and updating security test cases.
- R3 can be fixed by updating `RateLimiter::start_cleanup` in `src/middleware/rate_limit.rs` to enforce a minimum interval floor (e.g. `Duration::from_secs(self.config.cleanup_interval_secs.max(1))`).

## 5. Verification Method
- **Commands**:
  - `cargo check --all-targets`
  - `cargo test --test security`
  - `cargo test --all-features`
  - `cargo clippy --all-targets --all-features -- -D warnings`
- **Invalidation Conditions**:
  - If `ShellCommandTool` allows `cmd /c ...` execution.
  - If `RateLimiter::start_cleanup` given `cleanup_interval_secs = 0` spikes CPU usage to 100%.
