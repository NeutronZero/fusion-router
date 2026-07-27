# ADR-018: Capability Binary Interface (ABI) & Package Specification

* **Status:** Approved
* **Date:** 2026-07-27
* **Subsystem:** Capability Platform

---

## Context

Prior to v0.12, plugins in FusionRouter were dynamic C-ABI shared libraries (`.so`/`.dll`). To enable safe multi-tenant capability execution, cross-platform portability, sandboxed resource control, and cryptographic release governance, a formal binary package specification and WebAssembly host interface contract is required.

---

## Decision

We introduce **ADR-018: Capability Binary Interface (ABI)** defining the contract between FusionRouter's runtime engine and external capability packages (`.fusionpkg`).

### 1. Package Structure (`.fusionpkg`)

Capabilities ship as gzipped tarball archives (`.fusionpkg`) containing:
- `manifest.toml`: Metadata, capabilities exposed, input/output JSON schemas, and typed `Permission` declarations.
- `module.wasm`: Compiled WASI WebAssembly module executing capability logic.
- `attestation.json`: Cryptographically signed `AttestationEnvelope` verified via M3 ecosystem certification gates (`PLG-1`, `CON-1`).

### 2. Typed Permission Scoping

Capabilities MUST declare all required permissions in `manifest.toml`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Permission {
    Network(NetworkPolicy),
    Filesystem(FilesystemPolicy),
    Secrets(Vec<String>),
    Environment(Vec<String>),
    Http(Vec<UrlPattern>),
}
```

Capabilities requesting ungranted permissions are rejected at `CapabilityResolver` time by `PolicyCompilerPass`.

### 3. WASI Memory & Sandbox Invariants

1. **Memory Isolation:** Modules execute within isolated `wasmtime` linear memory with configurable ceilings (default 64MB).
2. **File/Network Isolation:** Filesystem access is limited to virtual pre-opened directories; outbound HTTP requires explicit `Permission::Http` approval.
3. **Event Stream Lifecycle:** Capability invocation emits `CapabilityLoaded`, `CapabilityInvoked`, `CapabilityCompleted`, or `CapabilityFailed` events onto the ADR-017 Event Stream.

---

## Consequence

- **Pros:** Safe, cross-platform sandboxing, cryptographically attested extensions, fine-grained permission control.
- **Cons:** WebAssembly compilation requirement for capability authors, simplified by `fusion-capability-sdk` procedural macros.
