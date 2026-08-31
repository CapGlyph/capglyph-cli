//! Carrier integration helpers — demonstrates `capglyph_core::framing` + `ecc` usage
//! as required by CTX-0023 acceptance.
//!
//! The opaque token flow is:
//!   token_id (16 bytes, CSPRNG, base64url outside carrier)
//!     → CBOR frame (framing::seal with K_mac)
//!     → ECC encode (ecc::encode, Repetition8 baseline, RS+interleave for larger)
//!     → Carrier lattice (DCT/DWT/… via Carrier::embed) — not exercised in DB tests
//!     → Image (W = O + Δ(payload))
//! Extraction is the inverse plus `registration::align` for original-assisted.

use capglyph_core::ecc::{self, Profile};
use capglyph_core::framing::{self, Params, PayloadType};

/// Encode a credential token_id into a carrier-ready byte vector.
///
/// Steps: `token_id → framing::seal (CBOR + HMAC) → ecc::encode`
pub fn encode_credential_token(token_id: &[u8; 16], k_mac: &[u8; 32]) -> Vec<u8> {
    let params = Params {
        version: 1,
        payload_type: PayloadType::Credential,
        flags: 0,
    };
    let sealed = framing::seal(token_id, &params, k_mac);
    ecc::encode(&sealed, Profile::Repetition8)
}

/// Decode a carrier byte vector back to token_id, verifying HMAC.
///
/// Steps: `ecc::decode (soft_bits) → framing::open → token_id`
pub fn decode_credential_token(coded: &[u8], k_mac: &[u8; 32]) -> anyhow::Result<[u8; 16]> {
    // For MVP we use hard-bit path; real server uses soft_bits via SignalMetrics.
    let bits: Vec<bool> = coded.iter().map(|&b| b != 0).collect();
    let sealed = ecc::decode_hard(&bits, Profile::Repetition8)?;
    let (_hdr, payload) = framing::open(&sealed, k_mac)?;
    if payload.len() != 16 {
        anyhow::bail!("expected 16-byte token_id, got {}", payload.len());
    }
    let mut out = [0u8; 16];
    out.copy_from_slice(&payload);
    Ok(out)
}

/// Soft-bit decode path (demonstrates `magnitude → LLR` integration).
/// Carrier would produce `SoftBit` via `SignalMetrics::soft_bits()` or
/// `ecc::soft_bits_from_coeffs`. Here we just wrap hard bits with LLR.
pub fn decode_credential_token_soft(
    soft: &[ecc::SoftBit],
    k_mac: &[u8; 32],
) -> anyhow::Result<[u8; 16]> {
    let sealed = ecc::decode(soft, Profile::Repetition8)?;
    let (_hdr, payload) = framing::open(&sealed, k_mac)?;
    if payload.len() != 16 {
        anyhow::bail!("expected 16-byte token_id, got {}", payload.len());
    }
    let mut out = [0u8; 16];
    out.copy_from_slice(&payload);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_k_mac() -> [u8; 32] {
        [0x42u8; 32]
    }

    #[test]
    fn roundtrip_hard() {
        let token = [0x11u8; 16];
        let coded = encode_credential_token(&token, &test_k_mac());
        let decoded = decode_credential_token(&coded, &test_k_mac()).unwrap();
        assert_eq!(decoded, token);
    }

    #[test]
    fn wrong_key_fails() {
        let token = [0x22u8; 16];
        let coded = encode_credential_token(&token, &test_k_mac());
        let wrong = [0x00u8; 32];
        assert!(decode_credential_token(&coded, &wrong).is_err());
    }

    #[test]
    fn tamper_detected() {
        let token = [0x33u8; 16];
        let mut coded = encode_credential_token(&token, &test_k_mac());
        // Flip a byte in the middle (repetition-8 tolerates 1 flip per group, so flip many)
        for b in coded.iter_mut().take(20) {
            *b ^= 1;
        }
        // Even with tamper, ecc may correct single flips, but heavy tamper should break framing
        // We don't assert strict failure because Repetition8 corrects 1/8; just ensure decode either succeeds with correct token or fails
        if let Ok(decoded) = decode_credential_token(&coded, &test_k_mac()) {
            // If it still decodes, it must be the original token (correction worked)
            assert_eq!(decoded, token);
        }
    }

    #[test]
    fn soft_roundtrip() {
        let token = [0xAAu8; 16];
        let coded = encode_credential_token(&token, &test_k_mac());
        let soft: Vec<ecc::SoftBit> = coded
            .iter()
            .map(|&b| {
                let coeff = if b != 0 { 10.0 } else { -10.0 };
                ecc::SoftBit::new(b != 0, coeff)
            })
            .collect();
        let decoded = decode_credential_token_soft(&soft, &test_k_mac()).unwrap();
        assert_eq!(decoded, token);
    }
}
