//! Pointer-mode stego: image → capability → encrypted object (CTX-0024).
//!
//! Three profiles per `pointer-and-stego.md` §1:
//! - `pointer-online` (default): 128-bit capability_id in carrier → server object lookup with authz (no IDOR)
//! - `pointer-offline`: object_id (128b) + content_key (256b) in carrier for direct decrypt, 1024px+ only
//! - `direct`: full AEAD ciphertext in carrier (not yet, deferred)
//!
//! This module shares the same `capglyph_core::{framing,ecc}` + `Carrier` stack as credential.
//! AEAD: ChaCha20-Poly1305 (RFC 8439) with 12-byte nonce, 16-byte tag, 32-byte key.
//! HPKE is deferred (single-primitive agility per `cryptographic-security.md` §5).

#![allow(deprecated)]
#![allow(clippy::too_many_arguments, clippy::type_complexity, clippy::needless_range_loop)]

use anyhow::{Context, Result};
use base64::Engine as _;
use chacha20poly1305::KeyInit as _;
use image::{ImageBuffer, Rgb};
use uuid::Uuid;

#[cfg(not(target_arch = "wasm32"))]
use rand::RngCore;

use crate::core::geometry::GeometryFile;
use crate::keying::KeyMaterial;

/// Minimum dimension for offline pointer (image must be at least 1024×1024).
pub const OFFLINE_MIN_DIMENSION: u32 = 1024;

/// Generate a fresh 32-byte content key (CSPRNG).
#[cfg(not(target_arch = "wasm32"))]
pub fn generate_content_key() -> [u8; 32] {
    let mut k = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut k);
    k
}

/// Generate a fresh 12-byte nonce.
#[cfg(not(target_arch = "wasm32"))]
pub fn generate_nonce() -> [u8; 12] {
    let mut n = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut n);
    n
}

/// Generate a fresh 16-byte capability_id.
#[cfg(not(target_arch = "wasm32"))]
pub fn generate_capability_id() -> [u8; 16] {
    let mut c = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut c);
    c
}

// ── AEAD helpers (ChaCha20-Poly1305) ────────────────────────────────────────

pub fn aead_encrypt(
    plaintext: &[u8],
    key: &[u8; 32],
    nonce_bytes: &[u8; 12],
) -> Result<(Vec<u8>, Vec<u8>)> {
    use chacha20poly1305::{aead::Aead, ChaCha20Poly1305, Key, Nonce};
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let nonce = Nonce::from_slice(nonce_bytes);
    let combined = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| anyhow::anyhow!("AEAD encrypt failed: {e}"))?;
    if combined.len() < 16 {
        anyhow::bail!("AEAD output too short");
    }
    let (ct, tag) = combined.split_at(combined.len() - 16);
    Ok((ct.to_vec(), tag.to_vec()))
}

pub fn aead_decrypt(
    ciphertext: &[u8],
    tag: &[u8],
    key: &[u8; 32],
    nonce_bytes: &[u8; 12],
) -> Result<Vec<u8>> {
    use chacha20poly1305::{aead::Aead, ChaCha20Poly1305, Key, Nonce};
    let mut combined = Vec::with_capacity(ciphertext.len() + tag.len());
    combined.extend_from_slice(ciphertext);
    combined.extend_from_slice(tag);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let nonce = Nonce::from_slice(nonce_bytes);
    let pt = cipher
        .decrypt(nonce, combined.as_ref())
        .map_err(|_| anyhow::anyhow!("AEAD tag verification failed"))?;
    Ok(pt)
}

// ── Payload helpers ─────────────────────────────────────────────────────────

/// Online payload: 16-byte capability_id.
pub fn payload_online(capability_id: &[u8; 16]) -> Vec<u8> {
    capability_id.to_vec()
}

/// Offline payload: UUID (16 bytes BE) || content_key (32 bytes) = 48 bytes.
/// UUID bytes are big-endian per RFC 4122 `as_bytes()`.
pub fn payload_offline(object_id: &Uuid, content_key: &[u8; 32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(48);
    out.extend_from_slice(object_id.as_bytes());
    out.extend_from_slice(content_key);
    out
}

/// Parse offline payload back to (object_id, content_key).
pub fn parse_payload_offline(payload: &[u8]) -> Result<(Uuid, [u8; 32])> {
    anyhow::ensure!(
        payload.len() == 48,
        "offline payload must be 48 bytes (object_id 16 + key 32), got {}",
        payload.len()
    );
    let (id_bytes, key_bytes) = payload.split_at(16);
    let id = Uuid::from_slice(id_bytes).context("invalid UUID bytes")?;
    let mut key = [0u8; 32];
    key.copy_from_slice(key_bytes);
    Ok((id, key))
}

// ── Carrier helpers ─────────────────────────────────────────────────────────

/// Choose ECC profile for pointer based on image size and mode.
/// - 512 DCT with Repetition8 insufficient → use Bch t=3
/// - 1024+ DCT → Repetition8 is most robust (soft LLR)
/// - DWT → Repetition8 (ample LH)
pub fn select_profile(w: u32, h: u32, mode: &crate::cli::EmbedMode) -> capglyph_core::ecc::Profile {
    use capglyph_core::ecc::Profile;
    match mode {
        crate::cli::EmbedMode::Dct => {
            if w < 1024 || h < 1024 {
                Profile::Bch { t: 3 }
            } else {
                Profile::Repetition8
            }
        }
        crate::cli::EmbedMode::Dwt => Profile::Repetition8,
        _ => Profile::Bch { t: 3 },
    }
}

/// Framing params for pointer-online (capability_id).
fn params_online() -> capglyph_core::framing::Params {
    capglyph_core::framing::Params {
        version: 1,
        payload_type: capglyph_core::framing::PayloadType::Pointer,
        flags: 0,
    }
}

/// Framing params for pointer-offline (object_id+content_key).
fn params_offline() -> capglyph_core::framing::Params {
    capglyph_core::framing::Params {
        version: 1,
        payload_type: capglyph_core::framing::PayloadType::Locator,
        flags: 0,
    }
}

/// Embed online capability (16b) into image.
/// Returns (marked_blocks, positions).
pub fn embed_online(
    img: &mut ImageBuffer<Rgb<u8>, Vec<u8>>,
    geometry: &GeometryFile,
    capability_id: &[u8; 16],
    keys: &KeyMaterial,
    placement: &crate::cli::PlacementStrategy,
    profile: capglyph_core::ecc::Profile,
    mode: crate::cli::EmbedMode,
) -> Result<(u64, Vec<(u32, u32)>)> {
    let payload = payload_online(capability_id);
    let params = params_online();
    let sealed = capglyph_core::framing::seal(&payload, &params, keys.k_mac());
    let coded = capglyph_core::ecc::encode(&sealed, profile);
    let coded_bits: Vec<bool> = match profile {
        capglyph_core::ecc::Profile::Repetition8 | capglyph_core::ecc::Profile::Bch { .. } => {
            coded.iter().map(|&b| b != 0).collect()
        }
        capglyph_core::ecc::Profile::RsInterleaved { .. } => {
            capglyph_core::ecc::bytes_to_bits(&coded)
        }
    };
    match mode {
        crate::cli::EmbedMode::Dct => {
            crate::dct::embed_coded_bits(img, geometry, &coded_bits, keys, placement)
        }
        crate::cli::EmbedMode::Dwt => {
            crate::dwt_embed::embed_coded_bits(img, geometry, &coded_bits, keys, placement)
        }
        _ => anyhow::bail!("pointer embed only supports dct/dwt modes"),
    }
}

/// Extract online capability from image.
pub fn extract_online(
    img: &ImageBuffer<Rgb<u8>, Vec<u8>>,
    keys: &KeyMaterial,
    profile: capglyph_core::ecc::Profile,
    mode: crate::cli::EmbedMode,
) -> Result<[u8; 16]> {
    let payload = extract_payload(
        img,
        keys,
        profile,
        mode,
        16,
        capglyph_core::framing::PayloadType::Pointer,
    )?;
    let mut out = [0u8; 16];
    out.copy_from_slice(&payload);
    Ok(out)
}

/// Embed offline payload (48b) — enforces 1024px+ check.
#[allow(clippy::too_many_arguments)]
pub fn embed_offline(
    img: &mut ImageBuffer<Rgb<u8>, Vec<u8>>,
    geometry: &GeometryFile,
    object_id: &Uuid,
    content_key: &[u8; 32],
    keys: &KeyMaterial,
    placement: &crate::cli::PlacementStrategy,
    profile: capglyph_core::ecc::Profile,
    mode: crate::cli::EmbedMode,
) -> Result<(u64, Vec<(u32, u32)>)> {
    let (w, h) = img.dimensions();
    anyhow::ensure!(
        w >= OFFLINE_MIN_DIMENSION && h >= OFFLINE_MIN_DIMENSION,
        "offline pointer requires image at least {}x{} (got {}x{}): payload 48 bytes needs 1024+ carrier",
        OFFLINE_MIN_DIMENSION,
        OFFLINE_MIN_DIMENSION,
        w,
        h
    );
    let payload = payload_offline(object_id, content_key);
    let params = params_offline();
    let sealed = capglyph_core::framing::seal(&payload, &params, keys.k_mac());
    let coded = capglyph_core::ecc::encode(&sealed, profile);
    let coded_bits: Vec<bool> = match profile {
        capglyph_core::ecc::Profile::Repetition8 | capglyph_core::ecc::Profile::Bch { .. } => {
            coded.iter().map(|&b| b != 0).collect()
        }
        capglyph_core::ecc::Profile::RsInterleaved { .. } => {
            capglyph_core::ecc::bytes_to_bits(&coded)
        }
    };
    match mode {
        crate::cli::EmbedMode::Dct => {
            crate::dct::embed_coded_bits(img, geometry, &coded_bits, keys, placement)
        }
        crate::cli::EmbedMode::Dwt => {
            crate::dwt_embed::embed_coded_bits(img, geometry, &coded_bits, keys, placement)
        }
        _ => anyhow::bail!("pointer offline only supports dct/dwt"),
    }
}

/// Extract offline payload.
pub fn extract_offline(
    img: &ImageBuffer<Rgb<u8>, Vec<u8>>,
    keys: &KeyMaterial,
    profile: capglyph_core::ecc::Profile,
    mode: crate::cli::EmbedMode,
) -> Result<(Uuid, [u8; 32])> {
    let payload = extract_payload(
        img,
        keys,
        profile,
        mode,
        48,
        capglyph_core::framing::PayloadType::Locator,
    )?;
    let (id, key) = parse_payload_offline(&payload)?;
    Ok((id, key))
}

// ── Generic payload extract ─────────────────────────────────────────────────

fn extract_payload(
    img: &ImageBuffer<Rgb<u8>, Vec<u8>>,
    keys: &KeyMaterial,
    profile: capglyph_core::ecc::Profile,
    mode: crate::cli::EmbedMode,
    expected_len: usize,
    expected_type: capglyph_core::framing::PayloadType,
) -> Result<Vec<u8>> {
    let params = capglyph_core::framing::Params {
        version: 1,
        payload_type: expected_type,
        flags: 0,
    };
    let sealed_len = capglyph_core::framing::sealed_len(expected_len, &params);
    let need_bits = capglyph_core::ecc::coded_bits_len(sealed_len, profile);
    // Extract soft bits via appropriate carrier
    let soft: Vec<capglyph_core::ecc::SoftBit> = match mode {
        crate::cli::EmbedMode::Dct => {
            crate::dct::extract_coded_bits_soft_with_hint(img, keys, Some(need_bits))?
        }
        crate::cli::EmbedMode::Dwt => {
            crate::dwt_embed::extract_coded_bits_soft_with_hint(img, keys, Some(need_bits))?
        }
        _ => anyhow::bail!("pointer extract only supports dct/dwt"),
    };
    let mut decoded_sealed = capglyph_core::ecc::decode(&soft, profile)?;
    if decoded_sealed.len() > sealed_len {
        decoded_sealed.truncate(sealed_len);
    }
    let (hdr, payload) = capglyph_core::framing::open(&decoded_sealed, keys.k_mac())?;
    anyhow::ensure!(
        hdr.payload_type == expected_type,
        "unexpected payload type: got {:?}, expected {:?}",
        hdr.payload_type,
        expected_type
    );
    anyhow::ensure!(
        payload.len() == expected_len,
        "payload length mismatch: got {}, expected {}",
        payload.len(),
        expected_len
    );
    Ok(payload)
}

// ── High-level image helpers (for tests/CLI) ────────────────────────────────

/// Create a synthetic test image (like `framed.rs` make_image).
pub fn make_test_image(w: u32, h: u32) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    ImageBuffer::from_fn(w, h, |x, y| {
        let v = ((x * 7 + y * 13 + x * y / 3) % 251) as u8;
        Rgb([v, v.wrapping_add(60), v.wrapping_add(120)])
    })
}

pub fn make_test_geometry(w: u32, h: u32) -> GeometryFile {
    use crate::core::geometry::{AnalysisParams, PathEntry};
    let points: Vec<[f64; 2]> = (0..64)
        .map(|i| [i as f64 * (w as f64 / 64.0), (i as f64 * 3.0) % h as f64])
        .collect();
    GeometryFile {
        version: 1,
        original_width: w,
        original_height: h,
        analysis_params: AnalysisParams {
            detail: 60,
            min_path_len: 5,
            chaikin_iters: 3,
            color: false,
        },
        paths: vec![PathEntry {
            color: None,
            points,
        }],
        prng_seed: None,
        blocks: None,
    }
}

pub fn test_keys() -> KeyMaterial {
    KeyMaterial::from_keys([0x11u8; 32], [0x22u8; 32])
}

/// Capability helpers for carrier (base64url).
pub fn capability_to_base64url(cap: &[u8; 16]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(cap)
}

pub fn capability_from_base64url(s: &str) -> Result<[u8; 16]> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(s.trim())
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(s.trim()))
        .or_else(|_| hex::decode(s.trim()).map_err(|e| anyhow::anyhow!("{e}")))
        .context("invalid capability_id encoding")?;
    anyhow::ensure!(bytes.len() == 16, "capability must be 16 bytes");
    let mut out = [0u8; 16];
    out.copy_from_slice(&bytes);
    Ok(out)
}

// ── CLI helpers (not for wasm) ────────────────────────────────────────────

#[cfg(not(target_arch = "wasm32"))]
pub fn db_from_path(path: Option<&std::path::Path>) -> Result<capglyph_server::db::Db> {
    if let Some(p) = path {
        capglyph_server::db::Db::new(p).context("open DB")
    } else {
        // Default: ./capglyph.db in current dir, or temp if not writable
        let default = std::path::Path::new("capglyph.db");
        capglyph_server::db::Db::new(default).context("open default ./capglyph.db")
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read_plaintext(args_pt: Option<&str>, args_file: Option<&std::path::Path>) -> Result<Vec<u8>> {
    if let Some(f) = args_file {
        std::fs::read(f).with_context(|| format!("read plaintext file {:?}", f))
    } else if let Some(s) = args_pt {
        Ok(s.as_bytes().to_vec())
    } else {
        anyhow::bail!("provide --plaintext <text> or --plaintext-file <path>")
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_owner(s: Option<&str>) -> Result<Option<Uuid>> {
    match s {
        Some(v) => Ok(Some(Uuid::parse_str(v).context("invalid owner_id UUID")?)),
        None => Ok(None),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_policy(s: Option<&str>) -> Result<serde_json::Value> {
    match s {
        Some(v) => serde_json::from_str(v).context("invalid policy JSON"),
        None => Ok(serde_json::json!({})),
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::type_complexity)]
fn load_image_and_geometry(
    input: &std::path::Path,
) -> Result<(ImageBuffer<Rgb<u8>, Vec<u8>>, GeometryFile, u32, u32)> {
    let dyn_img = image::open(input).with_context(|| format!("open image {:?}", input))?;
    let rgb = dyn_img.to_rgb8();
    let (w, h) = rgb.dimensions();
    let geo = crate::embed::extract_and_build_geometry(
        &rgb,
        w,
        h,
        &crate::embed::GeometryParams {
            detail: 60,
            min_path_len: 5,
            chaikin_iters: 3,
            color: false,
            recipient_id: None,
        },
    )?;
    Ok((rgb, geo, w, h))
}

#[cfg(not(target_arch = "wasm32"))]
fn keys_from_opt(key: Option<&str>) -> KeyMaterial {
    if let Some(k) = key {
        KeyMaterial::from_ikm(k, &[0u8; 16])
    } else {
        // Deterministic default for tests / CLI without key
        KeyMaterial::from_keys([0x11u8; 32], [0x22u8; 32])
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn run_pointer_embed(args: &crate::cli::PointerEmbedArgs) -> Result<()> {
    let pt = read_plaintext(args.plaintext.as_deref(), args.plaintext_file.as_deref())?;
    let db = db_from_path(args.db.as_deref())?;
    let svc = capglyph_server::service::Service::new_with_random_kms(db);
    let owner = parse_owner(args.owner_id.as_deref())?;
    let policy = parse_policy(args.policy.as_deref())?;
    // Encrypt and store server-side, get capability
    let (resp, _key, _nonce) = svc
        .encrypt_and_store(&pt, policy, owner, None)
        .context("store message")?;
    let cap = capability_from_base64url(&resp.capability_id)?;
    // Load cover image and geometry
    let (mut rgb, geo, w, h) = load_image_and_geometry(&args.input)?;
    let keys = keys_from_opt(args.key.as_deref());
    let profile = select_profile(w, h, &args.mode);
    let (_, _) = embed_online(
        &mut rgb,
        &geo,
        &cap,
        &keys,
        &args.placement,
        profile,
        args.mode,
    )?;
    // Save stego image
    let output = args.output.clone().unwrap_or_else(|| {
        let stem = args.input.file_stem().unwrap().to_string_lossy();
        let parent = args
            .input
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        parent.join(format!("{}_pointer.png", stem))
    });
    // Preserve alpha? For now save as RGB
    let dyn_out = image::DynamicImage::ImageRgb8(rgb);
    dyn_out
        .save(&output)
        .with_context(|| format!("save stego {:?}", output))?;
    println!(
        "pointer embed: capability {} → {:?}",
        resp.capability_id, output
    );
    println!("object_id: {}", resp.object_id);
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn run_pointer_extract(args: &crate::cli::PointerExtractArgs) -> Result<()> {
    let db = db_from_path(args.db.as_deref())?;
    let svc = capglyph_server::service::Service::new_with_random_kms(db);
    let actor = parse_owner(args.actor_id.as_deref())?;
    // Load stego image
    let dyn_img = image::open(&args.input).with_context(|| format!("open {:?}", args.input))?;
    let rgb = dyn_img.to_rgb8();
    let (w, h) = rgb.dimensions();
    let keys = keys_from_opt(args.key.as_deref());
    let profile = select_profile(w, h, &args.mode);
    let cap = extract_online(&rgb, &keys, profile, args.mode)?;
    let cap_b64 = capability_to_base64url(&cap);
    // Resolve with authz
    let pt = svc
        .resolve_and_decrypt(&cap_b64, actor, None)
        .context("resolve/decrypt")?;
    if let Some(out) = &args.output {
        std::fs::write(out, &pt).with_context(|| format!("write {:?}", out))?;
        println!("pointer extract: decrypted {} bytes → {:?}", pt.len(), out);
    } else {
        // Try to print as utf8, else base64
        match String::from_utf8(pt.clone()) {
            Ok(s) => println!("{}", s),
            Err(_) => println!("{}", base64::engine::general_purpose::STANDARD.encode(&pt)),
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn run_pointer_offline_embed(args: &crate::cli::PointerOfflineEmbedArgs) -> Result<()> {
    let pt = read_plaintext(args.plaintext.as_deref(), args.plaintext_file.as_deref())?;
    let db = db_from_path(args.db.as_deref())?;
    let svc = capglyph_server::service::Service::new_with_random_kms(db);
    let owner = parse_owner(args.owner_id.as_deref())?;
    let policy = parse_policy(args.policy.as_deref())?;
    // Store offline: get object_id + key
    let (object_id, content_key, _nonce, _tag) = svc
        .store_offline(&pt, policy, owner, None)
        .context("store offline")?;
    let mut key_arr = [0u8; 32];
    key_arr.copy_from_slice(&content_key);
    // Load image
    let (mut rgb, geo, w, h) = load_image_and_geometry(&args.input)?;
    let keys = keys_from_opt(args.key.as_deref());
    let profile = select_profile(w, h, &args.mode);
    embed_offline(
        &mut rgb,
        &geo,
        &object_id,
        &key_arr,
        &keys,
        &args.placement,
        profile,
        args.mode,
    )?;
    let output = args.output.clone().unwrap_or_else(|| {
        let stem = args.input.file_stem().unwrap().to_string_lossy();
        let parent = args
            .input
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        parent.join(format!("{}_offline.png", stem))
    });
    image::DynamicImage::ImageRgb8(rgb)
        .save(&output)
        .with_context(|| format!("save {:?}", output))?;
    println!("offline embed: object {} → {:?}", object_id, output);
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn run_pointer_offline_extract(args: &crate::cli::PointerOfflineExtractArgs) -> Result<()> {
    let db = db_from_path(args.db.as_deref())?;
    let svc = capglyph_server::service::Service::new_with_random_kms(db);
    let actor = parse_owner(args.actor_id.as_deref())?;
    let dyn_img = image::open(&args.input).with_context(|| format!("open {:?}", args.input))?;
    let rgb = dyn_img.to_rgb8();
    let (w, h) = rgb.dimensions();
    let keys = keys_from_opt(args.key.as_deref());
    let profile = select_profile(w, h, &args.mode);
    let (object_id, content_key) = extract_offline(&rgb, &keys, profile, args.mode)?;
    let pt = svc
        .resolve_offline(&object_id, &content_key, actor)
        .context("offline resolve")?;
    if let Some(out) = &args.output {
        std::fs::write(out, &pt).with_context(|| format!("write {:?}", out))?;
        println!("offline extract: {} bytes → {:?}", pt.len(), out);
    } else {
        match String::from_utf8(pt.clone()) {
            Ok(s) => println!("{}", s),
            Err(_) => println!("{}", base64::engine::general_purpose::STANDARD.encode(&pt)),
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn run_message_store(args: &crate::cli::MessageStoreArgs) -> Result<()> {
    let pt = read_plaintext(args.plaintext.as_deref(), args.plaintext_file.as_deref())?;
    let db = db_from_path(args.db.as_deref())?;
    let svc = capglyph_server::service::Service::new_with_random_kms(db);
    let owner = parse_owner(args.owner_id.as_deref())?;
    let policy = parse_policy(args.policy.as_deref())?;
    let (resp, _, _) = svc.encrypt_and_store(&pt, policy, owner, None)?;
    println!("{}", resp.capability_id);
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn run_message_resolve(args: &crate::cli::MessageResolveArgs) -> Result<()> {
    let db = db_from_path(args.db.as_deref())?;
    let svc = capglyph_server::service::Service::new_with_random_kms(db);
    let actor = parse_owner(args.actor_id.as_deref())?;
    let pt = svc.resolve_and_decrypt(&args.capability_id, actor, None)?;
    if let Some(out) = &args.output {
        std::fs::write(out, &pt).with_context(|| format!("write {:?}", out))?;
        println!("resolved {} bytes → {:?}", pt.len(), out);
    } else {
        match String::from_utf8(pt.clone()) {
            Ok(s) => println!("{}", s),
            Err(_) => println!("{}", base64::engine::general_purpose::STANDARD.encode(&pt)),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aead_roundtrip() {
        let key = [0x42u8; 32];
        let nonce = [0x11u8; 12];
        let pt = b"hello pointer mode";
        let (ct, tag) = aead_encrypt(pt, &key, &nonce).unwrap();
        let pt2 = aead_decrypt(&ct, &tag, &key, &nonce).unwrap();
        assert_eq!(pt.to_vec(), pt2);
    }

    #[test]
    fn aead_tamper_fails() {
        let key = [0x42u8; 32];
        let nonce = [0x11u8; 12];
        let pt = b"secret message";
        let (mut ct, tag) = aead_encrypt(pt, &key, &nonce).unwrap();
        ct[0] ^= 1;
        assert!(aead_decrypt(&ct, &tag, &key, &nonce).is_err());
    }

    #[test]
    fn payload_offline_roundtrip() {
        let id = Uuid::new_v4();
        let key = [0xAAu8; 32];
        let payload = payload_offline(&id, &key);
        assert_eq!(payload.len(), 48);
        let (id2, key2) = parse_payload_offline(&payload).unwrap();
        assert_eq!(id, id2);
        assert_eq!(key, key2);
    }

    #[test]
    fn embed_extract_online_dct_1024() {
        let (w, h) = (1024, 1024);
        let mut img = make_test_image(w, h);
        let geo = make_test_geometry(w, h);
        let keys = test_keys();
        let cap = [0xAB; 16];
        let profile = select_profile(w, h, &crate::cli::EmbedMode::Dct);
        embed_online(
            &mut img,
            &geo,
            &cap,
            &keys,
            &crate::cli::PlacementStrategy::Skeleton,
            profile,
            crate::cli::EmbedMode::Dct,
        )
        .unwrap();
        let out = extract_online(&img, &keys, profile, crate::cli::EmbedMode::Dct).unwrap();
        assert_eq!(cap, out);
    }

    #[test]
    fn embed_extract_online_dwt_512() {
        let (w, h) = (512, 512);
        let mut img = make_test_image(w, h);
        let geo = make_test_geometry(w, h);
        let keys = test_keys();
        let cap = [0xCD; 16];
        let profile = select_profile(w, h, &crate::cli::EmbedMode::Dwt);
        embed_online(
            &mut img,
            &geo,
            &cap,
            &keys,
            &crate::cli::PlacementStrategy::Skeleton,
            profile,
            crate::cli::EmbedMode::Dwt,
        )
        .unwrap();
        let out = extract_online(&img, &keys, profile, crate::cli::EmbedMode::Dwt).unwrap();
        assert_eq!(cap, out);
    }

    #[test]
    fn offline_requires_1024() {
        let (w, h) = (512, 512);
        let mut img = make_test_image(w, h);
        let geo = make_test_geometry(w, h);
        let keys = test_keys();
        let id = Uuid::new_v4();
        let key = [0x11u8; 32];
        let profile = select_profile(w, h, &crate::cli::EmbedMode::Dct);
        let res = embed_offline(
            &mut img,
            &geo,
            &id,
            &key,
            &keys,
            &crate::cli::PlacementStrategy::Skeleton,
            profile,
            crate::cli::EmbedMode::Dct,
        );
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("1024"));
    }

    #[test]
    fn embed_extract_offline_1024() {
        let (w, h) = (1024, 1024);
        let mut img = make_test_image(w, h);
        let geo = make_test_geometry(w, h);
        let keys = test_keys();
        let id = Uuid::new_v4();
        let ck = [0x55u8; 32];
        let profile = select_profile(w, h, &crate::cli::EmbedMode::Dwt);
        embed_offline(
            &mut img,
            &geo,
            &id,
            &ck,
            &keys,
            &crate::cli::PlacementStrategy::Skeleton,
            profile,
            crate::cli::EmbedMode::Dwt,
        )
        .unwrap();
        let (id2, ck2) = extract_offline(&img, &keys, profile, crate::cli::EmbedMode::Dwt).unwrap();
        assert_eq!(id, id2);
        assert_eq!(ck, ck2);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn end_to_end_pointer_online() {
        use capglyph_server::{db::Db, service::Service};
        let db = Db::new_in_memory().unwrap();
        let svc = Service::new_with_random_kms(db);
        let owner = Uuid::new_v4();
        let actor_ok = owner;
        let actor_bad = Uuid::new_v4();
        let plaintext = b"hello pointer online - secret message 123";

        // Encrypt + store (server)
        let (resp, key, nonce) = svc
            .encrypt_and_store(plaintext, serde_json::json!({}), Some(owner), None)
            .unwrap();
        let cap_b64 = resp.capability_id.clone();

        // Embed capability into image
        let (w, h) = (1024, 1024);
        let mut img = make_test_image(w, h);
        let geo = make_test_geometry(w, h);
        let keys = test_keys();
        let cap = capability_from_base64url(&cap_b64).unwrap();
        let profile = select_profile(w, h, &crate::cli::EmbedMode::Dct);
        embed_online(
            &mut img,
            &geo,
            &cap,
            &keys,
            &crate::cli::PlacementStrategy::Skeleton,
            profile,
            crate::cli::EmbedMode::Dct,
        )
        .unwrap();

        // Extract capability
        let cap2 = extract_online(&img, &keys, profile, crate::cli::EmbedMode::Dct).unwrap();
        assert_eq!(cap, cap2);
        let cap2_b64 = capability_to_base64url(&cap2);

        // Resolve with correct actor → decrypt succeeds
        let mut key_arr = [0u8; 32];
        key_arr.copy_from_slice(&key);
        let mut nonce_arr = [0u8; 12];
        nonce_arr.copy_from_slice(&nonce);
        // Use resolve_and_decrypt which fetches stored key internally
        let pt = svc
            .resolve_and_decrypt(&cap2_b64, Some(actor_ok), None)
            .unwrap();
        assert_eq!(pt, plaintext);

        // IDOR: wrong actor should fail when owner-restricted
        // Re-store with owner policy, try wrong actor
        let (resp2, _, _) = svc
            .encrypt_and_store(b"another secret", serde_json::json!({}), Some(owner), None)
            .unwrap();
        let err = svc
            .resolve_and_decrypt(&resp2.capability_id, Some(actor_bad), None)
            .unwrap_err();
        // Should be Unauthorized (IDOR prevented)
        match err {
            capglyph_server::error::ServerError::Unauthorized(_) => {}
            other => panic!("expected Unauthorized, got {:?}", other),
        }

        // Bearer: no owner → any actor can resolve (capability is bearer)
        let (resp3, _, _) = svc
            .encrypt_and_store(b"bearer secret", serde_json::json!({}), None, None)
            .unwrap();
        let pt3 = svc
            .resolve_and_decrypt(&resp3.capability_id, Some(actor_bad), None)
            .unwrap();
        assert_eq!(pt3, b"bearer secret");

        // AEAD tag tamper: fetch object, tamper ciphertext, decrypt should fail
        let obj = svc
            .db
            .get_message_object_by_capability_id(&cap)
            .unwrap()
            .unwrap();
        let mut tampered_ct = obj.ciphertext.clone();
        if !tampered_ct.is_empty() {
            tampered_ct[0] ^= 1;
        }
        let mut nonce_arr2 = [0u8; 12];
        nonce_arr2.copy_from_slice(&obj.nonce);
        let mut key_arr2 = [0u8; 32];
        key_arr2.copy_from_slice(obj.content_key.as_ref().unwrap());
        let tamper_res = Service::aead_decrypt(&tampered_ct, &obj.tag, &key_arr2, &nonce_arr2);
        assert!(
            tamper_res.is_err(),
            "tampered ciphertext should fail AEAD verify"
        );
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn end_to_end_offline_1024() {
        use capglyph_server::{db::Db, service::Service};
        let db = Db::new_in_memory().unwrap();
        let svc = Service::new_with_random_kms(db);
        let owner = Uuid::new_v4();
        let plaintext = b"offline secret: direct decrypt without capability lookup";

        // Store offline: get object_id + content_key
        let (object_id, content_key, _nonce, _tag) = svc
            .store_offline(plaintext, serde_json::json!({}), Some(owner), None)
            .unwrap();
        let mut key_arr = [0u8; 32];
        key_arr.copy_from_slice(&content_key);

        // Embed offline payload (object_id+key) into 1024 image
        let (w, h) = (1024, 1024);
        let mut img = make_test_image(w, h);
        let geo = make_test_geometry(w, h);
        let keys = test_keys();
        let profile = select_profile(w, h, &crate::cli::EmbedMode::Dwt);
        embed_offline(
            &mut img,
            &geo,
            &object_id,
            &key_arr,
            &keys,
            &crate::cli::PlacementStrategy::Skeleton,
            profile,
            crate::cli::EmbedMode::Dwt,
        )
        .unwrap();

        // Extract offline payload
        let (oid2, key2) =
            extract_offline(&img, &keys, profile, crate::cli::EmbedMode::Dwt).unwrap();
        assert_eq!(object_id, oid2);
        assert_eq!(key_arr, key2);

        // Resolve offline with correct actor
        let pt = svc.resolve_offline(&oid2, &key2, Some(owner)).unwrap();
        assert_eq!(pt, plaintext);

        // IDOR for offline: wrong actor fails
        let bad_actor = Uuid::new_v4();
        let err = svc
            .resolve_offline(&oid2, &key2, Some(bad_actor))
            .unwrap_err();
        match err {
            capglyph_server::error::ServerError::Unauthorized(_) => {}
            other => panic!("expected Unauthorized for offline IDOR, got {:?}", other),
        }
    }
}
