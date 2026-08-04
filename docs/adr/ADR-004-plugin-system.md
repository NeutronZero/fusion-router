# ADR-004: Plugin Ecosystem & Sandbox

## Status
Accepted (AF-003 Frozen)

## Context
FusionRouter requires extensibility for custom providers, routing strategies, tools, and telemetry without modifying core compiler logic or introducing security risks.

## Decision
Define `Plugin SDK v1` in `fusion-plugin-sdk` supporting WASM (via WASI/wasmtime) and native dynamically loaded modules behind strict manifest declarations (`plugin_manifest.yaml`).

## Alternatives Considered
- In-tree core modifications for each provider: Rejected to enforce Law 15 (Plugin Boundaries).
- Python/JS scripting runtime: Rejected due to memory overhead and execution latency constraints.

## Consequences
- Plugins declare capabilities, permissions, and manifest specifications.
- Protects `fusion-compiler` from provider-specific logic leakage.
