# Sprint M5 — Evidence Preservation & Release Attestation Implementation Plan

> **Goal:** Implement a durable, verifiable release attestation subsystem (`ReleaseAssessment`, `AttestationBuilder`, `Signer`, `SignedAttestation`, `AttestationEnvelope`, `ArchiveBackend`) preserving M4 policy evaluation outcomes and M1–M3 gate evidence as cryptographically signed release attestations.

---

## Technical Architecture & Design Principles

- **Append-Only Immutable Archival:** `ArchiveBackend` enforces append-only storage semantics (`store`, `load`, `exists`, `list`); attestations are never mutated or updated.
- **Sole Authority for Canonical Serialization:** `AttestationBuilder` is the sole authority for generating canonical UTF-8 JSON bytes prior to signing.
- **Content-Derived Assessment Identity:** `assessment_id` is deterministically computed via `asm-<sha256_prefix>`.
- **Versioned & Abstract Signing:** `Signer` trait produces `SignatureBlock` (version `1`) supporting `MockSigner` (SHA-256 mock) and `Ed25519Signer` (real Ed25519 signature validation).
- **Four-Phase Verification Pipeline:**
  1. *Schema Validation:* Verifies schema & envelope versions.
  2. *Canonical Serialization:* Re-renders canonical bytes via `AttestationBuilder`.
  3. *Cryptographic Verification:* Validates signature using `signer.verify()`.
  4. *Semantic Consistency:* Asserts policy decision matches embedded evidence.

---

## File Structure

| File | Responsibility |
|------|---------------|
| `src/release/assessment.rs` | `ReleaseAssessment` struct, canonical JSON hasher (`compute_assessment_id`) |
| `src/release/attestation.rs` | `ReleaseAttestation`, `HostInfo`, `AttestationBuilder` (sole serialization authority) |
| `src/release/signing.rs` | `SignatureBlock`, `SignedAttestation`, `Signer` trait, `MockSigner`, `Ed25519Signer` |
| `src/release/envelope.rs` | `AttestationEnvelope` wrapper struct |
| `src/release/archive.rs` | `ArchiveBackend` trait, `FilesystemArchiveBackend` (append-only storage) |
| `src/release/verifier.rs` | 4-phase `AttestationVerifier` logic |
| `src/release/mod.rs` | Re-export M5 modules (`assessment`, `attestation`, `signing`, `envelope`, `archive`, `verifier`) |
| `src/bin/fusion.rs` | Add `fusion gates attest` and `fusion gates verify-attestation` CLI subcommands |
| `tests/release_attestation_tests.rs` | Integration tests for attestation generation, signing, archival, verification |

---

## Task Breakdown & Checklists

### Task 1: Canonical Assessment & Attestation Builder Infrastructure

**Files:**
- Create: `src/release/assessment.rs`
- Create: `src/release/attestation.rs`
- Modify: `src/release/mod.rs`

- [ ] **Step 1: Implement `ReleaseAssessment` in `src/release/assessment.rs`**

Add `ReleaseAssessment` struct and `compute_assessment_id(payload: &str) -> String` producing `asm-<sha256_12_hex>`.

- [ ] **Step 2: Implement `AttestationBuilder` in `src/release/attestation.rs`**

Implement `ReleaseAttestation`, `HostInfo`, and `AttestationBuilder::to_canonical_bytes(&attestation) -> Result<Vec<u8>, GateError>`. Ensure `AttestationBuilder` is the sole authority for canonical byte generation.

- [ ] **Step 3: Re-export in `src/release/mod.rs`**

- [ ] **Step 4: Verify unit tests**

Run: `cargo test release::assessment` and `cargo test release::attestation`

---

### Task 2: Signing Layer & Transport Envelope

**Files:**
- Create: `src/release/signing.rs`
- Create: `src/release/envelope.rs`
- Modify: `src/release/mod.rs`

- [ ] **Step 1: Implement `SignatureBlock`, `SignedAttestation`, and `Signer` trait in `src/release/signing.rs`**

Add `SignatureBlock` with `version: 1`, `algorithm: String`, `public_key_id: String`, `signature_bytes_base64: String`, `signed_at: DateTime<Utc>`.
Implement `MockSigner` (SHA-256 mockup) and `Ed25519Signer` (ed25519-dalek / ring backing).

- [ ] **Step 2: Implement `AttestationEnvelope` in `src/release/envelope.rs`**

Add `AttestationEnvelope` wrapping `SignedAttestation` with `envelope_version: "v1"`.

- [ ] **Step 3: Re-export in `src/release/mod.rs`**

- [ ] **Step 4: Verify unit tests**

Run: `cargo test release::signing` and `cargo test release::envelope`

---

### Task 3: Append-Only Archive Backend & 4-Phase Verifier

**Files:**
- Create: `src/release/archive.rs`
- Create: `src/release/verifier.rs`
- Modify: `src/release/mod.rs`

- [ ] **Step 1: Implement `ArchiveBackend` and `FilesystemArchiveBackend` in `src/release/archive.rs`**

Implement `store()`, `load()`, `exists()`, and `list()` with append-only storage semantics under `.fusion/attestations/*.json`.

- [ ] **Step 2: Implement `AttestationVerifier` in `src/release/verifier.rs`**

Implement 4-phase verification pipeline:
1. Schema Validation
2. Canonical Serialization
3. Cryptographic Verification
4. Semantic Consistency Check

- [ ] **Step 3: Re-export in `src/release/mod.rs`**

- [ ] **Step 4: Verify unit tests**

Run: `cargo test release::archive` and `cargo test release::verifier`

---

### Task 4: Governance CLI Integration & End-to-End Tests

**Files:**
- Modify: `src/bin/fusion.rs`
- Create: `tests/release_attestation_tests.rs`

- [ ] **Step 1: Add `Attest` and `VerifyAttestation` subcommands to `src/bin/fusion.rs`**

Add `fusion gates attest [--env production] [--output-dir .fusion/attestations]` and `fusion gates verify-attestation <PATH_OR_ID>`.

- [ ] **Step 2: Implement integration tests in `tests/release_attestation_tests.rs`**

End-to-end tests for attestation creation, signing, archival, loading, signature verification, and tampered payload rejection.

- [ ] **Step 3: Run full workspace quality checks**

Run:
1. `cargo test --lib release`
2. `cargo test --test release_attestation_tests`
3. `cargo test --bin fusion`
4. `cargo clippy`

---

## Verification Plan

### Automated Tests
- `cargo test release::assessment`
- `cargo test release::attestation`
- `cargo test release::signing`
- `cargo test release::envelope`
- `cargo test release::archive`
- `cargo test release::verifier`
- `cargo test --test release_attestation_tests`
- `cargo test --bin fusion`
- `cargo clippy`

### CLI Command Execution
Command: `cargo run --bin fusion -- gates attest --env production`
Command: `cargo run --bin fusion -- gates verify-attestation .fusion/attestations/<ID>.json`
Expected output: Confirms creation of signed attestation envelope and successful 4-phase verification.
