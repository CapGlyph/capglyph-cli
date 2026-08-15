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

/// Strength for recipient ID bit embedding in DWT mode.
/// Must be large enough to dominate natural LH coefficient variance at skeleton
/// positions (which can be ±200 for textured photographic images).
/// 256.0 ensures ±delta >> typical group variance, making polarity reliable.
pub const DWT_ID_EMBED_STRENGTH: f32 = 256.0;

/// Flat-region LH coefficient threshold. Positions whose |LH coeff| is below
/// this are in visually flat image areas (solid background, sky). Strong ±256
/// embedding there produces visible ±128 spatial steps — so flat positions use
/// the much smaller FLAT_ID_EMBED_STRENGTH instead.
pub const FLAT_LH_THRESHOLD: f32 = 30.0;

/// ID/sync embedding strength for flat regions. ±32 → spatial change ±16,
/// invisible on solid backgrounds while still contributing polarity to the
/// group-mean decoding.
pub const FLAT_ID_EMBED_STRENGTH: f32 = 32.0;

/// Sub-band used for embedding.
pub const EMBED_BAND: WaveletBand = WaveletBand::LH;

/// Minimum absolute coefficient value to modify (skip near-zero coefficients).
pub const MIN_COEFF_THRESHOLD: f32 = 3.0;

/// Signal detection threshold: fraction of marked coefficients required to confirm.
pub const DWT_DETECT_THRESHOLD: f64 = 0.50;

/// Fixed magic seed for self-sync seed positions in the LH band (reuse DCT constant).
pub const SEED_MAGIC: u64 = crate::dct::SEED_MAGIC;

/// Redundancy for self-sync seed bits (same as DCT mode).
pub const SYNC_REDUNDANCY: usize = 8;

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
/// Three independent signal layers:
///   1. Primary watermark: +DWT_EMBED_STRENGTH at geometry positions (verify)
///   2. Self-sync seed: ±DWT_ID_EMBED_STRENGTH at SEED_MAGIC positions (64 bits)
///   3. Recipient ID: ±DWT_ID_EMBED_STRENGTH at stable_seed PRNG positions
///
/// Layers 2+3 are geometry-free — extraction locates them via PRNG only.
pub fn embed(
    img: &mut ImageBuffer<Rgb<u8>, Vec<u8>>,
    geometry: &GeometryFile,
    recipient_id: Option<&str>,
) -> Result<(u64, Vec<(u32, u32)>)> {
    let (w, h) = img.dimensions();

    let positions = collect_embed_positions(geometry, w, h);
    if positions.is_empty() {
        return Ok((0, vec![]));
    }

    // Compute ID bits if recipient_id provided
    let id_bits: Vec<bool> = if let Some(rid) = recipient_id {
        crate::spread_spectrum::str_to_bits(rid)
    } else {
        vec![]
    };

    let redundancy = crate::spread_spectrum::REDUNDANCY;
    let bits_needed = id_bits.len() * redundancy;

    // Band-space PRNG position sets (geometry-free layers)
    let band_w = w / 2;
    let band_h = h / 2;

    // Self-sync positions carry the 64-bit stable seed (only when ID is embedded)
    let seed = crate::dct::stable_seed(img);
    let sync_positions = if recipient_id.is_some() {
        prng_band_positions(SEED_MAGIC, band_w, band_h, 64 * SYNC_REDUNDANCY)
    } else {
        vec![]
    };
    let sync_set: std::collections::HashSet<(u32, u32)> = sync_positions.iter().copied().collect();

    // ID positions derived from the stable seed, excluding sync positions
    let id_positions: Vec<(u32, u32)> = if recipient_id.is_some() {
        prng_band_positions(seed, band_w, band_h, bits_needed + sync_set.len())
            .into_iter()
            .filter(|p| !sync_set.contains(p))
            .collect()
    } else {
        vec![]
    };

    let mut total_modified = 0u64;

    // Process each RGB channel independently
    for ch in 0..3usize {
        let channel_matrix = extract_channel(img, ch);

        // Forward DWT
        let mut decomp = haar_2d_forward(&channel_matrix)?;
        let band = decomp.band_mut(EMBED_BAND);
        let (bh, bw) = (band.len(), band[0].len());

        // Layer 1: primary watermark at geometry positions
        for &(bx, by) in &positions {
            let bx = bx as usize;
            let by = by as usize;
            if bx < bw && by < bh {
                band[by][bx] += DWT_EMBED_STRENGTH;
                total_modified += 1;
            }
        }

        // Layer 2: self-sync seed bits (±) — flat positions use reduced strength
        let seed_bits: Vec<bool> = seed
            .to_le_bytes()
            .iter()
            .flat_map(|b| (0..8).rev().map(move |i| (b >> i) & 1 == 1))
            .collect();
        for (i, &(bx, by)) in sync_positions.iter().enumerate() {
            let bit_idx = i / SYNC_REDUNDANCY;
            if bit_idx >= seed_bits.len() {
                break;
            }
            let bx = bx as usize;
            let by = by as usize;
            if bx < bw && by < bh {
                // Check flatness BEFORE modification
                let is_flat = band[by][bx].abs() < FLAT_LH_THRESHOLD;
                let strength = if is_flat {
                    FLAT_ID_EMBED_STRENGTH
                } else {
                    DWT_ID_EMBED_STRENGTH
                };
                let delta = if seed_bits[bit_idx] {
                    strength
                } else {
                    -strength
                };
                band[by][bx] += delta;
                total_modified += 1;
            }
        }

        // Layer 3: recipient ID bits (±) — flat positions use reduced strength
        for (i, &(bx, by)) in id_positions.iter().enumerate() {
            if i >= bits_needed {
                break;
            }
            let bit_idx = i / redundancy;
            let bx = bx as usize;
            let by = by as usize;
            if bx < bw && by < bh {
                let is_flat = band[by][bx].abs() < FLAT_LH_THRESHOLD;
                let strength = if is_flat {
                    FLAT_ID_EMBED_STRENGTH
                } else {
                    DWT_ID_EMBED_STRENGTH
                };
                let delta = if id_bits[bit_idx] {
                    strength
                } else {
                    -strength
                };
                band[by][bx] += delta;
                total_modified += 1;
            }
        }

        // Inverse DWT
        let reconstructed = haar_2d_inverse(&decomp)?;
        write_channel(img, ch, &reconstructed);
    }

    Ok((total_modified / 3, positions))
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

    let positions = collect_embed_positions(geometry, w, h);
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
/// Returns positions in deterministic sorted order (for reproducible ID embedding).
fn collect_embed_positions(geometry: &GeometryFile, img_w: u32, img_h: u32) -> Vec<(u32, u32)> {
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

    // Sort for deterministic ordering
    let mut positions: Vec<(u32, u32)> = positions.into_iter().collect();
    positions.sort_unstable();
    positions
}

/// Generate a deterministic ordered position list in LH band space.
///
/// Mirrors `crate::dct::prng_block_list` but operates on band coordinates
/// (band_w × band_h) instead of 8×8 block coordinates. Positions are sorted
/// for cross-process deterministic ordering.
pub fn prng_band_positions(seed: u64, band_w: u32, band_h: u32, count: usize) -> Vec<(u32, u32)> {
    if band_w == 0 || band_h == 0 {
        return vec![];
    }
    let mut set = std::collections::HashSet::new();
    let mut state = seed;
    while set.len() < count {
        state = lcg_next(state);
        let bx = ((state >> 32) as u32) % band_w;
        let by = (state as u32) % band_h;
        set.insert((bx, by));
    }
    let mut list: Vec<(u32, u32)> = set.into_iter().collect();
    list.sort_unstable();
    list
}

/// LCG with Knuth's constants — mirrors dct.rs for band-space PRNG.
fn lcg_next(state: u64) -> u64 {
    state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407)
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

        let (n, _positions) = embed(&mut img, &geo, None).unwrap();
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
    fn test_dwt_recipient_id_roundtrip() {
        use crate::geometry::PathEntry;

        let (w, h) = (512u32, 512u32);
        let mut img = ImageBuffer::from_fn(w, h, |x, y| {
            let v = ((x * 3 + y * 7 + x * y) % 255) as u8;
            Rgb([v, (v + 40) % 255, (v + 80) % 255])
        });

        // Build a geometry with multiple crossing paths to generate many LH positions.
        // Need >= 200 unique positions for "hi" (2 chars × 8 bits × 5 redundancy = 80 positions).
        let n_paths = 40;
        let step = w / n_paths;
        let mut paths = Vec::new();
        for i in 0..n_paths {
            let x = (i * step) as f64;
            paths.push(PathEntry {
                color: None,
                points: (0..20).map(|j| [x, (j as f64 / 19.0) * h as f64]).collect(),
            });
        }
        // Also add horizontal paths
        for i in 0..n_paths {
            let y = (i * step) as f64;
            paths.push(PathEntry {
                color: None,
                points: (0..20).map(|j| [(j as f64 / 19.0) * w as f64, y]).collect(),
            });
        }

        use crate::geometry::{AnalysisParams, GeometryFile};
        let geo = GeometryFile {
            version: 1,
            original_width: w,
            original_height: h,
            analysis_params: AnalysisParams {
                detail: 60,
                min_path_len: 5,
                chaikin_iters: 3,
                color: false,
            },
            paths,
            prng_seed: None,
            blocks: None,
        };

        let rid = "hi";
        let bits_needed = rid.len() * 8 * crate::spread_spectrum::REDUNDANCY;

        // Embed with recipient ID
        let (n, _positions) = embed(&mut img, &geo, Some(rid)).unwrap();
        assert!(n > 0, "No coefficients modified");

        // Geometry-free extraction: self-sync seed → PRNG ID positions → decode
        let band_w = w / 2;
        let band_h = h / 2;
        let sync_positions = prng_band_positions(SEED_MAGIC, band_w, band_h, 64 * SYNC_REDUNDANCY);

        // Recover seed from self-sync positions (embed modified the image, so
        // stable_seed can no longer be recomputed directly).
        let mut sync_signals: Vec<f32> = vec![0.0; 64 * SYNC_REDUNDANCY];
        for ch in 0..3usize {
            let channel_matrix = extract_channel(&img, ch);
            let decomp = crate::dwt::haar_2d_forward(&channel_matrix).unwrap();
            let band = decomp.band(EMBED_BAND);
            let (bh, bw) = (band.len(), band[0].len());
            for (i, &(bx, by)) in sync_positions.iter().enumerate() {
                let bx = bx as usize;
                let by = by as usize;
                if bx < bw && by < bh {
                    sync_signals[i] += band[by][bx];
                }
            }
        }
        for s in &mut sync_signals {
            *s /= 3.0;
        }
        let sync_global = sync_signals.iter().sum::<f32>() / sync_signals.len() as f32;
        let mut seed_bytes = [0u8; 8];
        for (byte_idx, _byte_bits) in (0..64).step_by(8).enumerate() {
            let mut byte = 0u8;
            for bit in 0..8 {
                let start = (byte_idx * 8 + bit) * SYNC_REDUNDANCY;
                let group_mean: f32 = sync_signals[start..start + SYNC_REDUNDANCY]
                    .iter()
                    .sum::<f32>()
                    / SYNC_REDUNDANCY as f32;
                byte = (byte << 1) | (group_mean > sync_global) as u8;
            }
            seed_bytes[byte_idx] = byte;
        }
        let seed = u64::from_le_bytes(seed_bytes);

        let sync_set: std::collections::HashSet<(u32, u32)> =
            sync_positions.iter().copied().collect();
        let id_positions: Vec<(u32, u32)> =
            prng_band_positions(seed, band_w, band_h, bits_needed + sync_set.len())
                .into_iter()
                .filter(|p| !sync_set.contains(p))
                .collect();
        assert!(
            id_positions.len() >= bits_needed,
            "Not enough PRNG positions"
        );

        let redundancy = crate::spread_spectrum::REDUNDANCY;
        let mut bit_signals: Vec<f32> = vec![0.0; bits_needed];
        for ch in 0..3usize {
            let channel_matrix = extract_channel(&img, ch);
            let decomp = crate::dwt::haar_2d_forward(&channel_matrix).unwrap();
            let band = decomp.band(EMBED_BAND);
            let (bh, bw) = (band.len(), band[0].len());
            for (i, &(bx, by)) in id_positions.iter().enumerate().take(bits_needed) {
                let bx = bx as usize;
                let by = by as usize;
                if bx < bw && by < bh {
                    bit_signals[i] += band[by][bx];
                }
            }
        }
        for s in &mut bit_signals {
            *s /= 3.0;
        }
        let global_mean = bit_signals.iter().sum::<f32>() / bit_signals.len() as f32;
        let mut decoded_bits = Vec::new();
        for bit_idx in 0..(rid.len() * 8) {
            let start = bit_idx * redundancy;
            let end = (start + redundancy).min(bits_needed);
            let group_mean = bit_signals[start..end].iter().sum::<f32>() / (end - start) as f32;
            decoded_bits.push(group_mean > global_mean);
        }
        let decoded = crate::spread_spectrum::bits_to_str(&decoded_bits).unwrap();
        assert_eq!(
            decoded, rid,
            "DWT recipient-id roundtrip failed: got {:?}",
            decoded
        );
    }
}
