//! Carrier trait — abstracts embed/verify/extract + metrics for each watermark mode.
//!
//! Prepares mechanical extraction of `capglyph-core` (DEC-0003 Phase 1). The trait
//! is intentionally object-safe-ish but used as static dispatch via associated
//! types; `embed.rs` dispatches through `DctCarrier`/`DwtCarrier` impls so the
//! call sites are ready to become `capglyph_core::Carrier` after the crate split.

use anyhow::Result;
use image::{ImageBuffer, Rgb};

use crate::geometry::GeometryFile;
use crate::placement::Placement;

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
        placement: &Placement,
    ) -> Result<(u64, Vec<(u32, u32)>)>;

    /// Embed with explicit strength (DWT uses `dwt_strength`, DCT ignores it).
    ///
    /// Default impl forwards to `embed` so DCT callers need not branch.
    fn embed_with_strength(
        img: &mut ImageBuffer<Rgb<u8>, Vec<u8>>,
        geometry: &GeometryFile,
        recipient_id: Option<&str>,
        key: Option<&str>,
        placement: &Placement,
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
        placement: &Placement,
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
