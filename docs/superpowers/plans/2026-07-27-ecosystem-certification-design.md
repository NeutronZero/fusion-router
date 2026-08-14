# Sprint M3 — Ecosystem Certification Design Specification

> **Goal:** Introduce static, artifact-based conformance gates for extension points (`Plugins`, `Strategies`, `Providers`, `Connectors`) verifying that every extension satisfies FusionRouter's platform contract before participating in execution.

---

## 1. Principles & Architectural Constitution

1. **Contract Certification over Implementation Validation:** Verify static conformance to platform expectations (manifest schemas, exported symbols, capability declarations, version compatibility, init hooks) rather than live runtime execution or network behavior.
2. **Offline & Deterministic:** Zero network calls, zero socket connections, zero external API keys required. Execution is 100% reproducible and CI-friendly.
3. **Infrastructure Reuse:** Extend M2's `FixtureLoader` and `FixtureManifest` infrastructure without duplicating file I/O or directory traversal logic.
4. **Consistent Gate Pattern:** All certification gates implement `ReleaseGate`, use backend traits, provide mock backends for tests, and register via `bootstrap::build_default_runner()`.

---

## 2. Shared Certification Abstraction, Context & Pipeline

Certification evaluates extensions through a unified inspection pipeline built on a common `CertificationContext` and `CertificationArtifact` abstraction:

```text
CertificationArtifact
       │
       ├─► Schema Validation (Structure, SerDe, Field Types)
       │
       ├─► Contract Validation (SDK Version, Capabilities, Symbol Exports, Init Hooks)
       │
       ▼
Aggregate Checks ──► GateResult / GateExecution Decision
```

### Shared Certification Context

```rust
pub struct CertificationContext {
    pub fixture_root: PathBuf,
    pub sdk_version: semver::Version,
    pub workspace_root: PathBuf,
}
```

### Common Certification Artifact Trait

```rust
pub trait CertificationArtifact: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &semver::Version;
    fn schema_checks(&self, ctx: &CertificationContext) -> Result<Vec<GateCheck>, GateError>;
    fn contract_checks(&self, ctx: &CertificationContext) -> Result<Vec<GateCheck>, GateError>;
}
```

Concrete artifact implementations specialize this pipeline:
- `PluginArtifact` (`src/release/gates/plugin.rs`)
- `StrategyArtifact` (`src/release/gates/strategy.rs`)
- `ProviderArtifact` (`src/release/gates/provider.rs`)
- `ConnectorArtifact` (`src/release/gates/connector.rs`)

---

## 3. Shared Fixture Infrastructure Extensions

Extend `FixtureKind` in `src/release/fixture.rs` to support all extension categories:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureKind {
    Configs,
    Snapshots,
    Plugins,
    Strategies,
    Providers,
    Connectors,
}
```

Manifest entries in `tests/fixtures/manifest.yaml` support optional stable IDs:

```yaml
plugins:
  - id: echo
    version: "0.10.0"
    path: plugins/echo
    expected:
      outcome: pass

strategies:
  - id: single
    version: "0.10.0"
    path: strategies/single
    expected:
      outcome: pass

providers:
  - id: openai
    version: "0.10.0"
    path: providers/openai
    expected:
      outcome: pass

connectors:
  - id: http
    version: "0.10.0"
    path: connectors/http
    expected:
      outcome: pass
```

---

## 4. Certification Gate Specifications

### 4.1 PLG-1 — Plugin Conformance Gate (`src/release/gates/plugin.rs`)

- **Identity:** `GateId::Plugin1` ("PLG-1"), Category `GateCategory::Certification`, Required: `true`.
- **Description:** "Verify plugin manifest, capability contracts, symbol exports, and initialization compatibility."
- **Artifact:** `PluginArtifact` implementing `CertificationArtifact`.
- **Invariants Checked:**
  - Manifest structure & SerDe (`plugin.yaml` / `Cargo.toml` metadata).
  - SDK Version compatibility (matches host platform semver requirements).
  - Required symbol exports (e.g. `create_plugin`, `plugin_api_version`).
  - Capability declarations (`CapabilityContract`, `CapabilityId`).
  - Initialization contract compatibility (valid default config schema).

### 4.2 STR-1 — Strategy Conformance Gate (`src/release/gates/strategy.rs`)

- **Identity:** `GateId::Strategy1` ("STR-1"), Category `GateCategory::Certification`, Required: `false`.
- **Description:** "Verify routing strategy registration, compiler compatibility, and execution graph compilation."
- **Artifact:** `StrategyArtifact` implementing `CertificationArtifact`.
- **Invariants Checked:**
  - Strategy descriptor and metadata schema.
  - Registration identifier uniqueness and pattern matching.
  - Compiler Integration (produces a compiler-valid `ExecutionGraph` for canonical fixture inputs).
  - Policy Compatibility (handles fallback, retry, and cost constraints).
  - Scheduling hints & execution metadata format.

### 4.3 PRV-1 — Provider Conformance Gate (`src/release/gates/provider.rs`)

- **Identity:** `GateId::Provider1` ("PRV-1"), Category `GateCategory::Certification`, Required: `false`.
- **Description:** "Verify provider catalog declarations, pricing metadata schema, model identifiers, and retry contracts."
- **Artifact:** `ProviderArtifact` implementing `CertificationArtifact`.
- **Invariants Checked:**
  - Provider manifest structure and model catalog definition.
  - Model identifier mapping & capability matrix.
  - Pricing metadata schema & token counting descriptors.
  - Timeout and retry policy declaration.
  - Authentication descriptor schema (without requiring live secrets).

### 4.4 CON-1 — Connector Conformance Gate (`src/release/gates/connector.rs`)

- **Identity:** `GateId::Connector1` ("CON-1"), Category `GateCategory::Certification`, Required: `false`.
- **Description:** "Verify connector protocol schema, serialization compatibility, and health endpoint declarations."
- **Artifact:** `ConnectorArtifact` implementing `CertificationArtifact`.
- **Invariants Checked:**
  - Connector descriptor and protocol schema version.
  - Request/Response serialization compatibility.
  - Health endpoint declaration/schema.
  - Credential descriptor schema.
  - Feature & transport capability flags.

---

## 5. Bootstrap Composition & Governance CLI Integration

`build_default_runner()` in `src/release/bootstrap.rs` registers all 8 release gates:

```text
Registered release gates:
  [SDK-1] SemVer Compatibility Gate - Checks SemVer compatibility via cargo semver-checks
  [RPL-1] Replay Compatibility - Verify replay snapshots remain readable and structurally valid
  [UPG-1] Configuration Upgrade - Verify historical configs load correctly through the current parser
  [DET-1] Planner Determinism - Verify same planner input produces identical execution graphs
  [PLG-1] Plugin Conformance - Verify plugin manifest, capability contracts, symbol exports, and init compatibility
  [STR-1] Strategy Conformance - Verify routing strategy registration, compiler compatibility, and graph compilation
  [PRV-1] Provider Conformance - Verify provider catalog declarations, pricing metadata schema, and retry contracts
  [CON-1] Connector Conformance - Verify connector protocol schema, serialization, and health endpoint declarations
```

---

## 6. Forward Compatibility with M4 (Release Policy Engine)

All M3 certification gates return `GateExecution::Success(GateResult)` containing structured `GateCheck` entries and `GateMetadata`. This emission structure allows M4's policy engine to evaluate rules (e.g. `require PLG-1 for all production release builds`, `allow PRV-1 advisory waivers`) without modifying gate interfaces.
