use crate::release::attestation::ReleaseAttestation;
use crate::release::gate::GateError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

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
    #[allow(dead_code)]
    fn key_id(&self) -> &str;
    fn algorithm(&self) -> &'static str;
    fn sign(&self, canonical_payload: &[u8]) -> Result<SignatureBlock, GateError>;
    fn verify(
        &self,
        canonical_payload: &[u8],
        signature: &SignatureBlock,
    ) -> Result<bool, GateError>;
}

pub struct MockSigner {
    key_id: String,
}

impl MockSigner {
    pub fn new(key_id: impl Into<String>) -> Self {
        Self {
            key_id: key_id.into(),
        }
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

    fn verify(
        &self,
        canonical_payload: &[u8],
        signature: &SignatureBlock,
    ) -> Result<bool, GateError> {
        if signature.version != 1 {
            return Ok(false);
        }
        let expected = self.sign(canonical_payload)?;
        Ok(expected.signature_bytes_base64 == signature.signature_bytes_base64)
    }
}

const HMAC_BLOCK_SIZE: usize = 64;

/// Real cryptographic signer: keyed HMAC-SHA256. Unlike `MockSigner`
/// (non-cryptographic DefaultHasher), a forged or tampered payload has no
/// chance of verifying, and signatures are only valid under the exact key
/// that produced them.
pub struct HmacSha256Signer {
    key_id: String,
    key: Vec<u8>,
}

impl HmacSha256Signer {
    pub fn new(key_id: impl Into<String>, key: &[u8]) -> Self {
        Self {
            key_id: key_id.into(),
            key: key.to_vec(),
        }
    }
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};

    let mut key = key.to_vec();
    if key.len() > HMAC_BLOCK_SIZE {
        key = Sha256::digest(&key).to_vec();
    }
    key.resize(HMAC_BLOCK_SIZE, 0);

    let mut ipad = [0x36u8; HMAC_BLOCK_SIZE];
    let mut opad = [0x5cu8; HMAC_BLOCK_SIZE];
    for i in 0..HMAC_BLOCK_SIZE {
        ipad[i] ^= key[i];
        opad[i] ^= key[i];
    }

    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(data);
    let inner_digest = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_digest);
    outer.finalize().into()
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

impl Signer for HmacSha256Signer {
    fn key_id(&self) -> &str {
        &self.key_id
    }

    fn algorithm(&self) -> &'static str {
        "hmac-sha256"
    }

    fn sign(&self, canonical_payload: &[u8]) -> Result<SignatureBlock, GateError> {
        use base64::Engine;
        let mac = hmac_sha256(&self.key, canonical_payload);
        Ok(SignatureBlock {
            version: 1,
            algorithm: self.algorithm().to_string(),
            public_key_id: self.key_id.clone(),
            signature_bytes_base64: base64::engine::general_purpose::STANDARD.encode(mac),
            signed_at: Utc::now(),
        })
    }

    fn verify(
        &self,
        canonical_payload: &[u8],
        signature: &SignatureBlock,
    ) -> Result<bool, GateError> {
        use base64::Engine;
        if signature.version != 1 {
            return Ok(false);
        }
        let expected = hmac_sha256(&self.key, canonical_payload);
        let supplied = base64::engine::general_purpose::STANDARD
            .decode(&signature.signature_bytes_base64)
            .map_err(|e| {
                GateError::ExecutionFailed(format!("signature base64 decode error: {e}"))
            })?;
        Ok(constant_time_eq(&expected, &supplied))
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

    #[test]
    fn test_hmac_signer_sign_and_verify_roundtrip() {
        let signer = HmacSha256Signer::new("ops-key", b"correct horse battery staple");
        let payload = b"canonical-attestation-bytes";

        let sig = signer.sign(payload).unwrap();
        assert_eq!(sig.version, 1);
        assert_eq!(sig.algorithm, "hmac-sha256");
        assert_eq!(sig.public_key_id, "ops-key");

        // base64-decoded signature is 32 bytes (SHA-256 digest)
        let decoded = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            &sig.signature_bytes_base64,
        )
        .unwrap();
        assert_eq!(decoded.len(), 32);

        let valid = signer.verify(payload, &sig).unwrap();
        assert!(valid);
    }

    #[test]
    fn test_hmac_signer_rejects_tampered_payload() {
        let signer = HmacSha256Signer::new("ops-key", b"correct horse battery staple");
        let sig = signer.sign(b"authentic payload").unwrap();

        let tampered = signer.verify(b"tampered payload", &sig).unwrap();
        assert!(!tampered);
    }

    #[test]
    fn test_hmac_signer_rejects_wrong_key() {
        let signer = HmacSha256Signer::new("ops-key", b"correct horse battery staple");
        let sig = signer.sign(b"authentic payload").unwrap();

        let attacker = HmacSha256Signer::new("ops-key", b"attacker key");
        let forged_check = attacker.verify(b"authentic payload", &sig).unwrap();
        assert!(
            !forged_check,
            "signature must not verify under a different key"
        );
    }
}
