//! Key-derived secret watermark layer.
//!
//! The secret layer adds a second, key-located signal on top of the public
//! watermark. Positions are derived from HMAC-SHA256(secret_key, image_hash),
//! so:
//!
//! - **Verification** requires the key: without it, the marked coefficient
//!   positions are indistinguishable from noise (the search space is the full
//!   block/band grid).
//! - **Forgery** is prevented: an attacker without the key cannot produce an
//!   image whose key-derived positions carry the expected signal.
//! - **Parameter learning** is prevented: each image derives different
//!   positions (image_hash mixes in), so a diff attack on one image leaks no
//!   information about where the secret layer sits in other images.

use digest::KeyInit;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Domain-separated key material for framing + placement.
/// Never log, never send to wasm. k_embed seeds placement PRNG,
/// k_mac authenticates frames, k_object optionally encrypts pointer objects.
#[derive(Clone, Debug)]
pub struct KeyMaterial {
    /// K_embed: PRNG seed for secret-layer positions and adaptive placement.
    k_embed: [u8; 32],
    /// K_mac: HMAC key for framing tag (and later Ed25519 seed / KMS handle).
    k_mac: [u8; 32],
    /// Optional K_object: AEAD key for pointer-mode ciphertext.
    k_object: Option<[u8; 32]>,
}

impl KeyMaterial {
    /// Derive from a single IKM string (CLI --key path) for backwards compat.
    /// Uses HKDF-like HMAC expansion with context separation.
    pub fn from_ikm(ikm: &str, cover_id: &[u8; 16]) -> Self {
        Self {
            k_embed: Self::derive(ikm.as_bytes(), b"sigil-k-embed-v1", cover_id),
            k_mac: Self::derive(ikm.as_bytes(), b"sigil-k-mac-v1", cover_id),
            k_object: None,
        }
    }

    /// Derive directly from explicit keys (for tests / KMS integration).
    pub fn from_keys(k_embed: [u8; 32], k_mac: [u8; 32]) -> Self {
        Self {
            k_embed,
            k_mac,
            k_object: None,
        }
    }

    /// Derive with explicit K_object (pointer mode).
    pub fn from_keys_with_object(k_embed: [u8; 32], k_mac: [u8; 32], k_object: [u8; 32]) -> Self {
        Self {
            k_embed,
            k_mac,
            k_object: Some(k_object),
        }
    }

    fn derive(ikm: &[u8], context: &[u8], cover_id: &[u8; 16]) -> [u8; 32] {
        let mut mac = <HmacSha256 as KeyInit>::new_from_slice(ikm).expect("HMAC key valid");
        mac.update(context);
        mac.update(cover_id);
        let out = mac.finalize().into_bytes();
        let mut key = [0u8; 32];
        key.copy_from_slice(&out);
        key
    }

    pub fn k_embed(&self) -> &[u8; 32] {
        &self.k_embed
    }
    pub fn k_mac(&self) -> &[u8; 32] {
        &self.k_mac
    }
    pub fn k_object(&self) -> Option<&[u8; 32]> {
        self.k_object.as_ref()
    }
}

/// PRF for placement: k_embed mixed with image hash → u64 seed (keyed placement).
pub fn prf_k_embed(k_embed: &[u8; 32], image_hash: u64) -> u64 {
    let mut mac = <HmacSha256 as KeyInit>::new_from_slice(k_embed).expect("HMAC key valid");
    mac.update(b"sigil-k-embed-prf-v1");
    mac.update(&image_hash.to_le_bytes());
    let out = mac.finalize().into_bytes();
    u64::from_le_bytes(out[..8].try_into().expect("32-byte digest"))
}

/// Tag helper: HMAC frame with k_mac (domain-separated).
pub fn prf_k_mac_tag(k_mac: &[u8; 32], frame: &[u8]) -> [u8; 32] {
    let mut mac = <HmacSha256 as KeyInit>::new_from_slice(k_mac).expect("HMAC key valid");
    mac.update(b"sigil-k-mac-tag-v1");
    mac.update(frame);
    let out = mac.finalize().into_bytes();
    let mut tag = [0u8; 32];
    tag.copy_from_slice(&out);
    tag
}

/// Derive the u64 key-seed for an image.
///
/// `image_hash` should be a content-derived seed (e.g. `dct::stable_seed`)
/// so that the same image + key always yields the same positions, while
/// different images diverge even under the same key.
pub fn key_seed(secret_key: &str, image_hash: u64) -> u64 {
    let mut mac = <HmacSha256 as KeyInit>::new_from_slice(secret_key.as_bytes())
        .expect("HMAC accepts any key");
    mac.update(b"sigil-secret-layer-v1");
    mac.update(&image_hash.to_le_bytes());
    let digest = mac.finalize().into_bytes();
    u64::from_le_bytes(digest[..8].try_into().expect("32-byte digest"))
}

/// Derive a 32-byte keystream for encrypting learned-mode payload bits.
///
/// `context` separates this derivation from `key_seed` (different domain
/// strings). The stream is XORed with the recipient-id bitstring so that
/// the payload is pseudorandom without the key (ID privacy + forgery
/// resistance), and recoverable with it.
pub fn keystream_bytes(secret_key: &str, context: &str, image_hash: u64) -> [u8; 32] {
    let mut mac = <HmacSha256 as KeyInit>::new_from_slice(secret_key.as_bytes())
        .expect("HMAC accepts any key");
    mac.update(b"sigil-learned-keystream-v1");
    mac.update(context.as_bytes());
    mac.update(&image_hash.to_le_bytes());
    let digest = mac.finalize().into_bytes();
    digest[..32].try_into().expect("32-byte digest")
}
