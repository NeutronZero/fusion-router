# Dispatch Assignment: Milestone 2 (Shell Hardening & Rate Limiter Guard)

## Mission
Harden `ShellCommandTool` to reject shell interpreter binaries, clean default config whitelists, update security test cases, and enforce non-zero cleanup interval floor in `RateLimiter`.

## Target Files
- `src/tools/shell_tool.rs`
- `src/config/mod.rs`
- `config/default.yaml`
- `src/middleware/rate_limit.rs`
- `tests/security.rs`

## Detailed Instructions
1. Read `c:\Projects\fusion-router\.agents\ORIGINAL_REQUEST.md` and survey report `c:\Projects\fusion-router\.agents\teamwork_preview_explorer_survey_2\handoff.md`.
2. In `src/tools/shell_tool.rs`:
   - In `validate_command`, reject shell interpreter binaries (`cmd`, `cmd.exe`, `sh`, `bash`, `powershell`, `powershell.exe`, `pwsh`, `zsh`) regardless of `allowed_commands` content.
3. In `src/config/mod.rs` and `config/default.yaml`:
   - Remove `"cmd"` from default allowed shell commands.
4. In `tests/security.rs`:
   - Update security test cases to verify rejection of shell interpreter binaries (such as `"cmd"`).
5. In `src/middleware/rate_limit.rs`:
   - Update `RateLimiter::start_cleanup` to enforce a non-zero minimum interval floor: `Duration::from_secs(self.config.cleanup_interval_secs.max(1))`.
   - Add a unit test verifying that when `cleanup_interval_secs = 0`, the cleanup interval floor defaults to non-zero (at least 1 second) and does not CPU busy-spin.
6. Run `cargo check --all-targets` and `cargo test --all-features`.
7. Write `handoff.md` in your working directory `c:\Projects\fusion-router\.agents\teamwork_preview_worker_m2_gen3` detailing changes, test commands, and output.

## MANDATORY INTEGRITY WARNING
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.
