# ADR-019: Capability Host Interface Specification

* **Status:** Approved
* **Date:** 2026-07-27
* **Subsystem:** Capability Platform / Runtime Services

---

## Context

ADR-018 defines the binary package format (`.fusionpkg`), manifest schemas, and WebAssembly target ABI for capabilities. Capabilities executing inside a sandboxed environment require access to runtime services (event publication, structured logging, policy-scoped secrets, policy-scoped outbound HTTP, and metrics) without direct coupling to FusionRouter internal structures.

---

## Decision

We introduce **ADR-019: Capability Host Interface** defining the `CapabilityHostServices` trait and runtime host function contracts exposed to executing capabilities.

### 1. `CapabilityHostServices` Trait

```rust
#[async_trait]
pub trait CapabilityHostServices: Send + Sync {
    async fn emit_event(&self, event: ExecutionEvent) -> Result<(), GateError>;
    async fn log(&self, level: tracing::Level, message: &str);
    async fn fetch_secret(&self, secret_name: &str) -> Result<String, GateError>;
    async fn http_request(&self, req: reqwest::Request) -> Result<reqwest::Response, GateError>;
    fn record_metric(&self, name: &str, value: f64);
}
```

### 2. Policy-Gated Service Boundaries

- **Secrets Access:** Gated by `Permission::Secrets(Vec<String>)`. Attempts to read undeclared secrets return `GateError::PermissionDenied`.
- **HTTP Access:** Gated by `Permission::Http(Vec<UrlPattern>)`. Outbound requests to undeclared host patterns are rejected before execution.
- **Event Integration:** Calls to `emit_event` wrap payloads into `ExecutionEventEnvelope` and publish onto the ADR-017 Event Stream under the current capability's correlation context.

### 3. Abstract `SandboxRuntime` Trait

```rust
#[async_trait]
pub trait SandboxRuntime: Send + Sync {
    fn name(&self) -> &'static str;
    async fn instantiate(&self, module_bytes: &[u8], host_services: Arc<dyn CapabilityHostServices>) -> Result<Box<dyn SandboxInstance>, GateError>;
}
```

Initial implementation: `WasmtimeSandboxRuntime` implementing `SandboxRuntime`.

---

## Consequence

- **Pros:** Clean decoupling of capability logic from runtime internals; enables alternative sandbox backends (`Wasmtime`, `Wasmer`, OCI) without modifying host service contracts.
- **Cons:** Additional host function marshaling layer, mitigated by `fusion-capability-sdk`.
