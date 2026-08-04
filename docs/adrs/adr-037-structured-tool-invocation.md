# ADR-037: Structured Tool Invocation

- **Status:** Approved (implemented, v0.13.1)
- **Date:** 2026-08-03 (approved 2026-08-04)
- **Applies to:** executor (`src/executor`), providers (`src/providers`), tools (`src/tools`), config (`src/config`)
- **Charter:** `docs/implementation/security-hardening-v0.13.1.md` Phase 3, Runtime Law 7

## Context

The executor parses the model's raw output text as JSON and, if it contains `{"tool": ..., "args": ...}`, executes the tool unconditionally (`src/executor/mod.rs:242-270`). This makes free-form model output executable actions, turning prompt injection into a direct command channel (audit H2): fetched web content, uploaded text, or a hostile system prompt can steer the model into emitting a tool call for `shell_command`/`cat` (C3) or arbitrary HTTP (H1). The architecture distinguishes `Model Output` from `Provider Tool Call`; the implementation conflates them.

## Decision

1. **Model output is data, not commands.** Free-form JSON tool parsing is removed from the executor; it is never used to invoke tools.
2. **Tools execute only via provider-native `tool_calls`:** structured tool-call results bound to the model response by the provider transport. Providers without native tool-call support execute no tools (fail closed, no emulation).
3. **Per-request tool allowlist:** tool execution is scoped to an explicit allowlist per request/session (`node.config["tool_allowlist"]`); `tools.allow_auto_exec` must be enabled for any automatic execution.
4. **Migration flag:** `executor.allow_model_json_tools` (default `false`) exists for one release cycle to surface behavior changes, then is removed.

## Consequences

- The prompt-injection → tool-execution chain is severed at the trust boundary: model text can never become an action.
- Strategy tests and tools tests in `tests/strategy_sdk/*` are updated to the structured-call contract.
- Providers (`openrouter`, `zen`, `ollama`) gain a typed `native_tool_calls` field; response plumbing changes are additive.

## Implementation (v0.13.1)

- `ChatCompletionResponse.native_tool_calls: Option<Vec<ToolCall>>` where
  `ToolCall { id, name, arguments: Value }` (types); `native_tool_calls_from`
  normalizes OpenAI (`choices[0].message.tool_calls`) and Ollama
  (`message.tool_calls`) wire shapes.
- `DefaultExecutor.allow_auto_exec` (config `tools.allow_auto_exec`, default
  false) gates execution; per-request `tool_allowlist` must be non-empty;
  `request_tool_definitions` advertises `tools` to the provider only under
  the same conditions.
- Non-allowlisted / disabled calls are surfaced as text
  (`{"tool_calls": [{..., "executed": false, "reason": ...}]}`).
- Tests: `law7_no_freeform_tool_parsing` (executor + end-to-end),
  `law7_native_tool_calls_*`, provider normalization tests.
