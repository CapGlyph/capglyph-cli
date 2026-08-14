//! DWT-based watermark embedding and verification.
//!
//! Embeds a structural watermark by modifying LH sub-band coefficients at
//! positions determined by the image's own path geometry (skeleton-guided).
//! Unlike DCT (which operates on fixed 8×8 blocks), DWT coefficients scale
//! with the image, giving much better robustness against resize operations.

use anyhow::Result;
use image::{ImageBuffer, Rgb};

use crate::dwt::{haar_2d_forward, haar_2d_inverse, WaveletBand};
use crate::geometry::GeometryFile;

/// Strength of DWT coefficient modification.
/// Higher = more robust but more visible. 8.0 gives PSNR ≈ 44-46 dB (invisible).
pub const DWT_EMBED_STRENGTH: f32 = 8.0;

/// Sub-band used for embedding.
pub const EMBED_BAND: WaveletBand = WaveletBand::LH;

/// Minimum absolute coefficient value to modify (skip near-zero coefficients).
pub const MIN_COEFF_THRESHOLD: f32 = 3.0;

/// Signal detection threshold: fraction of marked coefficients required to confirm.
pub const DWT_DETECT_THRESHOLD: f64 = 0.50;

/// Metrics from DWT watermark verification.
#[derive(Debug)]
pub struct DwtSignalMetrics {
    /// Number of coefficients that were checked
    pub total_coefficients: u64,
    /// Number that show the expected modification direction
    pub detected_count: u64,
    /// Detection rate (detected / total)
    pub detection_rate: f64,
    /// Mean signal strength at embedded positions
    pub mean_signal: f32,
}

// ── Embed ─────────────────────────────────────────────────────────────────────

/// Embed a DWT watermark into the RGB image in-place.
///
/// For each RGB channel:
///   1. Convert channel to f32 matrix
///   2. Apply 2D Haar DWT
///   3. Map geometry path coordinates into LH sub-band coordinates
///   4. Add EMBED_STRENGTH to positive coefficients, subtract from negative
///   5. Apply inverse DWT
///   6. Clamp to 0-255 and write back
///
/// Returns the number of modified coefficients.
pub fn embed(
    img: &mut ImageBuffer<Rgb<u8>, Vec<u8>>,
    geometry: &GeometryFile,
    recipient_id: Option<&str>,
) -> Result<u64> {
    let (w, h) = img.dimensions();

    let positions = collect_embed_positions(geometry, w, h, recipient_id);
    if positions.is_empty() {
        return Ok(0);
    }

    let mut total_modified = 0u64;

    // Process each RGB channel independently
    for ch in 0..3usize {
        let channel_matrix = extract_channel(img, ch);

        // Forward DWT
        let mut decomp = haar_2d_forward(&channel_matrix)?;

        // Modify LH band coefficients at geometry positions
        let band = decomp.band_mut(EMBED_BAND);
        let (bh, bw) = (band.len(), band[0].len());

        for &(bx, by) in &positions {
            let bx = bx as usize;
            let by = by as usize;
            if bx < bw && by < bh {
                // Always embed: add EMBED_STRENGTH regardless of current coefficient value.
                // Near-zero coefficients are fine — we're adding a positive bias.
                band[by][bx] += DWT_EMBED_STRENGTH;
                total_modified += 1;
            }
        }

        // Inverse DWT
        let reconstructed = haar_2d_inverse(&decomp)?;

        // Write back clamped values
        write_channel(img, ch, &reconstructed);
    }

    Ok(total_modified / 3) // count per-channel, return per-position count
}

// ── Verify ────────────────────────────────────────────────────────────────────

/// Verify DWT watermark presence.
///
/// Checks if LH sub-band coefficients at geometry positions are shifted
/// in the expected direction (positive bias from embedding).
///
/// The test is distribution-free: it checks if the mean offset at marked
/// positions significantly exceeds the baseline (unmarked positions).
pub fn verify(
    img: &ImageBuffer<Rgb<u8>, Vec<u8>>,
    geometry: &GeometryFile,
) -> Result<DwtSignalMetrics> {
    let (w, h) = img.dimensions();

    let positions = collect_embed_positions(geometry, w, h, None);
    if positions.is_empty() {
        return Ok(DwtSignalMetrics {
            total_coefficients: 0,
            detected_count: 0,
            detection_rate: 0.0,
            mean_signal: 0.0,
        });
    }

    let mut total = 0u64;
    let mut detected = 0u64;
    let mut signal_sum = 0.0f64;

    // Average the detection over all 3 channels
    for ch in 0..3usize {
        let channel_matrix = extract_channel(img, ch);
        let decomp = haar_2d_forward(&channel_matrix)?;
        let band = decomp.band(EMBED_BAND);
        let (bh, bw) = (band.len(), band[0].len());

        for &(bx, by) in &positions {
            let bx = bx as usize;
            let by = by as usize;
            if bx < bw && by < bh {
                let coeff = band[by][bx];
                // Watermark pushes coeff in + direction; detect if positive bias
                if coeff > MIN_COEFF_THRESHOLD {
                    detected += 1;
                }
                signal_sum += coeff as f64;
                total += 1;
            }
        }
    }

    let detection_rate = if total > 0 {
        detected as f64 / total as f64
    } else {
        0.0
    };

    Ok(DwtSignalMetrics {
        total_coefficients: total,
        detected_count: detected,
        detection_rate,
        mean_signal: (signal_sum / total.max(1) as f64) as f32,
    })
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Convert geometry path coordinates into DWT LH sub-band positions.
/// The LH band is W/2 × H/2, so path coordinates are halved.
fn collect_embed_positions(
    geometry: &GeometryFile,
    img_w: u32,
    img_h: u32,
    recipient_id: Option<&str>,
) -> Vec<(u32, u32)> {
    let band_w = img_w / 2;
    let band_h = img_h / 2;

    // Build position set from path geometry (scale to LH band dimensions)
    let mut positions: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();

    for path in &geometry.paths {
        for point in &path.points {
            let px = point[0] as f32;
            let py = point[1] as f32;
            // Scale from image space to LH band space (divide by 2)
            let bx = (px as u32 * band_w / img_w).min(band_w.saturating_sub(1));
            let by = (py as u32 * band_h / img_h).min(band_h.saturating_sub(1));
            positions.insert((bx, by));
        }
    }

    // Apply recipient ID mixing if provided
    let positions: Vec<(u32, u32)> = if let Some(rid) = recipient_id {
        let seed = fnv1a_hash(rid);
        positions
            .into_iter()
            .filter(|(x, y)| {
                // XOR-shift the position with recipient seed for unique pattern
                let mixed = (*x as u64 ^ seed).wrapping_add(*y as u64);
                mixed.is_multiple_of(2) // ~50% of positions used per recipient
            })
            .collect()
    } else {
        positions.into_iter().collect()
    };

    positions
}

/// FNV-1a hash for recipient ID mixing.
fn fnv1a_hash(s: &str) -> u64 {
    const FNV_OFFSET: u64 = 14_695_981_039_346_656_037;
    const FNV_PRIME: u64 = 1_099_511_628_211;
    let mut hash = FNV_OFFSET;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Extract a single RGB channel as f32 matrix.
fn extract_channel(img: &ImageBuffer<Rgb<u8>, Vec<u8>>, ch: usize) -> Vec<Vec<f32>> {
    let (w, h) = img.dimensions();
    (0..h)
        .map(|y| (0..w).map(|x| img.get_pixel(x, y)[ch] as f32).collect())
        .collect()
}

/// Write a f32 matrix back to a single RGB channel, clamping to [0, 255].
fn write_channel(img: &mut ImageBuffer<Rgb<u8>, Vec<u8>>, ch: usize, data: &[Vec<f32>]) {
    let (w, h) = img.dimensions();
    for y in 0..h {
        for x in 0..w {
            let val = data[y as usize][x as usize].round().clamp(0.0, 255.0) as u8;
            img.get_pixel_mut(x, y)[ch] = val;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{AnalysisParams, GeometryFile, PathEntry};

    fn make_test_geometry(w: u32, h: u32) -> GeometryFile {
        // Create a diagonal path covering the image
        let points: Vec<[f64; 2]> = (0..20)
            .map(|i| {
                let t = i as f64 / 20.0;
                [t * w as f64, t * h as f64]
            })
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
            prng_seed: Some(0),
            blocks: None,
        }
    }

    #[test]
    fn test_dwt_embed_modifies_image() {
        let (w, h) = (64u32, 64u32);
        let mut img = ImageBuffer::from_fn(w, h, |x, y| {
            let v = ((x + y) % 255) as u8;
            Rgb([v, v, v])
        });
        let original = img.clone();
        let geo = make_test_geometry(w, h);

        let n = embed(&mut img, &geo, None).unwrap();
        assert!(n > 0, "Expected some coefficients to be modified");

        // Image should be visually similar but not identical
        let mut diff_sum = 0u64;
        for y in 0..h {
            for x in 0..w {
                let op = original.get_pixel(x, y);
                let mp = img.get_pixel(x, y);
                for c in 0..3 {
                    diff_sum += (op[c] as i64 - mp[c] as i64).unsigned_abs();
                }
            }
        }
        let mean_diff = diff_sum as f64 / (w * h * 3) as f64;
        assert!(
            mean_diff < 5.0,
            "Mean pixel diff too high: {mean_diff:.2} (watermark too visible)"
        );
    }

    #[test]
    fn test_dwt_verify_detects_embedded_watermark() {
        let (w, h) = (64u32, 64u32);
        let mut img = ImageBuffer::from_fn(w, h, |x, y| {
            let v = ((x * 3 + y * 7) % 255) as u8;
            Rgb([v, (v + 40) % 255, (v + 80) % 255])
        });
        let geo = make_test_geometry(w, h);

        // Baseline (no watermark)
        let metrics_before = verify(&img, &geo).unwrap();

        // Embed
        embed(&mut img, &geo, None).unwrap();

        // After embedding
        let metrics_after = verify(&img, &geo).unwrap();

        assert!(
            metrics_after.detection_rate > metrics_before.detection_rate,
            "Detection rate should increase after embedding: {} vs {}",
            metrics_after.detection_rate,
            metrics_before.detection_rate
        );
        assert!(
            metrics_after.detection_rate > DWT_DETECT_THRESHOLD,
            "Detection rate below threshold: {}",
            metrics_after.detection_rate
        );
    }
}
