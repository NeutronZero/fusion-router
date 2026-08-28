use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use fusion_core::PlatformError;
use ring::aead::{self, BoundKey, OpeningKey, SealingKey, UnboundKey, AES_256_GCM, NONCE_LEN};
use ring::rand::{SecureRandom, SystemRandom};
use std::sync::Arc;
use zeroize::Zeroizing;

struct OneNonce(Option<aead::Nonce>);

impl aead::NonceSequence for OneNonce {
    fn advance(&mut self) -> Result<aead::Nonce, ring::error::Unspecified> {
        self.0.take().ok_or(ring::error::Unspecified)
    }
}

pub struct SecretManager {
    /// Master key wrapped in `Zeroizing`: the buffer is wiped when the
    /// manager (or any temporary copy produced from it) is dropped instead of
    /// leaking key material into freed heap memory.
    key_bytes: Zeroizing<[u8; 32]>,
    rng: SystemRandom,
}

impl SecretManager {
    pub fn new(key_bytes: [u8; 32]) -> Self {
        Self {
            key_bytes: Zeroizing::new(key_bytes),
            rng: SystemRandom::new(),
        }
    }

    pub fn generate_random_key() -> [u8; 32] {
        let mut key = [0u8; 32];
        let rng = SystemRandom::new();
        rng.fill(&mut key).expect("Secure random generation");
        key
    }

    /// Builds a manager from a base64-encoded 32-byte master key.
    pub fn from_base64_key(key_b64: &str) -> Result<Self, PlatformError> {
        use zeroize::Zeroize;
        let mut raw = BASE64
            .decode(key_b64.trim())
            .map_err(|e| PlatformError::Security {
                code: "MASTER_KEY_DECODE".to_string(),
                message: e.to_string(),
                recovery_suggestion: "FUSION_MASTER_KEY must be base64-encoded 32 bytes"
                    .to_string(),
            })?;
        if raw.len() != 32 {
            let len = raw.len();
            raw.zeroize();
            return Err(PlatformError::Security {
                code: "MASTER_KEY_LEN".to_string(),
                message: format!("master key is {} bytes, expected 32", len),
                recovery_suggestion: "Regenerate with 32 random bytes, base64-encoded".to_string(),
            });
        }
        let mut key = Zeroizing::new([0u8; 32]);
        key.copy_from_slice(&raw);
        // Wipe the caller's decoded buffer immediately; the stored copy is
        // wiped again on drop by Zeroizing.
        raw.zeroize();
        Ok(Self {
            key_bytes: key,
            rng: SystemRandom::new(),
        })
    }

    /// Builds a manager from the `FUSION_MASTER_KEY` environment variable.
    pub fn from_env() -> Result<Self, PlatformError> {
        let value = std::env::var("FUSION_MASTER_KEY").map_err(|_| PlatformError::Security {
            code: "MASTER_KEY_MISSING".to_string(),
            message: "FUSION_MASTER_KEY is not set".to_string(),
            recovery_suggestion:
                "Set FUSION_MASTER_KEY to a base64-encoded 32-byte key to use encrypted provider keys"
                    .to_string(),
        })?;
        Self::from_base64_key(&value)
    }

    /// Returns this manager's key base64-encoded, for provisioning
    /// `FUSION_MASTER_KEY` on hosts that will consume encrypted values.
    ///
    /// Test-only: this hands the raw master key to any caller, so production
    /// builds must not be able to reach it. The sole caller is a cfg(test)
    /// round-trip test in providers/factory.rs.
    #[cfg(test)]
    pub fn export_master_key_base64(&self) -> String {
        BASE64.encode(self.key_bytes.as_slice())
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<String, PlatformError> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        self.rng
            .fill(&mut nonce_bytes)
            .map_err(|e| PlatformError::Security {
                code: "RNG_FAIL".to_string(),
                message: e.to_string(),
                recovery_suggestion: "Check system entropy source".to_string(),
            })?;

        let unbound_key =
            UnboundKey::new(&AES_256_GCM, self.key_bytes.as_slice()).map_err(|_| {
                PlatformError::Security {
                    code: "INVALID_KEY".to_string(),
                    message: "Invalid AES-256 key".to_string(),
                    recovery_suggestion: "Provide a 32-byte secret key".to_string(),
                }
            })?;

        let nonce = aead::Nonce::try_assume_unique_for_key(&nonce_bytes).map_err(|_| {
            PlatformError::Security {
                code: "NONCE_ERR".to_string(),
                message: "Failed to construct nonce".to_string(),
                recovery_suggestion: "Check nonce generation".to_string(),
            }
        })?;

        let mut sealing_key = SealingKey::new(unbound_key, OneNonce(Some(nonce)));
        let mut in_out = plaintext.as_bytes().to_vec();

        sealing_key
            .seal_in_place_append_tag(aead::Aad::empty(), &mut in_out)
            .map_err(|_| PlatformError::Security {
                code: "ENCRYPT_ERR".to_string(),
                message: "Encryption failed".to_string(),
                recovery_suggestion: "Check payload size and encryption key".to_string(),
            })?;

        let mut result = nonce_bytes.to_vec();
        result.extend_from_slice(&in_out);

        Ok(BASE64.encode(result))
    }

    pub fn decrypt(&self, ciphertext_b64: &str) -> Result<String, PlatformError> {
        let payload = BASE64
            .decode(ciphertext_b64)
            .map_err(|e| PlatformError::Security {
                code: "BASE64_DECODE_ERR".to_string(),
                message: e.to_string(),
                recovery_suggestion: "Ensure base64 encoded ciphertext".to_string(),
            })?;

        if payload.len() < NONCE_LEN {
            return Err(PlatformError::Security {
                code: "CIPHERTEXT_TOO_SHORT".to_string(),
                message: "Ciphertext too short".to_string(),
                recovery_suggestion: "Provide valid ciphertext".to_string(),
            });
        }

        let (nonce_bytes, ciphertext) = payload.split_at(NONCE_LEN);

        let unbound_key =
            UnboundKey::new(&AES_256_GCM, self.key_bytes.as_slice()).map_err(|_| {
                PlatformError::Security {
                    code: "INVALID_KEY".to_string(),
                    message: "Invalid AES-256 key".to_string(),
                    recovery_suggestion: "Provide a 32-byte secret key".to_string(),
                }
            })?;

        let nonce = aead::Nonce::try_assume_unique_for_key(nonce_bytes).map_err(|_| {
            PlatformError::Security {
                code: "NONCE_ERR".to_string(),
                message: "Failed to construct nonce".to_string(),
                recovery_suggestion: "Check nonce generation".to_string(),
            }
        })?;

        let mut opening_key = OpeningKey::new(unbound_key, OneNonce(Some(nonce)));
        let mut in_out = ciphertext.to_vec();

        let plaintext_bytes = opening_key
            .open_in_place(aead::Aad::empty(), &mut in_out)
            .map_err(|_| PlatformError::Security {
                code: "DECRYPT_ERR".to_string(),
                message: "Decryption failed or invalid tag".to_string(),
                recovery_suggestion: "Verify encryption key and ciphertext integrity".to_string(),
            })?;

        String::from_utf8(plaintext_bytes.to_vec()).map_err(|e| PlatformError::Security {
            code: "UTF8_ERR".to_string(),
            message: e.to_string(),
            recovery_suggestion: "Ensure decrypted bytes form valid UTF-8".to_string(),
        })
    }

    pub fn redact(&self, _secret: &str) -> String {
        "****".to_string()
    }
}

pub type SharedSecretManager = Arc<SecretManager>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_manager_encrypt_decrypt_round_trip() {
        let key = SecretManager::generate_random_key();
        let manager = SecretManager::new(key);
        let secret = "sk-openrouter-secret-key-12345";

        let encrypted = manager.encrypt(secret).expect("Encrypt secret");
        assert_ne!(secret, encrypted);

        let decrypted = manager.decrypt(&encrypted).expect("Decrypt secret");
        assert_eq!(secret, decrypted);
    }

    #[test]
    fn test_secret_manager_redact() {
        let key = SecretManager::generate_random_key();
        let manager = SecretManager::new(key);

        assert_eq!(manager.redact("sk-openrouter-secret-key-12345"), "****");
        assert_eq!(manager.redact("ab"), "****");
    }

    #[test]
    fn test_key_material_is_zeroized_on_drop() {
        // Type-level guarantee: the master key must be stored inside
        // `Zeroizing`, whose Drop wipes the heap buffer before freeing.
        let key: [u8; 32] = core::array::from_fn(|i| i as u8);
        let manager = SecretManager::new(key);
        let stored: &Zeroizing<[u8; 32]> = &manager.key_bytes;
        assert_eq!(stored[0], 0);

        // The zeroization machinery itself behaves as expected on a live
        // (safe) buffer.
        let mut scratch = [7u8; 32];
        use zeroize::Zeroize;
        scratch.zeroize();
        assert!(scratch.iter().all(|&b| b == 0), "zeroize must wipe bytes");

        // Encryption behavior unchanged by the wrapper.
        let ciphertext = manager.encrypt("payload").unwrap();
        assert_eq!(manager.decrypt(&ciphertext).unwrap(), "payload");
    }

    #[test]
    fn test_export_master_key_base64_round_trips_to_env_form() {
        let key_b64 = BASE64.encode([7u8; 32]);
        let manager = SecretManager::from_base64_key(&key_b64).unwrap();
        assert_eq!(manager.export_master_key_base64(), key_b64);
    }

    #[test]
    fn test_secret_manager_redact_multibyte_utf8() {
        let key = SecretManager::generate_random_key();
        let manager = SecretManager::new(key);
        // Short Unicode secret (<= 4 chars)
        assert_eq!(manager.redact("🔑🔐"), "****");
        // Long Unicode secret (> 4 chars) must still be fully redacted
        let unicode_secret = "🔑🔑🔑🔑secret🔑🔑🔑🔑";
        let redacted = manager.redact(unicode_secret);
        assert_eq!(redacted, "****");
    }
}
