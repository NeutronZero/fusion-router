use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use crate::release::attestation::ReleaseAttestation;
use crate::release::gate::GateError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureBlock {
    pub version: u32,
    pub algorithm: String,
    pub public_key_id: String,
    pub signature_bytes_base64: String,
    pub signed_at: DateTime<Utc>,
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

pub struct MockSigner {
    key_id: String,
}

impl MockSigner {
    pub fn new(key_id: impl Into<String>) -> Self {
        Self { key_id: key_id.into() }
    }
}

impl Default for MockSigner {
    fn default() -> Self {
        Self::new("mock-key-1")
    }
}

impl Signer for MockSigner {
    fn key_id(&self) -> &str {
        &self.key_id
    }

    fn algorithm(&self) -> &'static str {
        "mock-sha256"
    }

    fn sign(&self, canonical_payload: &[u8]) -> Result<SignatureBlock, GateError> {
        let mut hasher = DefaultHasher::new();
        canonical_payload.hash(&mut hasher);
        self.key_id.hash(&mut hasher);
        let hash = hasher.finish();

        Ok(SignatureBlock {
            version: 1,
            algorithm: self.algorithm().to_string(),
            public_key_id: self.key_id.clone(),
            signature_bytes_base64: format!("{:016x}", hash),
            signed_at: Utc::now(),
        })
    }

    fn verify(&self, canonical_payload: &[u8], signature: &SignatureBlock) -> Result<bool, GateError> {
        if signature.version != 1 {
            return Ok(false);
        }
        let expected = self.sign(canonical_payload)?;
        Ok(expected.signature_bytes_base64 == signature.signature_bytes_base64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_signer_sign_and_verify() {
        let signer = MockSigner::default();
        let payload = b"canonical-attestation-bytes";

        let sig = signer.sign(payload).unwrap();
        assert_eq!(sig.version, 1);
        assert_eq!(sig.algorithm, "mock-sha256");

        let valid = signer.verify(payload, &sig).unwrap();
        assert!(valid);

        let tampered_payload = b"tampered-bytes";
        let invalid = signer.verify(tampered_payload, &sig).unwrap();
        assert!(!invalid);
    }
}
