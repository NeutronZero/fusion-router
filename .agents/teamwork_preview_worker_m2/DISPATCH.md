## 2026-08-03T10:45:00Z
You are teamwork_preview_worker instance (gen 3) for Milestone 2: Shell Command Hardening (R2) & Rate Limiter Guard (R3).
Working directory: c:\Projects\fusion-router\.agents\teamwork_preview_worker_m2
Original Request Path: c:\Projects\fusion-router\.agents\ORIGINAL_REQUEST.md
Survey Analysis Path: c:\Projects\fusion-router\.agents\teamwork_preview_explorer_survey_2\handoff.md

Your task:
Implement Requirement R2 (Shell Command Hardening) and Requirement R3 (Rate Limiter Guard):
1. Read ORIGINAL_REQUEST.md and the survey handoff report at c:\Projects\fusion-router\.agents\teamwork_preview_explorer_survey_2\handoff.md.
2. Requirement R2 (Shell Command Hardening):
   - Modify `src/tools/shell_tool.rs` to explicitly reject shell interpreter binaries (`cmd`, `cmd.exe`, `sh`, `bash`, `powershell`, `pwsh`, `zsh`) in `ShellCommandTool::validate_command`, regardless of `allowed_commands`.
   - Remove `"cmd"` from default allowed shell commands in `src/config/mod.rs` and `config/default.yaml`.
   - Update tests in `tests/security.rs` to verify that execution of `"cmd"` or other shell binaries is rejected.
3. Requirement R3 (Rate Limiter Guard):
   - Modify `RateLimiter::start_cleanup` in `src/middleware/rate_limit.rs` to clamp `cleanup_interval_secs` to a non-zero minimum floor (`self.config.cleanup_interval_secs.max(1)` or minimum 1 second duration) so that zero interval config cannot cause infinite CPU busy-spinning loops.
   - Add unit test for `RateLimiter` verifying non-zero cleanup interval enforcement under zero input config.
4. Run `cargo check --all-targets` and `cargo test --all-features`. Ensure all tests pass.
5. Create your working directory at `c:\Projects\fusion-router\.agents\teamwork_preview_worker_m2`. Write your implementation report to `c:\Projects\fusion-router\.agents\teamwork_preview_worker_m2\handoff.md` including exact diffs, command lines executed, and build/test outputs.
