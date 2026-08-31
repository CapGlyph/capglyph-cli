//! Carrier trait — abstracts embed/verify/extract + metrics for each watermark mode.
//!
//! Prepares mechanical extraction of `sigil-core` (DEC-0003 Phase 1). The trait
//! is intentionally object-safe-ish but used as a static dispatch via associated
//! types; `embed.rs` dispatches through `DctCarrier`/`DwtCarrier` impls so the
//! call sites are ready to become `sigil_core::Carrier` after the crate split.
//!
//! No behavior change — each impl delegates to the existing `crate::dct`,
//! `crate::dwt_embed`, and `crate::extract` primitives.

use anyhow::{Context, Result};
use image::{ImageBuffer, Rgb};

use crate::cli::PlacementStrategy;
use crate::geometry::GeometryFile;
use crate::registration::{CoverVault, HybridMatch, Registration};

/// Unified interface for a watermark carrier (frequency-domain or spatial).
///
/// Each carrier operates on an RGB image buffer and optional geometry. The
/// `Metrics` associated type carries the verification signal for that carrier.
pub trait Carrier {
    /// Human-readable name (`"dct"`, `"dwt"`, `"alpha"`).
    const NAME: &'static str;

    /// Verification metrics produced by this carrier.
    type Metrics: std::fmt::Debug;

    /// Embed watermark into `img` in-place.
    ///
    /// Returns `(count, positions)` where `count` is the number of marked
    /// blocks/coefficients and `positions` are the sorted coordinates used.
    fn embed(
        img: &mut ImageBuffer<Rgb<u8>, Vec<u8>>,
        geometry: &GeometryFile,
        recipient_id: Option<&str>,
        key: Option<&str>,
        placement: &PlacementStrategy,
    ) -> Result<(u64, Vec<(u32, u32)>)>;

    /// Embed with explicit strength (DWT uses `dwt_strength`, DCT ignores it).
    ///
    /// Default impl forwards to `embed` so DCT callers need not branch.
    fn embed_with_strength(
        img: &mut ImageBuffer<Rgb<u8>, Vec<u8>>,
        geometry: &GeometryFile,
        recipient_id: Option<&str>,
        key: Option<&str>,
        placement: &PlacementStrategy,
        strength: f32,
    ) -> Result<(u64, Vec<(u32, u32)>)> {
        let _ = strength;
        Self::embed(img, geometry, recipient_id, key, placement)
    }

    /// Verify watermark presence and return carrier-specific metrics.
    ///
    /// `placement` selects the coefficient placement arm. For DWT only
    /// `Skeleton` is supported — `Edge`/`Prng` return an error.
    fn verify(
        img: &ImageBuffer<Rgb<u8>, Vec<u8>>,
        geometry: &GeometryFile,
        placement: &PlacementStrategy,
    ) -> Result<Self::Metrics>;

    /// Verify the key-derived secret layer (differential-pair mean).
    ///
    /// Returns mean signal: correct key → ≈ 2·delta, wrong key → ≈ 0.
    fn verify_secret(img: &ImageBuffer<Rgb<u8>, Vec<u8>>, key: &str) -> f64;

    /// Extract geometry-free recipient ID (self-sync PRNG recovered).
    fn extract(img: &ImageBuffer<Rgb<u8>, Vec<u8>>, id_length: usize) -> Result<String>;

    /// Whether `metrics` indicates watermark presence at `threshold`.
    fn metrics_is_present(metrics: &Self::Metrics, threshold: f64) -> bool;

    /// Mean signal extracted from `metrics` (for threshold comparisons).
    fn metrics_mean_signal(metrics: &Self::Metrics) -> f64;
}

// ── DctCarrier ───────────────────────────────────────────────────────────────

/// DCT-domain carrier (`8×8` blocks, `F[2,3]` primary + `F[3,4]` ID bits).
pub struct DctCarrier;

impl Carrier for DctCarrier {
    const NAME: &'static str = "dct";
    type Metrics = crate::dct::DctSignalMetrics;

    fn embed(
        img: &mut ImageBuffer<Rgb<u8>, Vec<u8>>,
        geometry: &GeometryFile,
        recipient_id: Option<&str>,
        key: Option<&str>,
        placement: &PlacementStrategy,
    ) -> Result<(u64, Vec<(u32, u32)>)> {
        crate::dct::embed(img, geometry, recipient_id, key, placement)
    }

    fn verify(
        img: &ImageBuffer<Rgb<u8>, Vec<u8>>,
        geometry: &GeometryFile,
        placement: &PlacementStrategy,
    ) -> Result<Self::Metrics> {
        crate::dct::verify(img, geometry, placement)
    }

    fn verify_secret(img: &ImageBuffer<Rgb<u8>, Vec<u8>>, key: &str) -> f64 {
        crate::dct::verify_secret(img, key)
    }

    fn extract(img: &ImageBuffer<Rgb<u8>, Vec<u8>>, id_length: usize) -> Result<String> {
        let (w, h) = img.dimensions();
        crate::extract::extract_from_dct(img, id_length, w, h)
    }

    fn metrics_is_present(metrics: &Self::Metrics, threshold: f64) -> bool {
        metrics.is_present(threshold as f32)
    }

    fn metrics_mean_signal(metrics: &Self::Metrics) -> f64 {
        f64::from(metrics.signal_strength)
    }
}

// ── DwtCarrier ───────────────────────────────────────────────────────────────

/// DWT-domain carrier (Haar LH band).
///
/// Only `PlacementStrategy::Skeleton` is currently supported. `Edge` and
/// `Prng` are rejected fail-closed (`anyhow::Error` containing "unsupported
/// DWT placement") so that callers cannot silently receive a Skeleton result
/// when they asked for a different placement arm. This keeps `DwtCarrier`
/// consistent with `DctCarrier` (which does honour all three placements) and
/// with `verify.rs` which now forwards the placement to `dwt_embed::verify`.
pub struct DwtCarrier;

impl Carrier for DwtCarrier {
    const NAME: &'static str = "dwt";
    type Metrics = crate::dwt_embed::DwtSignalMetrics;

    fn embed(
        img: &mut ImageBuffer<Rgb<u8>, Vec<u8>>,
        geometry: &GeometryFile,
        recipient_id: Option<&str>,
        key: Option<&str>,
        placement: &PlacementStrategy,
    ) -> Result<(u64, Vec<(u32, u32)>)> {
        crate::dwt_embed::embed(img, geometry, recipient_id, key, placement)
    }

    fn embed_with_strength(
        img: &mut ImageBuffer<Rgb<u8>, Vec<u8>>,
        geometry: &GeometryFile,
        recipient_id: Option<&str>,
        key: Option<&str>,
        placement: &PlacementStrategy,
        strength: f32,
    ) -> Result<(u64, Vec<(u32, u32)>)> {
        crate::dwt_embed::embed_with_strength(img, geometry, recipient_id, key, placement, strength)
    }

    fn verify(
        img: &ImageBuffer<Rgb<u8>, Vec<u8>>,
        geometry: &GeometryFile,
        placement: &PlacementStrategy,
    ) -> Result<Self::Metrics> {
        crate::dwt_embed::verify(img, geometry, placement)
    }

    fn verify_secret(img: &ImageBuffer<Rgb<u8>, Vec<u8>>, key: &str) -> f64 {
        crate::dwt_embed::verify_secret(img, key)
    }

    fn extract(img: &ImageBuffer<Rgb<u8>, Vec<u8>>, id_length: usize) -> Result<String> {
        let (w, h) = img.dimensions();
        crate::extract::extract_from_dwt(img, id_length, w, h)
    }

    fn metrics_is_present(metrics: &Self::Metrics, threshold: f64) -> bool {
        // Mirrors wasm_api/verify logic: mean_signal dominates, detection_rate
        // is a secondary gate for noisy images.
        let mean = f64::from(metrics.mean_signal);
        mean >= threshold || (metrics.detection_rate >= 0.8 && mean >= 2.0)
    }

    fn metrics_mean_signal(metrics: &Self::Metrics) -> f64 {
        f64::from(metrics.mean_signal)
    }
}

// ── Framed payload extension (CTX-0020) ──────────────────────────────────────

/// Framed payload helpers — Credential / Pointer / Message share this pipeline:
/// `payload → CBOR(frame) → HMAC → ECC → interleave → differential±delta`
/// and reverse via soft-bits LLR.
///
/// These are inherent methods on DctCarrier/DwtCarrier so existing `Carrier`
/// call sites stay unchanged; new credential code calls `DctCarrier::embed_framed`.
impl DctCarrier {
    /// Embed a framed payload (already authenticated via `keys.k_mac()`).
    /// `payload` is raw application bytes (e.g., 16B 128b token). This method
    /// handles CBOR framing, HMAC, ECC, interleave and differential modulation.
    pub fn embed_framed(
        img: &mut ImageBuffer<Rgb<u8>, Vec<u8>>,
        geometry: &GeometryFile,
        payload: &[u8],
        keys: &crate::keying::KeyMaterial,
        placement: &PlacementStrategy,
        profile: crate::ecc::Profile,
        payload_type: crate::framing::PayloadType,
    ) -> Result<(u64, Vec<(u32, u32)>)> {
        let params = crate::framing::Params {
            version: 1,
            payload_type,
            flags: 0,
        };
        let sealed = crate::framing::seal(payload, &params, keys.k_mac());
        let coded = crate::ecc::encode(&sealed, profile);
        let coded_bits: Vec<bool> = match profile {
            crate::ecc::Profile::Repetition8 | crate::ecc::Profile::Bch { .. } => {
                coded.iter().map(|&b| b != 0).collect()
            }
            crate::ecc::Profile::RsInterleaved { .. } => crate::ecc::bytes_to_bits(&coded),
        };
        // Delegate to DCT differential embed
        crate::dct::embed_coded_bits(img, geometry, &coded_bits, keys, placement)
    }

    /// Extract and open a framed payload. Returns raw payload bytes after ECC
    /// decode and HMAC verify. Uses soft-bit LLR path.
    /// `expected_payload_len` is the known credential size (e.g., 16 for 128b).
    /// If None, will try to auto-detect via progressive slicing.
    pub fn extract_framed(
        img: &ImageBuffer<Rgb<u8>, Vec<u8>>,
        keys: &crate::keying::KeyMaterial,
        profile: crate::ecc::Profile,
    ) -> Result<Vec<u8>> {
        Self::extract_framed_with_hint(img, keys, profile, Some(16))
    }

    pub fn extract_framed_with_hint(
        img: &ImageBuffer<Rgb<u8>, Vec<u8>>,
        keys: &crate::keying::KeyMaterial,
        profile: crate::ecc::Profile,
        expected_payload_len: Option<usize>,
    ) -> Result<Vec<u8>> {
        let try_decode = |soft: &[crate::ecc::SoftBit]| -> Result<Vec<u8>> {
            let decoded_sealed = crate::ecc::decode(soft, profile)?;
            let (_hdr, payload) = crate::framing::open(&decoded_sealed, keys.k_mac())?;
            Ok(payload)
        };
        if let Some(len) = expected_payload_len {
            let params = crate::framing::Params {
                version: 1,
                payload_type: crate::framing::PayloadType::Credential,
                flags: 0,
            };
            let sealed_len = crate::framing::sealed_len(len, &params);
            let need_bits = crate::ecc::coded_bits_len(sealed_len, profile);
            let soft = crate::dct::extract_coded_bits_soft_with_hint(img, keys, Some(need_bits))?;
            let mut decoded_sealed = crate::ecc::decode(&soft, profile)?;
            // BCH pads to k-boundary; truncate to actual sealed length before open
            if decoded_sealed.len() > sealed_len {
                decoded_sealed.truncate(sealed_len);
            }
            let (_hdr, payload) = crate::framing::open(&decoded_sealed, keys.k_mac())?;
            return Ok(payload);
        }
        let soft_all = crate::dct::extract_coded_bits_soft(img, keys)?;
        // Auto-detect: try slicing at various repetition-aligned lengths until success
        // For now, just try full and then progressive 8-aligned prefixes.
        let steps = soft_all.len() / 8;
        for k in (1..=steps).rev() {
            let end = k * 8;
            if let Ok(p) = try_decode(&soft_all[..end]) {
                return Ok(p);
            }
        }
        // Fallback to full
        try_decode(&soft_all)
    }

    /// Verify framed payload presence via soft-bit variance (without full decode).
    /// Returns true if mean LLR magnitude exceeds threshold.
    pub fn verify_framed(
        img: &ImageBuffer<Rgb<u8>, Vec<u8>>,
        keys: &crate::keying::KeyMaterial,
        threshold: f64,
    ) -> Result<bool> {
        let soft = crate::dct::extract_coded_bits_soft(img, keys)?;
        if soft.is_empty() {
            return Ok(false);
        }
        let mean_abs_llr: f32 = soft.iter().map(|s| s.llr.abs()).sum::<f32>() / soft.len() as f32;
        Ok(mean_abs_llr as f64 >= threshold)
    }

    // ── CTX-0021: original-assisted (registered-residual) extract / verify ─────

    /// Original-assisted extraction: `R = I_aligned − I_original` → matched filter.
    ///
    /// `original` is the server-held cover (private). `submitted` is the
    /// submitted credential image (possibly distorted). `registration` warps
    /// `submitted` into `original`'s coords before `R` is formed.
    pub fn extract_framed_registered(
        original: &ImageBuffer<Rgb<u8>, Vec<u8>>,
        submitted: &ImageBuffer<Rgb<u8>, Vec<u8>>,
        registration: &dyn Registration,
        keys: &crate::keying::KeyMaterial,
        profile: crate::ecc::Profile,
        expected_payload_len: Option<usize>,
    ) -> Result<Vec<u8>> {
        let aligned = registration
            .align(original, submitted)
            .context("registration warp failed")?;
        Self::extract_framed_registered_aligned(
            original,
            &aligned.image,
            keys,
            profile,
            expected_payload_len,
        )
    }

    /// Same as `extract_framed_registered` but `aligned` is already warped.
    pub fn extract_framed_registered_aligned(
        original: &ImageBuffer<Rgb<u8>, Vec<u8>>,
        aligned: &ImageBuffer<Rgb<u8>, Vec<u8>>,
        keys: &crate::keying::KeyMaterial,
        profile: crate::ecc::Profile,
        expected_payload_len: Option<usize>,
    ) -> Result<Vec<u8>> {
        if let Some(len) = expected_payload_len {
            let params = crate::framing::Params {
                version: 1,
                payload_type: crate::framing::PayloadType::Credential,
                flags: 0,
            };
            let sealed_len = crate::framing::sealed_len(len, &params);
            let need_bits = crate::ecc::coded_bits_len(sealed_len, profile);
            let soft = crate::dct::extract_coded_bits_soft_residual(
                original,
                aligned,
                keys,
                Some(need_bits),
            )?;
            let mut decoded_sealed = crate::ecc::decode(&soft, profile)?;
            if decoded_sealed.len() > sealed_len {
                decoded_sealed.truncate(sealed_len);
            }
            let (_hdr, payload) = crate::framing::open(&decoded_sealed, keys.k_mac())?;
            return Ok(payload);
        }
        let soft = crate::dct::extract_coded_bits_soft_residual(original, aligned, keys, None)?;
        let decoded_sealed = crate::ecc::decode(&soft, profile)?;
        let (_hdr, payload) = crate::framing::open(&decoded_sealed, keys.k_mac())?;
        Ok(payload)
    }

    /// Hybrid extractor: blind locator → cover family → strong verify.
    ///
    /// `vault` holds candidate originals (cover family). Each candidate is
    /// tried with `registration` + residual decode. The first candidate whose
    /// `R`-based decode passes ECC + HMAC is returned as `HybridMatch`.
    /// This fixes the bootstrap problem of selecting among N covers without a
    /// file-XOR trick.
    pub fn extract_framed_hybrid(
        submitted: &ImageBuffer<Rgb<u8>, Vec<u8>>,
        vault: &CoverVault,
        registration: &dyn Registration,
        keys: &crate::keying::KeyMaterial,
        profile: crate::ecc::Profile,
        expected_payload_len: Option<usize>,
    ) -> Result<HybridMatch> {
        anyhow::ensure!(!vault.is_empty(), "hybrid vault is empty");
        for (idx, (cover_id, original)) in vault.all().iter().enumerate() {
            let aligned = match registration.align(original, submitted) {
                Ok(a) => a,
                Err(_) => continue,
            };
            let res = Self::extract_framed_registered_aligned(
                original,
                &aligned.image,
                keys,
                profile,
                expected_payload_len,
            );
            if let Ok(payload) = res {
                return Ok(HybridMatch {
                    vault_index: idx,
                    cover_id: cover_id.clone(),
                    payload,
                    transform: aligned.transform,
                });
            }
        }
        anyhow::bail!(
            "hybrid extraction failed: no vault cover produced a valid payload (tried {})",
            vault.len()
        )
    }

    /// Strong verify via residual: returns true if residual decode succeeds.
    pub fn verify_framed_registered(
        original: &ImageBuffer<Rgb<u8>, Vec<u8>>,
        submitted: &ImageBuffer<Rgb<u8>, Vec<u8>>,
        registration: &dyn Registration,
        keys: &crate::keying::KeyMaterial,
        profile: crate::ecc::Profile,
        expected_payload_len: Option<usize>,
    ) -> Result<bool> {
        let aligned = registration.align(original, submitted)?;
        let res = Self::extract_framed_registered_aligned(
            original,
            &aligned.image,
            keys,
            profile,
            expected_payload_len,
        );
        Ok(res.is_ok())
    }

    /// Unified `extract_framed_with_hint` that now accepts an optional cover.
    /// If `cover` is `Some`, uses the strong residual path; otherwise the
    /// blind path. This satisfies the CTX-0021 requirement that the hint
    /// extractor integrate soft-bits from `R` when a cover is available.
    pub fn extract_framed_with_hint_and_cover(
        img: &ImageBuffer<Rgb<u8>, Vec<u8>>,
        keys: &crate::keying::KeyMaterial,
        profile: crate::ecc::Profile,
        expected_payload_len: Option<usize>,
        cover: Option<&ImageBuffer<Rgb<u8>, Vec<u8>>>,
        registration: Option<&dyn Registration>,
    ) -> Result<Vec<u8>> {
        if let (Some(orig), Some(reg)) = (cover, registration) {
            return Self::extract_framed_registered(
                orig,
                img,
                reg,
                keys,
                profile,
                expected_payload_len,
            );
        }
        Self::extract_framed_with_hint(img, keys, profile, expected_payload_len)
    }
}

impl DwtCarrier {
    pub fn embed_framed(
        img: &mut ImageBuffer<Rgb<u8>, Vec<u8>>,
        geometry: &GeometryFile,
        payload: &[u8],
        keys: &crate::keying::KeyMaterial,
        placement: &PlacementStrategy,
        profile: crate::ecc::Profile,
        payload_type: crate::framing::PayloadType,
    ) -> Result<(u64, Vec<(u32, u32)>)> {
        let params = crate::framing::Params {
            version: 1,
            payload_type,
            flags: 0,
        };
        let sealed = crate::framing::seal(payload, &params, keys.k_mac());
        let coded = crate::ecc::encode(&sealed, profile);
        let coded_bits: Vec<bool> = match profile {
            crate::ecc::Profile::Repetition8 | crate::ecc::Profile::Bch { .. } => {
                coded.iter().map(|&b| b != 0).collect()
            }
            crate::ecc::Profile::RsInterleaved { .. } => crate::ecc::bytes_to_bits(&coded),
        };
        crate::dwt_embed::embed_coded_bits(img, geometry, &coded_bits, keys, placement)
    }

    pub fn extract_framed(
        img: &ImageBuffer<Rgb<u8>, Vec<u8>>,
        keys: &crate::keying::KeyMaterial,
        profile: crate::ecc::Profile,
    ) -> Result<Vec<u8>> {
        Self::extract_framed_with_hint(img, keys, profile, Some(16))
    }

    pub fn extract_framed_with_hint(
        img: &ImageBuffer<Rgb<u8>, Vec<u8>>,
        keys: &crate::keying::KeyMaterial,
        profile: crate::ecc::Profile,
        expected_payload_len: Option<usize>,
    ) -> Result<Vec<u8>> {
        let try_decode = |soft: &[crate::ecc::SoftBit]| -> Result<Vec<u8>> {
            let decoded_sealed = crate::ecc::decode(soft, profile)?;
            let (_hdr, payload) = crate::framing::open(&decoded_sealed, keys.k_mac())?;
            Ok(payload)
        };
        if let Some(len) = expected_payload_len {
            let params = crate::framing::Params {
                version: 1,
                payload_type: crate::framing::PayloadType::Credential,
                flags: 0,
            };
            let sealed_len = crate::framing::sealed_len(len, &params);
            let need_bits = crate::ecc::coded_bits_len(sealed_len, profile);
            let soft =
                crate::dwt_embed::extract_coded_bits_soft_with_hint(img, keys, Some(need_bits))?;
            let mut decoded_sealed = crate::ecc::decode(&soft, profile)?;
            if decoded_sealed.len() > sealed_len {
                decoded_sealed.truncate(sealed_len);
            }
            let (_hdr, payload) = crate::framing::open(&decoded_sealed, keys.k_mac())?;
            return Ok(payload);
        }
        let soft_all = crate::dwt_embed::extract_coded_bits_soft(img, keys)?;
        let steps = soft_all.len() / 8;
        for k in (1..=steps).rev() {
            let end = k * 8;
            if let Ok(p) = try_decode(&soft_all[..end]) {
                return Ok(p);
            }
        }
        try_decode(&soft_all)
    }

    // ── CTX-0021: original-assisted (registered-residual) for DWT ───────────

    pub fn extract_framed_registered(
        original: &ImageBuffer<Rgb<u8>, Vec<u8>>,
        submitted: &ImageBuffer<Rgb<u8>, Vec<u8>>,
        registration: &dyn Registration,
        keys: &crate::keying::KeyMaterial,
        profile: crate::ecc::Profile,
        expected_payload_len: Option<usize>,
    ) -> Result<Vec<u8>> {
        let aligned = registration
            .align(original, submitted)
            .context("registration warp failed (dwt)")?;
        Self::extract_framed_registered_aligned(
            original,
            &aligned.image,
            keys,
            profile,
            expected_payload_len,
        )
    }

    pub fn extract_framed_registered_aligned(
        original: &ImageBuffer<Rgb<u8>, Vec<u8>>,
        aligned: &ImageBuffer<Rgb<u8>, Vec<u8>>,
        keys: &crate::keying::KeyMaterial,
        profile: crate::ecc::Profile,
        expected_payload_len: Option<usize>,
    ) -> Result<Vec<u8>> {
        if let Some(len) = expected_payload_len {
            let params = crate::framing::Params {
                version: 1,
                payload_type: crate::framing::PayloadType::Credential,
                flags: 0,
            };
            let sealed_len = crate::framing::sealed_len(len, &params);
            let need_bits = crate::ecc::coded_bits_len(sealed_len, profile);
            let soft = crate::dwt_embed::extract_coded_bits_soft_residual(
                original,
                aligned,
                keys,
                Some(need_bits),
            )?;
            let mut decoded_sealed = crate::ecc::decode(&soft, profile)?;
            if decoded_sealed.len() > sealed_len {
                decoded_sealed.truncate(sealed_len);
            }
            let (_hdr, payload) = crate::framing::open(&decoded_sealed, keys.k_mac())?;
            return Ok(payload);
        }
        let soft =
            crate::dwt_embed::extract_coded_bits_soft_residual(original, aligned, keys, None)?;
        let decoded_sealed = crate::ecc::decode(&soft, profile)?;
        let (_hdr, payload) = crate::framing::open(&decoded_sealed, keys.k_mac())?;
        Ok(payload)
    }

    pub fn extract_framed_hybrid(
        submitted: &ImageBuffer<Rgb<u8>, Vec<u8>>,
        vault: &CoverVault,
        registration: &dyn Registration,
        keys: &crate::keying::KeyMaterial,
        profile: crate::ecc::Profile,
        expected_payload_len: Option<usize>,
    ) -> Result<HybridMatch> {
        anyhow::ensure!(!vault.is_empty(), "hybrid vault is empty (dwt)");
        for (idx, (cover_id, original)) in vault.all().iter().enumerate() {
            let aligned = match registration.align(original, submitted) {
                Ok(a) => a,
                Err(_) => continue,
            };
            let res = Self::extract_framed_registered_aligned(
                original,
                &aligned.image,
                keys,
                profile,
                expected_payload_len,
            );
            if let Ok(payload) = res {
                return Ok(HybridMatch {
                    vault_index: idx,
                    cover_id: cover_id.clone(),
                    payload,
                    transform: aligned.transform,
                });
            }
        }
        anyhow::bail!(
            "hybrid dwt extraction failed: no vault cover produced valid payload (tried {})",
            vault.len()
        )
    }

    pub fn verify_framed_registered(
        original: &ImageBuffer<Rgb<u8>, Vec<u8>>,
        submitted: &ImageBuffer<Rgb<u8>, Vec<u8>>,
        registration: &dyn Registration,
        keys: &crate::keying::KeyMaterial,
        profile: crate::ecc::Profile,
        expected_payload_len: Option<usize>,
    ) -> Result<bool> {
        let aligned = registration.align(original, submitted)?;
        let res = Self::extract_framed_registered_aligned(
            original,
            &aligned.image,
            keys,
            profile,
            expected_payload_len,
        );
        Ok(res.is_ok())
    }

    pub fn extract_framed_with_hint_and_cover(
        img: &ImageBuffer<Rgb<u8>, Vec<u8>>,
        keys: &crate::keying::KeyMaterial,
        profile: crate::ecc::Profile,
        expected_payload_len: Option<usize>,
        cover: Option<&ImageBuffer<Rgb<u8>, Vec<u8>>>,
        registration: Option<&dyn Registration>,
    ) -> Result<Vec<u8>> {
        if let (Some(orig), Some(reg)) = (cover, registration) {
            return Self::extract_framed_registered(
                orig,
                img,
                reg,
                keys,
                profile,
                expected_payload_len,
            );
        }
        Self::extract_framed_with_hint(img, keys, profile, expected_payload_len)
    }
}

// ── AlphaCarrier (presence-only, no recoverable bits) ──────────────────────

/// Alpha-channel carrier (sparse semi-transparent pixels).
///
/// Exists for completeness so `crate::core` can enumerate all carriers. Embed
/// is **not** implemented via the RGB `Carrier::embed` signature because alpha
/// compositing requires an `RgbaImage`; callers should continue using
/// `crate::embed::embed_to_image` for `Alpha`. Verify/extract helpers below
/// operate on `Rgba` buffers via `crate::signal`.
pub struct AlphaCarrier;

impl AlphaCarrier {
    /// Verify alpha presence on an `RgbaImage` byte buffer.
    pub fn verify_rgba(pixels: &[u8], width: u32, height: u32, threshold: f64) -> bool {
        let m = crate::signal::SignalMetrics::compute(pixels, width, height);
        m.is_present(threshold)
    }

    /// Verify alpha presence (v2) with minimum pixel count.
    pub fn verify_rgba_v2(
        pixels: &[u8],
        width: u32,
        height: u32,
        threshold: f64,
        min_pixels: u64,
    ) -> bool {
        let m = crate::signal::SignalMetrics::compute(pixels, width, height);
        m.is_present_v2(threshold, min_pixels)
    }
}
