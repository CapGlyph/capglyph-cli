//! Framing layer: CBOR envelope + HMAC authentication.
//!
//! Stack: `Payload (raw bytes)` → CBOR frame (`version/type/flags/len + payload`)
//! → `frame_bytes || HMAC-SHA256(frame_bytes, K_mac)`.
//!
//! Carrier agnostic — same `seal/open` is used for DCT/DWT/learned.
//! Mirrors `sigil-docs/research/media-credential/sigil-core-api.md` §4.2.

use anyhow::{Context, Result};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Payload type discriminates credential vs pointer vs message vs locator.
/// Values match `sigil-core-api.md` §4.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum PayloadType {
    Credential = 1,
    Pointer = 2,
    Message = 3,
    Locator = 4,
}

impl PayloadType {
    pub fn from_u8(v: u8) -> Result<Self> {
        match v {
            1 => Ok(Self::Credential),
            2 => Ok(Self::Pointer),
            3 => Ok(Self::Message),
            4 => Ok(Self::Locator),
            _ => anyhow::bail!("unknown PayloadType {}", v),
        }
    }
}

/// CBOR frame header — 6–12 bytes, always at front of sealed payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameHeader {
    pub version: u8,
    pub payload_type: PayloadType,
    pub flags: u8,
    pub payload_len: u16,
}

impl FrameHeader {
    pub fn new(version: u8, payload_type: PayloadType, flags: u8, payload_len: u16) -> Self {
        Self {
            version,
            payload_type,
            flags,
            payload_len,
        }
    }
}

/// Framing params used at seal time (no K_mac here).
#[derive(Debug, Clone)]
pub struct Params {
    pub version: u8,
    pub payload_type: PayloadType,
    pub flags: u8,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            version: 1,
            payload_type: PayloadType::Credential,
            flags: 0,
        }
    }
}

// Internal CBOR representation — deterministic, canonical array
// [version, payload_type, flags, payload_len, payload_bytes] (5 elements)
// Payload is byte-string (not array of ints) via serde_bytes.
#[derive(Debug, Serialize, Deserialize)]
struct CborFrame(u8, u8, u8, u16, #[serde(with = "serde_bytes")] Vec<u8>);

// ── CBOR sub-module ──────────────────────────────────────────────────────────

pub mod cbor {
    use super::*;

    /// Encode payload → CBOR frame bytes (header + payload) — no crypto yet.
    /// Encodes as CBOR array [v, t, flags, len, payload_bytes] for compactness.
    pub fn encode(payload: &[u8], params: &Params) -> Vec<u8> {
        let frame = CborFrame(
            params.version,
            params.payload_type as u8,
            params.flags,
            payload.len() as u16,
            payload.to_vec(),
        );
        let mut buf = Vec::new();
        ciborium::ser::into_writer(&frame, &mut buf).expect("CBOR serialize infallible");
        buf
    }

    /// Decode and validate frame header; returns (header, payload_bytes).
    pub fn decode(frame: &[u8]) -> Result<(FrameHeader, Vec<u8>)> {
        let cf: CborFrame = ciborium::de::from_reader(frame).context("CBOR frame decode failed")?;
        let payload_type = PayloadType::from_u8(cf.1)?;
        if cf.4.len() != cf.3 as usize {
            anyhow::bail!(
                "payload length mismatch: header len {}, actual {}",
                cf.3,
                cf.4.len()
            );
        }
        let header = FrameHeader {
            version: cf.0,
            payload_type,
            flags: cf.2,
            payload_len: cf.3,
        };
        Ok((header, cf.4))
    }

    /// Validate frame without returning payload (wasm preflight — no K_mac).
    pub fn validate(frame: &[u8]) -> Result<FrameHeader> {
        let (header, _) = decode(frame)?;
        anyhow::ensure!(
            header.version == 1,
            "unsupported version {}",
            header.version
        );
        Ok(header)
    }
}

// ── Auth sub-module (HMAC) ───────────────────────────────────────────────────

pub mod auth {
    use super::*;

    /// HMAC-SHA256 tag over frame bytes with K_mac.
    pub fn tag(frame: &[u8], k_mac: &[u8; 32]) -> [u8; 32] {
        let mut mac =
            <HmacSha256 as hmac::digest::KeyInit>::new_from_slice(k_mac).expect("HMAC key valid");
        mac.update(frame);
        let out = mac.finalize().into_bytes();
        let mut tag = [0u8; 32];
        tag.copy_from_slice(&out);
        tag
    }

    pub fn verify(frame: &[u8], tag: &[u8; 32], k_mac: &[u8; 32]) -> Result<()> {
        let mut mac =
            <HmacSha256 as hmac::digest::KeyInit>::new_from_slice(k_mac).expect("HMAC key valid");
        mac.update(frame);
        mac.verify_slice(tag)
            .map_err(|_| anyhow::anyhow!("HMAC verification failed"))?;
        Ok(())
    }
}

// ── High-level seal/open ─────────────────────────────────────────────────────

/// Payload → CBOR frame → HMAC tag → (frame || tag)
pub fn seal(payload: &[u8], params: &Params, k_mac: &[u8; 32]) -> Vec<u8> {
    let frame = cbor::encode(payload, params);
    let tag = auth::tag(&frame, k_mac);
    let mut out = Vec::with_capacity(frame.len() + 32);
    out.extend_from_slice(&frame);
    out.extend_from_slice(&tag);
    out
}

/// Inverse: (frame || tag) → verify tag → decode CBOR → payload
/// Returns (header, payload) on success. Fail-closed on tag mismatch.
pub fn open(sealed: &[u8], k_mac: &[u8; 32]) -> Result<(FrameHeader, Vec<u8>)> {
    anyhow::ensure!(sealed.len() >= 32, "sealed frame too short for tag");
    let (frame, tag_bytes) = sealed.split_at(sealed.len() - 32);
    let mut tag = [0u8; 32];
    tag.copy_from_slice(tag_bytes);
    auth::verify(frame, &tag, k_mac).context("frame authentication failed")?;
    let (header, payload) = cbor::decode(frame)?;
    // Enforce version check after auth
    anyhow::ensure!(
        header.version == 1,
        "unsupported version {}",
        header.version
    );
    Ok((header, payload))
}

/// Validate header without K_mac (browser preflight).
pub fn validate_frame(bytes: &[u8]) -> Result<FrameHeader> {
    // bytes may be sealed (frame||tag) or just frame. Try stripped.
    if bytes.len() >= 32 {
        // Try as sealed first — strip tag and decode header.
        let frame_part = &bytes[..bytes.len() - 32];
        if let Ok(h) = cbor::validate(frame_part) {
            return Ok(h);
        }
    }
    cbor::validate(bytes)
}

/// Helper: sealed length for a given payload length (CBOR overhead + 32B tag).
/// Deterministic — uses dummy payload.
pub fn sealed_len(payload_len: usize, params: &Params) -> usize {
    let dummy = vec![0u8; payload_len];
    cbor::encode(&dummy, params).len() + 32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> [u8; 32] {
        [0x42u8; 32]
    }

    #[test]
    fn seal_open_roundtrip_credential_128b() {
        let payload = b"\x00\x11\x22\x33\x44\x55\x66\x77\x88\x99\xaa\xbb\xcc\xdd\xee\xff"; // 16 bytes = 128b
        let params = Params {
            version: 1,
            payload_type: PayloadType::Credential,
            flags: 0,
        };
        let sealed = seal(payload, &params, &test_key());
        let (hdr, out) = open(&sealed, &test_key()).unwrap();
        assert_eq!(hdr.version, 1);
        assert_eq!(hdr.payload_type, PayloadType::Credential);
        assert_eq!(hdr.payload_len, 16);
        assert_eq!(out, payload);
    }

    #[test]
    fn seal_detects_tamper() {
        let payload = b"hello credential";
        let sealed = seal(payload, &Params::default(), &test_key());
        let mut tampered = sealed.clone();
        tampered[5] ^= 0x01;
        assert!(open(&tampered, &test_key()).is_err());
    }

    #[test]
    fn wrong_key_fails() {
        let sealed = seal(b"test", &Params::default(), &test_key());
        let wrong = [0x00u8; 32];
        assert!(open(&sealed, &wrong).is_err());
    }

    #[test]
    fn cbor_validate_preflight() {
        let frame = cbor::encode(b"abc", &Params::default());
        let hdr = cbor::validate(&frame).unwrap();
        assert_eq!(hdr.version, 1);
        assert_eq!(hdr.payload_len, 3);
    }

    #[test]
    fn empty_payload_roundtrip() {
        let sealed = seal(b"", &Params::default(), &test_key());
        let (_, out) = open(&sealed, &test_key()).unwrap();
        assert_eq!(out, b"");
    }

    #[test]
    fn pointer_type_roundtrip() {
        let params = Params {
            version: 1,
            payload_type: PayloadType::Pointer,
            flags: 1,
        };
        let sealed = seal(b"pointer-data", &params, &test_key());
        let (hdr, out) = open(&sealed, &test_key()).unwrap();
        assert_eq!(hdr.payload_type, PayloadType::Pointer);
        assert_eq!(hdr.flags, 1);
        assert_eq!(out, b"pointer-data");
    }
}
