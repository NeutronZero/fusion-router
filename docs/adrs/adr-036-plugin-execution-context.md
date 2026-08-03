# ADR-036: Plugin Execution Context

- **Status:** Draft
- **Date:** 2026-08-03
- **Applies to:** plugin ABI (`crates/fusion-plugin-api`), runtime sandbox (`src/runtime`, `src/wasm`, `src/plugin`), package system (`src/package`, `src/release`)
- **Charter:** `docs/implementation/security-hardening-v0.13.1.md` Phase 4 (Plugin Trust Boundary), Runtime Law 8
- **Amends:** ADR-019 (Capability Host Interface), ADR-022 (Plugin ABI)

## Context

Host services (`WasmtimeCapabilityHost::fetch_secret`, `http_request`) enforce permissions by scanning the registry for *any* contract holding the needed permission and granting that permission to *whichever plugin is calling* — a subject-confused permission check (audit C4). Additionally, WASM guest execution is un-metered in the plugin strategy path (H7/H12), attestation signatures do not bind package content (H8), and plugin/package paths allow traversal (H9/H10). The missing concept across all of these is **plugin identity**.

## Decision

1. **`PluginExecutionContext` is carried with every execution:** `{ capability_id, package_hash: [u8;32], permissions, fuel_budget, memory_limit_bytes }` lives in `Store` data and is passed to every host function call.
2. **Caller-bound permissions:** host functions check `caller.permissions` only. Registry scans for permission lookup are removed from host services; `CapabilityExecutor` re-checks the instance's contract before dispatch (defense in depth).
3. **URL-structural matching:** permission globs match on parsed scheme/host/port (IDNA-normalized); `*` is permitted in path components only; bare `*` requires explicit operator approval.
4. **Metered, timed, bounded guest execution:** fuel consumption on, per-call fuel budget, `ResourceLimiter` caps (memory + tables), wall-clock timeout per guest call, bounded `read_string`.
5. **Content-bound attestation:** signed attestations carry digests of `manifest.toml` and `module.wasm`; verification fails on digest mismatch. `manifest.permissions` is authoritative over embedded contract permissions.
6. **Law 10 path containment:** all externally supplied paths are canonicalized and proven inside their trust root (`src/security/paths.rs`); `CapabilityId` is validated at construction.

## Consequences

- A zero-permission plugin cannot exfiltrate secrets or egress, even when other contracts hold broad permissions.
- Guest code cannot hang or exhaust the host (fuel + timeout + memory caps).
- Repackaged packages with a stolen valid signature are rejected.
- Plugin identity becomes the single trust primitive for permissions, metering, and attestation; ADR-019/022 host-function semantics are amended accordingly.
