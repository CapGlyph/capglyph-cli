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
