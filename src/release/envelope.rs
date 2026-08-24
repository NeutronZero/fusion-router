use crate::release::signing::SignedAttestation;
use serde::{Deserialize, Serialize};

pub const ENVELOPE_VERSION: &str = "v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationEnvelope {
    pub envelope_version: String,
    pub signed_attestation: SignedAttestation,
}

impl AttestationEnvelope {
    pub fn new(signed_attestation: SignedAttestation) -> Self {
        Self {
            envelope_version: ENVELOPE_VERSION.to_string(),
            signed_attestation,
        }
    }
}
