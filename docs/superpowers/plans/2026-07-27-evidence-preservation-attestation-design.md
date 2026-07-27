# Sprint M5 — Evidence Preservation & Release Attestation Design Specification

> **Goal:** Introduce a durable, verifiable, and portable attestation subsystem (`ReleaseAssessment`, `AttestationBuilder`, `Signer`, `ArchiveBackend`, `AttestationEnvelope`) that captures M4 policy evaluation outcomes and M1–M3 gate evidence into cryptographically signed release attestations.

---

## 1. Principles & Architectural Constitution

1. **Pure Preservation & Provenance:**
   Sprint M5 does NOT re-evaluate gates or policies. It consumes completed `ReleaseAssessment` bundles (`Vec<GateResult>` + `PolicyEvaluation`) and produces signed, immutable attestations.
2. **Sole Authority for Canonical Serialization:**
   `AttestationBuilder` is the sole authority for generating canonical JSON representations with deterministic field ordering prior to signing. No signer or archive component serializes independently.
3. **Content-Derived Assessment Identity:**
   `assessment_id` is deterministically derived from canonical SHA-256 content hashes (e.g. `asm-8a9f3b12...`) guaranteeing immutable payload-to-ID binding.
4. **Pluggable & Versioned Signing Layer:**
   A versioned `SignatureBlock` (version `1`) and abstract `Signer` trait decouple key management from attestation structures. Initial backends include `MockSigner` (CI testing) and `Ed25519Signer` (local keypairs).
5. **Envelope & Archive Storage:**
   Signed attestations are wrapped in an `AttestationEnvelope` and persisted via `ArchiveBackend` (`FilesystemArchiveBackend`, `InMemoryArchiveBackend`).

---

## 2. Subsystem Architecture & Pipeline

```text
ReleaseAssessment (Vec<GateResult> + PolicyEvaluation)
       │
       ├─► Compute Content Hash -> assessment_id
       │
       ▼
1. AttestationBuilder (Sole Authority for Canonical JSON Serialization)
       │
       ▼
ReleaseAttestation
       │
       ▼
2. Signer Trait (MockSigner, Ed25519Signer) -> SignatureBlock (versioned)
       │
       ▼
SignedAttestation ──► AttestationEnvelope
       │
       ▼
3. ArchiveBackend Trait (FilesystemArchiveBackend, with exists check)
       │
       ▼
Durable Compliance Store (.fusion/attestations/*.json)
```

---

## 3. Data Models & Schemas

### 3.1 Immutable Release Assessment Bundle (`src/release/assessment.rs`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseAssessment {
    pub assessment_id: String, // e.g. "asm-8a9f3b12c4d5" derived from SHA-256
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub environment: ReleaseEnvironment,
    pub policy_evaluation: PolicyEvaluation,
    pub gate_results: Vec<GateResult>,
}
```

### 3.2 Portable Attestation Descriptor (`src/release/attestation.rs`)

```rust
pub const ATTESTATION_SCHEMA_VERSION: &str = "fusion.router.release.attestation.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseAttestation {
    pub schema_version: String,
    pub assessment: ReleaseAssessment,
    pub host_info: HostInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostInfo {
    pub fusion_version: String,
    pub os: String,
    pub arch: String,
}
```

### 3.3 Versioned Signature & Signed Attestation (`src/release/signing.rs`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureBlock {
    pub version: u32, // Version 1
    pub algorithm: String, // e.g. "ed25519" or "mock-sha256"
    pub public_key_id: String,
    pub signature_bytes_base64: String,
    pub signed_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedAttestation {
    pub attestation: ReleaseAttestation,
    pub signature: SignatureBlock,
}

pub trait Signer: Send + Sync {
    fn key_id(&self) -> &str;
    fn algorithm(&self) -> &'static str;
    fn sign(&self, canonical_payload: &[u8]) -> Result<SignatureBlock, GateError>;
    fn verify(&self, canonical_payload: &[u8], signature: &SignatureBlock) -> Result<bool, GateError>;
}
```

### 3.4 Transport Attestation Envelope (`src/release/envelope.rs`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationEnvelope {
    pub envelope_version: String, // "v1"
    pub signed_attestation: SignedAttestation,
}
```

### 3.5 Archive Storage Interface (`src/release/archive.rs`)

```rust
pub trait ArchiveBackend: Send + Sync {
    fn store(&self, envelope: &AttestationEnvelope) -> Result<PathBuf, GateError>;
    fn load(&self, assessment_id: &str) -> Result<AttestationEnvelope, GateError>;
    fn exists(&self, assessment_id: &str) -> bool;
    fn list(&self) -> Result<Vec<String>, GateError>;
}
```

---

## 4. Four-Phase Verification & Validation Pipeline

Attestation verification executes across 4 explicit phases:

```text
AttestationEnvelope
       │
       ├─► 1. Schema Validation (Verify schema_version & envelope_version)
       │
       ├─► 2. Canonical Serialization (Re-render canonical JSON via AttestationBuilder)
       │
       ├─► 3. Cryptographic Verification (Verify signature against canonical bytes)
       │
       └─► 4. Semantic Consistency (Optionally verify policy decision matches gate evidence)
```

---

## 5. Governance CLI Integration

Extend `fusion gates` CLI subcommands in `src/bin/fusion.rs`:

```text
fusion gates attest --env production [--output-dir .fusion/attestations] [--key path/to/key.pem]
fusion gates verify-attestation <PATH_OR_ID> [--key path/to/pubkey.pem]
```

Example CLI Output:

```text
Signed Release Attestation Created
Assessment ID: asm-8a9f3b12c4d5
Environment: production
Decision: Approved
Signature Algorithm: ed25519 (Key: release-prod-2026, SigVersion: 1)
Saved to: .fusion/attestations/asm-8a9f3b12c4d5.json
```

---

## 6. Subsystem Completeness & Epic M Closing State

Sprint M5 completes Epic M's release governance architecture:

```text
Gates (M1-M3) ──► Evidence ──► Policy Evaluation (M4) ──► Decision ──► Signed Attestation Envelope (M5)
```
