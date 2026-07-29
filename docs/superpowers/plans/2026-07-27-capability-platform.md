# v0.12 — Capability Platform & Developer Ecosystem Implementation Plan

> **Goal:** Implement the v0.12 Capability Platform (`fusion-capability-sdk`, `CapabilityRegistry`, typed `Permission` model, `CapabilityResolver`, `CapabilityGraph`, `SandboxRuntime` abstraction, WASI Sandboxing, `.fusionpkg` package format with ADR-018 & ADR-019 ABIs, CLI DX commands, and Operations Console projections).

---

## Technical Architecture & Invariants (ADR-018, ADR-019 & ADR-017)

- **ADR-018 Capability Binary Interface (ABI):** `.fusionpkg` gzipped tarball archives containing `manifest.toml`, `module.wasm`, and `attestation.json` (signed `AttestationEnvelope` verified via M3 certification gates `PLG-1`, `CON-1`).
- **ADR-019 Capability Host Interface:** `CapabilityHostServices` trait exposing policy-controlled runtime services (secrets, HTTP, event publication, logging, metrics) to sandboxed capabilities.
- **`SandboxRuntime` Trait Abstraction:** Abstract `SandboxRuntime` trait enabling `WasmtimeSandboxRuntime` and future container backends.
- **Event-Native Capability Lifecycle:** Capability actions emit append-only `ExecutionEvent` variants (`CapabilityLoaded`, `CapabilityInvoked`, `CapabilityCompleted`, `CapabilityFailed`) onto the ADR-017 Event Stream.
- **Separation of Discovery & Resolution:** `CapabilityRegistry` discovers available capabilities; `CapabilityResolver` expands semver requirements and policy rules into topological `CapabilityGraph` DAGs lowered to compiler `ExecutionGraph`.

---

## File Structure

| File | Responsibility |
|------|---------------|
| `crates/fusion-capability-sdk/src/lib.rs` | Ergonomic SDK & `#[capability]` macro attribute definitions |
| `src/capability/permission.rs` | Typed `Permission` enum policies (`Network`, `Filesystem`, `Secrets`, `Environment`, `Http`) |
| `src/capability/registry.rs` | `CapabilityDescriptor`, `CapabilityRegistry` trait, `InMemoryCapabilityRegistry` |
| `src/capability/resolver.rs` | `CapabilityResolver` engine & `CapabilityGraph` resolution (Sprint O2.5) |
| `src/capability/host.rs` | `CapabilityHostServices` trait & policy-gated host service implementations (Sprint O3.5 / ADR-019) |
| `src/capability/sandbox.rs` | `SandboxRuntime` trait & `WasmtimeSandboxRuntime` WASI execution engine (Sprint O3) |
| `src/capability/package.rs` | `.fusionpkg` archive unpacker, manifest validation, and ADR-018 attestation verification |
| `src/capability/mod.rs` | Capability subsystem re-exports |
| `src/bin/fusion.rs` | CLI commands (`fusion new`, `fusion pack`, `fusion verify`, `fusion dev`, `fusion console`) |
| `tests/capability_platform_tests.rs` | Integration & architectural regression test suite covering SDK, resolver, WASI sandbox, and `.fusionpkg` validation |

---

## Task Breakdown & Checklists

### Task 1: Developer Experience SDK (`crates/fusion-capability-sdk`) (Sprint O1)

**Files:**
- Create: `crates/fusion-capability-sdk/Cargo.toml`
- Create: `crates/fusion-capability-sdk/src/lib.rs`

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

### Task 4: `SandboxRuntime` Abstraction & WASI Engine (Sprint O3)

**Files:**
- Create: `src/capability/sandbox.rs`
- Modify: `src/capability/mod.rs`

- [ ] **Step 1: Implement `SandboxRuntime` trait and `WasmtimeSandboxRuntime` in `src/capability/sandbox.rs`**
- [ ] **Step 2: Implement 64MB memory guards and WASI resource bounds**
- [ ] **Step 3: Add unit tests for sandboxed execution**

Run: `cargo test capability::sandbox`

---

### Task 5: Capability Host Interface (Sprint O3.5 / ADR-019)

**Files:**
- Create: `src/capability/host.rs`
- Modify: `src/capability/mod.rs`

- [ ] **Step 1: Implement `CapabilityHostServices` trait in `src/capability/host.rs`**
- [ ] **Step 2: Connect policy-gated secrets, HTTP, logging, and metrics services**
- [ ] **Step 3: Integrate event emission (`CapabilityInvoked`, `CapabilityCompleted`, `CapabilityFailed`) on ADR-017 Event Stream**

Run: `cargo test capability::host`

---

### Task 6: Signed `.fusionpkg` Archives & ADR-018 Verification (Sprint O4)

**Files:**
- Create: `src/capability/package.rs`
- Modify: `src/capability/mod.rs`

- [ ] **Step 1: Implement `.fusionpkg` package reader and tar.gz unpacker in `src/capability/package.rs`**
- [ ] **Step 2: Implement `attestation.json` signature verification against M3 certification gates**
- [ ] **Step 3: Add unit tests for package verification and tamper rejection**

Run: `cargo test capability::package`

---

### Task 7: CLI Tooling & Operations Console Projection (Sprints O5 & O6)

**Files:**
- Modify: `src/bin/fusion.rs`
- Create: `tests/capability_platform_tests.rs`

- [ ] **Step 1: Add `fusion new`, `fusion pack`, `fusion verify`, `fusion dev`, `fusion console` subcommands**
- [ ] **Step 2: Create architectural regression test suite in `tests/capability_platform_tests.rs`**
- [ ] **Step 3: Run workspace quality checks**

Run:
1. `cargo test --lib capability`
2. `cargo test --test capability_platform_tests`
3. `cargo test --bin fusion`
4. `cargo clippy --all-targets -- -D warnings`

---

## Verification Plan

### Automated Tests & Regression Suite
- `cargo test -p fusion-capability-sdk`
- `cargo test capability::registry`
- `cargo test capability::resolver`
- `cargo test capability::sandbox`
- `cargo test capability::host`
- `cargo test capability::package`
- `cargo test --test capability_platform_tests`
- `cargo test --bin fusion`
- `cargo clippy --all-targets -- -D warnings`

### CLI Command Execution
Command: `cargo run --bin fusion -- capability list`
Command: `cargo run --bin fusion -- capability inspect <CAPABILITY_ID>`
Command: `cargo run --bin fusion -- capability verify <PACKAGE_PATH>`
Expected output: Scans, inspects, and verifies capability packages against ADR-018/ADR-019 contracts and signed attestations.
