# v0.12 — Capability Platform & Developer Ecosystem Specification

> **Goal:** Establish a production-grade Capability Platform (`fusion-capability-sdk`, `CapabilityRegistry`, `CapabilityResolver`, `CapabilityGraph`, WASI Sandboxing, and Signed `.fusionpkg` Packages) that seamlessly integrates third-party capabilities into FusionRouter's compiler pipeline, event-driven runtime (ADR-017), and release governance engine (ADR-018 ABI).

---

## 1. Architectural Constitution & Positioning

FusionRouter is **not** an agent wrapper. It is a **compiler-driven, event-sourced LLM orchestration runtime and capability platform**.

### Core Platform Invariants for v0.12

1. **Agents as Strategies, Not Substrates:** "Agents" exist as high-level reasoning strategies (`Strategy` trait) or capability compositions (`PrimitiveGraph`), preserving the core compiler model (`Intent -> WorkflowIR -> ExecutionGraph -> Event Stream`).
2. **Event-Native Capability Lifecycle:** Capability loading, activation, invocation, and teardown emit append-only `ExecutionEvent` variants (`CapabilityLoaded`, `CapabilityInvoked`, `CapabilityCompleted`, `CapabilityFailed`).
3. **Policy-Governed Capability Execution:** Capability access to network, filesystem, and system resources is strictly scoped by `PolicyCompilerPass`, typed `Permission` policies, and WASI sandboxing.
4. **Cryptographically Attested Capability Packages (ADR-018 ABI):** Capabilities are packaged with signed release attestations (`SignedAttestation`) verified against ecosystem certification gates (`PLG-1`, `CON-1`).

---

## 2. System Architecture

```text
                        FusionRouter Engine
                              │
        ┌─────────────────────┼─────────────────────┐
        ▼                     ▼                     ▼
 Compiler Platform      Event Runtime        Release Governance
        │                     │                     │
        └──────────────┬──────┴──────────────┬──────┘
                       ▼
               v0.12 Capability Platform
                       │
        ┌──────────────┼──────────────┬──────────────┬──────────────┐
        ▼              ▼              ▼              ▼              ▼
Capability SDK    Registry       Resolver       Capability     WASI Sandbox
(fusion-capability)(Discovery)   (O2.5 Engine)  Graph (Sort)   (Isolation)
        │
        ▼
 Signed Capability Packages (.fusionpkg / ADR-018 ABI)
        │
        ▼
 Fusion Operations Console
(Timelines • Traces • Policies • Events • Attestations)
```

---

## 3. Subsystem Components & Contracts

### 3.1 Developer Experience SDK (`crates/fusion-capability-sdk`)

Ergonomic Rust SDK with procedural macro attributes:

```rust
use fusion_capability_sdk::prelude::*;

#[capability(
    name = "web_summarizer",
    version = "1.0.0",
    description = "Summarizes web pages with context trimming",
    category = "TextProcessing"
)]
pub struct WebSummarizer {
    #[config(default = 512)]
    max_tokens: usize,
}

#[async_trait]
impl Capability for WebSummarizer {
    async fn execute(&self, ctx: &CapabilityContext, input: Value) -> Result<Value, CapabilityError> {
        Ok(json!({ "summary": "processed payload" }))
    }
}
```

### 3.2 Typed Permission Model (`src/capability/permission.rs`)

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

### 3.3 Dynamic Capability Registry (`src/capability/registry.rs`)

```rust
pub struct CapabilityDescriptor {
    pub id: String,
    pub name: String,
    pub version: semver::Version,
    pub input_schema: serde_json::Value,
    pub output_schema: serde_json::Value,
    pub permissions: Vec<Permission>,
    pub dependencies: Vec<String>,
}

pub trait CapabilityRegistry: Send + Sync {
    fn register(&self, descriptor: CapabilityDescriptor) -> Result<(), GateError>;
    fn discover(&self) -> Vec<CapabilityDescriptor>;
}
```

### 3.4 Capability Resolution Engine & Graph (`src/capability/resolver.rs` - Sprint O2.5)

```rust
pub struct CapabilityResolutionRequest {
    pub required_capabilities: Vec<String>,
    pub environment: ReleaseEnvironment,
}

pub struct CapabilityResolver {
    registry: Arc<dyn CapabilityRegistry>,
}

impl CapabilityResolver {
    pub fn resolve(&self, req: &CapabilityResolutionRequest) -> Result<CapabilityGraph, GateError> {
        // Query registry -> evaluate policies -> resolve semver versions -> expand DAG dependencies
        todo!()
    }
}
```

### 3.5 WASI Sandboxed Execution Runtime (`src/capability/sandbox.rs`)

Capabilities compile to WebAssembly/WASI modules and run within an isolated `wasmtime` runtime with memory bounds and WASI file/network permissions.

### 3.6 Signed Capability Packages (`.fusionpkg` - ADR-018 ABI)

Capabilities ship as signed bundle archives containing `manifest.toml`, `module.wasm`, and `attestation.json` (signed `AttestationEnvelope`).

---

## 4. Milestone Progression (v0.12 Roadmap)

1. **Sprint O1:** `fusion-capability-sdk` & Procedural Macro Attributes (`#[capability]`).
2. **Sprint O2:** `CapabilityRegistry` & Typed `Permission` Model.
3. **Sprint O2.5:** `CapabilityResolver` Engine & Semantic `CapabilityGraph`.
4. **Sprint O3:** WASI Sandboxed Execution Runtime (`wasmtime` integration).
5. **Sprint O4:** Signed `.fusionpkg` Packages & ADR-018 ABI Verification.
6. **Sprint O5:** CLI Tooling (`fusion new`, `fusion pack`) & Operations Console Backend.
