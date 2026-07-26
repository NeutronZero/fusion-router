# FusionRouter Architecture Debt Register

> **Purpose**: Tracks explicit architectural trade-offs, deferred capabilities, and planned structural refinements. This prevents temporary implementation choices from accumulating into silent technical debt.

---

## Active Architecture Debt Matrix

| ID | Area | Trade-off / Deferred Scope | Impact | Planned Resolution Target | Status |
|---|---|---|---|---|---|
| **AD-001** | Plugin Loading | In-process Rust C-ABI (`libloading`), WASM (`wasmtime`), and static plugins supported in v0.10. Out-of-process gRPC/IPC plugins deferred. | Third-party plugins in non-WASM languages require compiling against Rust ABI. | v0.11.0 | Planned |
| **AD-002** | WASM Permissions | Coarse-grained WASM sandbox fuel and memory limits in v0.10. Fine-grained capability-based syscall permissions deferred. | WASM plugins operate with coarse sandbox envelopes. | v0.11.0 | Planned |
| **AD-003** | Connector Resolver | Late binding binds single active connector per capability. Connector load balancing and dynamic failover deferred. | Single active connector binding per execution instance. | Future (v0.11.0 / v1.0.0) | Under Review |
| **AD-004** | Capability Cache | In-memory single-node LRU `CapabilityPlannerCache` for `RequirementSet` → `ResolvedCapabilitySet`. Distributed cache deferred. | Cache instances are local to each engine node. | Future (v1.0.0) | Under Review |

---

## Governance Rules for Debt

1. Every deliberate architectural trade-off made during an implementation phase **must** be logged with an `AD-xxx` ID.
2. Architecture Debt items cannot be closed without an empirical benchmark or PR demonstrating resolution.
