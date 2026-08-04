use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use fusion_core::PlatformError;
use ring::aead::{self, BoundKey, SealingKey, UnboundKey, OpeningKey, AES_256_GCM, NONCE_LEN};
use ring::rand::{SecureRandom, SystemRandom};
use std::sync::Arc;

struct OneNonce(Option<aead::Nonce>);

impl aead::NonceSequence for OneNonce {
    fn advance(&mut self) -> Result<aead::Nonce, ring::error::Unspecified> {
        self.0.take().ok_or(ring::error::Unspecified)
    }
}

pub struct SecretManager {
    key_bytes: [u8; 32],
    rng: SystemRandom,
}

impl SecretManager {
    pub fn new(key_bytes: [u8; 32]) -> Self {
        Self {
            key_bytes,
            rng: SystemRandom::new(),
        }
    }

    pub fn generate_random_key() -> [u8; 32] {
        let mut key = [0u8; 32];
        let rng = SystemRandom::new();
        rng.fill(&mut key).expect("Secure random generation");
        key
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<String, PlatformError> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        self.rng.fill(&mut nonce_bytes).map_err(|e| PlatformError::Security {
            code: "RNG_FAIL".to_string(),
            message: e.to_string(),
            recovery_suggestion: "Check system entropy source".to_string(),
        })?;

        let unbound_key = UnboundKey::new(&AES_256_GCM, &self.key_bytes)
            .map_err(|_| PlatformError::Security {
                code: "INVALID_KEY".to_string(),
                message: "Invalid AES-256 key".to_string(),
                recovery_suggestion: "Provide a 32-byte secret key".to_string(),
            })?;

        let nonce = aead::Nonce::try_assume_unique_for_key(&nonce_bytes)
            .map_err(|_| PlatformError::Security {
                code: "NONCE_ERR".to_string(),
                message: "Failed to construct nonce".to_string(),
                recovery_suggestion: "Check nonce generation".to_string(),
            })?;

        let mut sealing_key = SealingKey::new(unbound_key, OneNonce(Some(nonce)));
        let mut in_out = plaintext.as_bytes().to_vec();

        sealing_key.seal_in_place_append_tag(aead::Aad::empty(), &mut in_out)
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
        let payload = BASE64.decode(ciphertext_b64).map_err(|e| PlatformError::Security {
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

        let unbound_key = UnboundKey::new(&AES_256_GCM, &self.key_bytes)
            .map_err(|_| PlatformError::Security {
                code: "INVALID_KEY".to_string(),
                message: "Invalid AES-256 key".to_string(),
                recovery_suggestion: "Provide a 32-byte secret key".to_string(),
            })?;

        let nonce = aead::Nonce::try_assume_unique_for_key(nonce_bytes)
            .map_err(|_| PlatformError::Security {
                code: "NONCE_ERR".to_string(),
                message: "Failed to construct nonce".to_string(),
                recovery_suggestion: "Check nonce generation".to_string(),
            })?;

        let mut opening_key = OpeningKey::new(unbound_key, OneNonce(Some(nonce)));
        let mut in_out = ciphertext.to_vec();

        let plaintext_bytes = opening_key.open_in_place(aead::Aad::empty(), &mut in_out)
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

    pub fn redact(&self, secret: &str) -> String {
        if secret.len() <= 8 {
            "********".to_string()
        } else {
            format!("{}...{}", &secret[..4], &secret[secret.len() - 4..])
        }
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

        assert_eq!(manager.redact("sk-openrouter-secret-key-12345"), "sk-o...2345");
        assert_eq!(manager.redact("short"), "********");
    }
}
