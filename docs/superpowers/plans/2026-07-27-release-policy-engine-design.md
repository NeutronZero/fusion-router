# Sprint M4 — Release Policy Engine Design Specification

> **Goal:** Introduce a data-driven, declarative Release Policy Engine that consumes structured evidence emitted by release gates (M1–M3) and evaluates environment-specific rules, waivers, and advisory status to yield a formal `ReleaseDecision`.

---

## 1. Principles & Architectural Constitution

1. **Strict Decoupling of Evidence Generation & Policy Evaluation:**
   Release gates (M1–M3) only produce evidence (`GateResult`, `GateCheck`, `GateMetadata`). Gates do NOT possess environment context, policy knowledge, waiver logic, or deployment permissions.
2. **Data-Driven Declarative Policies:**
   Policies are defined as YAML descriptors (`policy.yaml`) mapping typed environments (`Production`, `Staging`, `Development`, `Custom(String)`) to required and advisory gates.
3. **Auditable, Scoped & Identified Waivers:**
   Waivers specify explicit gate overrides with mandatory fields: `id` (e.g. `waiver-2026-0042`), `gate`, `artifact`, `reason`, `expires` (RFC3339 timestamp), and `approved_by`. Expired or malformed waivers are automatically rejected.
4. **Structured Decision Outcomes & Immutable Context:**
   Evaluation executes against an immutable `EvaluationContext` and returns a structured `PolicyEvaluation` report containing an explicit `ReleaseDecision` (`Approved`, `ApprovedWithWaivers`, `Blocked`) with a `PolicySummary`.

---

## 2. Pipeline & Policy Architecture

Evaluation separates **Evidence Classification** from **Policy Application**:

```text
Vec<GateResult> (M1-M3 Evidence)
       │
       ▼
EvaluationContext (Environment, PolicyDefinition, WaiverSet, EvaluationTime)
       │
       ├─► 1. Evidence Classification (Categorize Passed, Failed, Advisory, Ignored)
       ├─► 2. Policy Application (Validate Waiver Expiry, Match IDs/Artifacts)
       │
       ▼
PolicySummary ──► PolicyEvaluation ──► ReleaseDecision (Approved, ApprovedWithWaivers, Blocked)
```

---

## 3. Data Models & Schemas

### 3.1 Typed Environment & Policy Definition Schema (`src/release/policy.rs`)

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseEnvironment {
    Production,
    Staging,
    Development,
    Custom(String),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PolicyDefinition {
    pub name: String,
    pub environments: HashMap<ReleaseEnvironment, EnvironmentPolicy>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EnvironmentPolicy {
    #[serde(default)]
    pub require: Vec<GateId>,
    #[serde(default)]
    pub advisory: Vec<GateId>,
}
```

Example `policy.yaml`:

```yaml
name: standard-release-policy
environments:
  production:
    require:
      - SDK-1
      - RPL-1
      - UPG-1
      - DET-1
      - PLG-1
    advisory:
      - STR-1
      - PRV-1
      - CON-1

  staging:
    require:
      - SDK-1
      - UPG-1
      - PLG-1
    advisory:
      - RPL-1
      - DET-1
      - STR-1
      - PRV-1
      - CON-1
```

### 3.2 Waiver Schema with Stable ID (`src/release/waiver.rs`)

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Waiver {
    pub id: String,
    pub gate: GateId,
    pub artifact: Option<String>,
    pub reason: String,
    pub expires: chrono::DateTime<chrono::Utc>,
    pub approved_by: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct WaiverSet {
    #[serde(default)]
    pub waivers: Vec<Waiver>,
}
```

Example `waivers.yaml`:

```yaml
waivers:
  - id: waiver-2026-0042
    gate: PRV-1
    artifact: openai
    reason: "Pricing metadata update pending upstream sync"
    expires: "2026-09-30T00:00:00Z"
    approved_by: "architecture-team"
```

### 3.3 Immutable Evaluation Context & Results (`src/release/evaluator.rs`)

```rust
#[derive(Debug, Clone)]
pub struct EvaluationContext {
    pub environment: ReleaseEnvironment,
    pub policy: PolicyDefinition,
    pub waivers: WaiverSet,
    pub evaluation_time: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReleaseDecision {
    Approved,
    ApprovedWithWaivers,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicySummary {
    pub total_gates: usize,
    pub passed: usize,
    pub required_failed: usize,
    pub waived: usize,
    pub advisory_failed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyEvaluation {
    pub environment: ReleaseEnvironment,
    pub decision: ReleaseDecision,
    pub summary: PolicySummary,
    pub required_failures: Vec<GateId>,
    pub waived_failures: Vec<WaiverEvaluation>,
    pub advisory_failures: Vec<GateId>,
    pub passed_gates: Vec<GateId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaiverEvaluation {
    pub waiver: Waiver,
    pub active: bool,
}
```

---

## 4. Evaluator Rules & Decision Matrix

1. **Approved:** All gates listed in `require` for the target environment have passed (`GateResult::passed == true`), and `required_failed == 0`.
2. **ApprovedWithWaivers:** One or more required gates failed, but every required failure is covered by an active, unexpired, matching waiver (`required_failed == 0` after waiver matching).
3. **Blocked:** At least one required gate failed and is NOT covered by a valid, active waiver (`required_failed > 0`).
4. **Advisory Failures:** Failed gates listed in `advisory` do NOT block the release decision but are logged as advisory warnings in `PolicyEvaluation` and `PolicySummary`.

---

## 5. Governance CLI Integration

Extend `fusion gates` CLI subcommands in `src/bin/fusion.rs`:

```text
fusion gates evaluate --env production [--policy path/to/policy.yaml] [--waivers path/to/waivers.yaml]
```

Output format:

```text
Release Policy Evaluation Report
Environment: production
Decision: APPROVED_WITH_WAIVERS

Summary: 8 total gates, 7 passed, 0 required failed, 1 waived, 0 advisory failed.

Required Gates Passed: SDK-1, RPL-1, UPG-1, DET-1
Waived Failures:
  [waiver-2026-0042] PRV-1 Provider Conformance (Waiver approved by architecture-team until 2026-09-30)
Advisory Failures: None
```

---

## 6. Forward Compatibility with M5 (Release Evidence & Attestation)

The `PolicyEvaluation` report, coupled with `EvaluationContext` and `Vec<GateResult>`, forms a complete, self-contained release attestation payload that can be cryptographically signed and archived for compliance reporting in M5.
