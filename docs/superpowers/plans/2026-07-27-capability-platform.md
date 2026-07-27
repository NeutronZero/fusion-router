# v0.12 — Capability Platform & Developer Ecosystem Implementation Plan

> **Goal:** Implement the v0.12 Capability Platform (`fusion-capability-sdk`, `CapabilityRegistry`, typed `Permission` model, `CapabilityResolver`, `CapabilityGraph`, WASI Sandboxing, `.fusionpkg` package format with ADR-018 ABI, CLI DX commands, and Operations Console projections).

---

## Technical Architecture & Invariants (ADR-018 & ADR-017)

- **ADR-018 Capability Binary Interface (ABI):** `.fusionpkg` archives contain `manifest.toml`, `module.wasm`, and `attestation.json` (signed `AttestationEnvelope` verified via M3 certification gates `PLG-1`, `CON-1`).
- **Event-Native Capability Lifecycle:** Capability actions emit append-only `ExecutionEvent` variants (`CapabilityLoaded`, `CapabilityInvoked`, `CapabilityCompleted`, `CapabilityFailed`) onto the ADR-017 Event Stream.
- **WASI Memory & Policy Scoping:** Scoped WebAssembly execution (`wasmtime` integration) with 64MB memory limits and explicit typed `Permission` policies (`Network`, `Filesystem`, `Secrets`, `Environment`, `Http`).
- **Separation of Discovery & Resolution:** `CapabilityRegistry` discovers available capabilities; `CapabilityResolver` expands semver requirements and policy rules into topological `CapabilityGraph` DAGs lowered to compiler `ExecutionGraph`.

---

## File Structure

| File | Responsibility |
|------|---------------|
| `crates/fusion-capability-sdk/src/lib.rs` | Ergonomic SDK & `#[capability]` macro attribute definitions |
| `src/capability/permission.rs` | Typed `Permission` enum policies (`Network`, `Filesystem`, `Secrets`, `Environment`, `Http`) |
| `src/capability/registry.rs` | `CapabilityDescriptor`, `CapabilityRegistry` trait, `InMemoryCapabilityRegistry` |
| `src/capability/resolver.rs` | `CapabilityResolver` engine & `CapabilityGraph` resolution (Sprint O2.5) |
| `src/capability/sandbox.rs` | WASI WebAssembly execution engine (`wasmtime` integration & memory guards) |
| `src/capability/package.rs` | `.fusionpkg` archive unpacker, manifest validation, and ADR-018 attestation verification |
| `src/capability/mod.rs` | Capability subsystem re-exports |
| `src/bin/fusion.rs` | CLI commands (`fusion new`, `fusion pack`, `fusion dev`, `fusion console`) |
| `tests/capability_platform_tests.rs` | Integration test suite covering SDK, resolver, WASI sandbox, and `.fusionpkg` validation |

---

## Task Breakdown & Checklists

### Task 1: Developer Experience SDK (`crates/fusion-capability-sdk`) (Sprint O1)

**Files:**
- Create: `crates/fusion-capability-sdk/Cargo.toml`
- Create: `crates/fusion-capability-sdk/src/lib.rs`
- Create: `crates/fusion-capability-sdk/src/macro.rs`

- [ ] **Step 1: Create `fusion-capability-sdk` crate with `Capability` trait and context types**
- [ ] **Step 2: Implement procedural attribute macro `#[capability]` generating manifests & JSON schemas**
- [ ] **Step 3: Verify SDK compilation & unit tests**

Run: `cargo test -p fusion-capability-sdk`

---

### Task 2: Typed Permissions & Capability Registry (Sprint O2)

**Files:**
- Create: `src/capability/permission.rs`
- Create: `src/capability/registry.rs`
- Modify: `src/capability/mod.rs`

- [ ] **Step 1: Implement typed `Permission` policy enum in `src/capability/permission.rs`**
- [ ] **Step 2: Implement `CapabilityDescriptor` and `InMemoryCapabilityRegistry` in `src/capability/registry.rs`**
- [ ] **Step 3: Add unit tests for registry discovery and permission checks**

Run: `cargo test capability::registry`

---

### Task 3: Capability Resolution Engine & Graph (Sprint O2.5)

**Files:**
- Create: `src/capability/resolver.rs`
- Modify: `src/capability/mod.rs`

- [ ] **Step 1: Implement `CapabilityResolver` and `CapabilityGraph` in `src/capability/resolver.rs`**
- [ ] **Step 2: Implement policy evaluation and semver resolution**
- [ ] **Step 3: Add unit tests for graph sorting and dependency expansion**

Run: `cargo test capability::resolver`

---

### Task 4: WASI Sandboxed Execution Runtime (Sprint O3)

**Files:**
- Create: `src/capability/sandbox.rs`
- Modify: `src/capability/mod.rs`

- [ ] **Step 1: Implement WASI sandbox engine in `src/capability/sandbox.rs` (`wasmtime` integration)**
- [ ] **Step 2: Implement 64MB memory guards and permission-scoped host calls**
- [ ] **Step 3: Integrate event emission (`CapabilityInvoked`, `CapabilityCompleted`, `CapabilityFailed`)**
- [ ] **Step 4: Add unit tests for sandboxed execution**

Run: `cargo test capability::sandbox`

---

### Task 5: Signed `.fusionpkg` Archives & ADR-018 Verification (Sprint O4)

**Files:**
- Create: `src/capability/package.rs`
- Modify: `src/capability/mod.rs`

- [ ] **Step 1: Implement `.fusionpkg` package reader and tar.gz unpacker in `src/capability/package.rs`**
- [ ] **Step 2: Implement `attestation.json` signature verification against M3 certification gates**
- [ ] **Step 3: Add unit tests for package verification and tamper rejection**

Run: `cargo test capability::package`

---

### Task 6: CLI Tooling & Operations Console Projection (Sprints O5 & O6)

**Files:**
- Modify: `src/bin/fusion.rs`
- Create: `tests/capability_platform_tests.rs`

- [ ] **Step 1: Add `fusion new`, `fusion pack`, `fusion dev`, and `fusion console` subcommands**
- [ ] **Step 2: Create integration test suite in `tests/capability_platform_tests.rs`**
- [ ] **Step 3: Run workspace quality checks**

Run:
1. `cargo test --lib capability`
2. `cargo test --test capability_platform_tests`
3. `cargo test --bin fusion`
4. `cargo clippy --all-targets -- -D warnings`

---

## Verification Plan

### Automated Tests
- `cargo test -p fusion-capability-sdk`
- `cargo test capability::registry`
- `cargo test capability::resolver`
- `cargo test capability::sandbox`
- `cargo test capability::package`
- `cargo test --test capability_platform_tests`
- `cargo test --bin fusion`
- `cargo clippy --all-targets -- -D warnings`

### CLI Command Execution
Command: `cargo run --bin fusion -- capability list`
Command: `cargo run --bin fusion -- capability inspect <CAPABILITY_ID>`
Expected output: Lists discovered registered capabilities and displays permissions, schemas, and attestation status.
