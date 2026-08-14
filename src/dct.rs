//! Stage 2: RGB DCT-domain residual watermark.
//!
//! Embeds the watermark signal in mid-frequency DCT coefficients of 8×8 pixel
//! blocks along the geometric skeleton path. Unlike Stage 1, this signal:
//!
//! - Lives entirely in the RGB channels (no alpha dependency)
//! - Survives PNG→JPG conversion at quality ≥ 50
//! - Cannot be stripped by `img.convert('RGB')` or `convert -alpha off`
//!
//! ## Algorithm
//!
//! For each 8×8 block whose bounding box intersects a skeleton path pixel:
//!
//! 1. Forward DCT-II on each RGB channel independently
//! 2. Modulate mid-frequency coefficient at `(TARGET_U, TARGET_V)` (u=2, v=3):
//!    `C'[u][v] = C[u][v] + EMBED_DELTA`
//! 3. Inverse DCT back to pixel space
//! 4. Spatial change `|block' - block|_max ≈ 3.6/255` — sub-perceptual in textured areas
//!
//! ## Verification
//!
//! Re-extract skeleton → locate same 8×8 blocks → forward DCT → measure mean
//! coefficient at `TARGET_U, TARGET_V` vs non-skeleton baseline. If the difference
//! exceeds `VERIFY_THRESHOLD`, watermark is present.

use crate::geometry::GeometryFile;
use anyhow::Result;
use image::{ImageBuffer, Rgb};

// ─── Constants ───────────────────────────────────────────────────────────────

/// Target DCT coefficient position (u, v) in the 8×8 block.
/// u=2, v=3 is a mid-frequency position: large enough to survive JPEG quantization
/// at quality≥50, but not so high that it gets zeroed at quality=75.
/// Spatial domain max change ≈ delta * cos(5π/16) * cos(7π/16) / 4 ≈ delta * 0.11
pub const TARGET_U: usize = 2;
pub const TARGET_V: usize = 3;

/// Second DCT coefficient used for spread-spectrum recipient ID encoding.
/// (U=3, V=4) is adjacent to TARGET but distinct — separated to avoid interference.
pub const ID_TARGET_U: usize = 3;
pub const ID_TARGET_V: usize = 4;

/// Embedding strength for recipient ID bits in DCT coefficient (3,4).
/// Uses differential pair encoding: bit=1 → (A+delta, B-delta), bit=0 → (A-delta, B+delta)
/// where A and B are two consecutive blocks.
pub const ID_EMBED_DELTA: f32 = 64.0;
/// JPEG quantization step at quality=75 for this frequency ≈ 8–12.
/// delta=16 → survives quality≥50; spatial change ≈ 1.8/255 → invisible.
pub const EMBED_DELTA: f32 = 16.0;

/// Minimum mean offset to report watermark as present during verification.
/// Half of EMBED_DELTA to tolerate minor degradation (JPEG quality≥50 preserves ≥50%).
pub const VERIFY_THRESHOLD: f32 = 8.0;

// ─── Public API ──────────────────────────────────────────────────────────────

/// Embed DCT-domain watermark along skeleton paths.
///
/// Modifies `img` in-place. Returns (number of 8×8 blocks watermarked, sorted block coordinates).
///
/// When the skeleton has no paths (solid-color images, logos), falls back to
/// PRNG scatter: pseudorandom block selection seeded by image pixel hash.
/// This ensures all image types can be watermarked.
///
/// `recipient_id`: Optional string mixed into PRNG seed for per-recipient tracking.
///
/// The returned `Vec<(u32, u32)>` contains the exact sorted block coordinates used during embed,
/// which must be stored in the geometry file for accurate recipient ID extraction.
pub fn embed(
    img: &mut ImageBuffer<Rgb<u8>, Vec<u8>>,
    geometry: &GeometryFile,
    recipient_id: Option<&str>,
) -> Result<(u64, Vec<(u32, u32)>)> {
    let (iw, ih) = img.dimensions();

    // Collect all skeleton pixels via Bresenham
    let path_pixels = collect_path_pixels(geometry, iw, ih);

    let block_set = if path_pixels.is_empty() {
        // Fallback: no skeleton (solid colors, logos) → PRNG scatter
        prng_blocks(img, iw, ih, recipient_id)
    } else {
        // Normal path: skeleton-guided blocks
        let mut set = std::collections::HashSet::new();
        for (px, py) in &path_pixels {
            let bx = px / 8;
            let by = py / 8;
            if (bx + 1) * 8 <= iw && (by + 1) * 8 <= ih {
                set.insert((bx, by));
            }
        }
        set
    };

    let n_blocks = block_set.len() as u64;

    // Convert blocks to sorted vec for deterministic ordering
    let mut blocks: Vec<_> = block_set.into_iter().collect();
    blocks.sort_unstable();

    // Embed primary watermark + spread-spectrum recipient ID if provided
    if let Some(id_str) = recipient_id {
        let id_bits = crate::spread_spectrum::str_to_bits(id_str);
        let redundancy = crate::spread_spectrum::REDUNDANCY;
        let bits_needed = id_bits.len() * redundancy;

        if blocks.len() >= bits_needed {
            // Embed both primary watermark and ID bits
            for (i, &(bx, by)) in blocks.iter().enumerate() {
                let ox = bx * 8;
                let oy = by * 8;

                // Determine ID bit for this block (if within range)
                let id_delta = if i < bits_needed {
                    let bit_idx = i / redundancy;
                    if id_bits[bit_idx] {
                        ID_EMBED_DELTA
                    } else {
                        -ID_EMBED_DELTA
                    }
                } else {
                    0.0
                };

                for ch in 0..3usize {
                    let mut block = extract_block(img, ox, oy, ch);
                    dct8x8_forward(&mut block);
                    block[TARGET_U][TARGET_V] += EMBED_DELTA;
                    if id_delta != 0.0 {
                        block[ID_TARGET_U][ID_TARGET_V] += id_delta;
                    }
                    dct8x8_inverse(&mut block);
                    write_block(img, ox, oy, ch, &block);
                }
            }
        } else {
            // Not enough blocks — embed primary watermark only and warn
            tracing::warn!(
                "Insufficient blocks ({}) for ID embedding (need {}). Embedding primary watermark only.",
                blocks.len(),
                bits_needed
            );
            for &(bx, by) in &blocks {
                let ox = bx * 8;
                let oy = by * 8;
                for ch in 0..3usize {
                    let mut block = extract_block(img, ox, oy, ch);
                    dct8x8_forward(&mut block);
                    block[TARGET_U][TARGET_V] += EMBED_DELTA;
                    dct8x8_inverse(&mut block);
                    write_block(img, ox, oy, ch, &block);
                }
            }
        }
    } else {
        // No recipient ID — embed primary watermark only
        for &(bx, by) in &blocks {
            let ox = bx * 8;
            let oy = by * 8;
            for ch in 0..3usize {
                let mut block = extract_block(img, ox, oy, ch);
                dct8x8_forward(&mut block);
                block[TARGET_U][TARGET_V] += EMBED_DELTA;
                dct8x8_inverse(&mut block);
                write_block(img, ox, oy, ch, &block);
            }
        }
    }

    Ok((n_blocks, blocks))
}

/// Metrics from DCT-domain verification.
#[derive(Debug, Clone)]
pub struct DctSignalMetrics {
    pub watermarked_blocks: u64,
    pub total_skeleton_blocks: u64,
    /// Mean coefficient offset at TARGET_U,TARGET_V across skeleton blocks
    pub mean_offset: f32,
    /// Same for non-skeleton blocks (reference baseline)
    pub baseline_mean_offset: f32,
    /// Difference: if watermark present → mean_offset >> baseline_mean_offset
    pub signal_strength: f32,
    pub image_width: u32,
    pub image_height: u32,
}

impl DctSignalMetrics {
    pub fn is_present(&self, threshold: f32) -> bool {
        self.signal_strength >= threshold
    }

    pub fn summary(&self) -> String {
        format!(
            "dct_signal={:.2}  skeleton_blocks={}  mean_offset={:.2}  baseline={:.2}",
            self.signal_strength,
            self.total_skeleton_blocks,
            self.mean_offset,
            self.baseline_mean_offset,
        )
    }
}

/// Verify DCT watermark in an image by re-extracting the skeleton and measuring
/// the coefficient offset at the known embed position.
pub fn verify(
    img: &ImageBuffer<Rgb<u8>, Vec<u8>>,
    geometry: &GeometryFile,
) -> Result<DctSignalMetrics> {
    let (iw, ih) = img.dimensions();

    let path_pixels = collect_path_pixels(geometry, iw, ih);

    let mut skeleton_blocks = std::collections::HashSet::new();
    for (px, py) in &path_pixels {
        let bx = px / 8;
        let by = py / 8;
        if (bx + 1) * 8 <= iw && (by + 1) * 8 <= ih {
            skeleton_blocks.insert((bx, by));
        }
    }

    // Measure mean offset at TARGET across skeleton blocks
    let mut skeleton_sum = 0.0f64;
    let mut baseline_sum = 0.0f64;
    let mut baseline_count = 0u64;
    let total_bx = iw / 8;
    let total_by = ih / 8;

    for (bx, by) in &skeleton_blocks {
        // Average over 3 channels
        let mut ch_sum = 0.0f32;
        for ch in 0..3usize {
            let mut block = extract_block(img, bx * 8, by * 8, ch);
            dct8x8_forward(&mut block);
            ch_sum += block[TARGET_U][TARGET_V];
        }
        skeleton_sum += (ch_sum / 3.0) as f64;
    }

    // Sample some non-skeleton blocks as baseline
    for bx in (0..total_bx).step_by(4) {
        for by in (0..total_by).step_by(4) {
            if skeleton_blocks.contains(&(bx, by)) {
                continue;
            }
            let mut ch_sum = 0.0f32;
            for ch in 0..3usize {
                let mut block = extract_block(img, bx * 8, by * 8, ch);
                dct8x8_forward(&mut block);
                ch_sum += block[TARGET_U][TARGET_V];
            }
            baseline_sum += (ch_sum / 3.0) as f64;
            baseline_count += 1;
        }
    }

    let n_skel = skeleton_blocks.len() as u64;
    let mean_skel = if n_skel > 0 {
        (skeleton_sum / n_skel as f64) as f32
    } else {
        0.0
    };
    let mean_base = if baseline_count > 0 {
        (baseline_sum / baseline_count as f64) as f32
    } else {
        0.0
    };
    let signal = mean_skel - mean_base;

    Ok(DctSignalMetrics {
        watermarked_blocks: n_skel,
        total_skeleton_blocks: n_skel,
        mean_offset: mean_skel,
        baseline_mean_offset: mean_base,
        signal_strength: signal,
        image_width: iw,
        image_height: ih,
    })
}

// ─── DCT-II / IDCT-II for 8×8 blocks (pure Rust, no external deps) ──────────

/// Extract an 8×8 block from `img` at pixel offset (ox, oy) for color channel `ch`.
/// Returns `block[row][col]` as f32.
pub fn extract_block(
    img: &ImageBuffer<Rgb<u8>, Vec<u8>>,
    ox: u32,
    oy: u32,
    ch: usize,
) -> [[f32; 8]; 8] {
    let mut block = [[0.0f32; 8]; 8];
    for (row, row_buf) in block.iter_mut().enumerate() {
        for (col, cell) in row_buf.iter_mut().enumerate() {
            *cell = img.get_pixel(ox + col as u32, oy + row as u32)[ch] as f32;
        }
    }
    block
}

/// Write an 8×8 block back to `img`, clamping to [0, 255].
fn write_block(
    img: &mut ImageBuffer<Rgb<u8>, Vec<u8>>,
    ox: u32,
    oy: u32,
    ch: usize,
    block: &[[f32; 8]; 8],
) {
    for (row, row_buf) in block.iter().enumerate() {
        for (col, &val) in row_buf.iter().enumerate() {
            img.get_pixel_mut(ox + col as u32, oy + row as u32)[ch] =
                val.round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// 1D DCT-II for N=8 (orthonormal).
/// X[k] = α[k] * Σ_{n=0}^{7} x[n] * cos(π*k*(2n+1)/16)
/// α[0] = 1/√8, α[k>0] = √(2/8)
fn dct8(x: &[f32; 8]) -> [f32; 8] {
    const N: f32 = 8.0;
    let mut out = [0.0f32; 8];
    for (k, out_k) in out.iter_mut().enumerate() {
        let alpha = if k == 0 {
            1.0 / N.sqrt()
        } else {
            (2.0 / N).sqrt()
        };
        let sum: f32 = x
            .iter()
            .enumerate()
            .map(|(n, &xn)| {
                xn * (std::f32::consts::PI * k as f32 * (2.0 * n as f32 + 1.0) / 16.0).cos()
            })
            .sum();
        *out_k = alpha * sum;
    }
    out
}

/// 1D IDCT-II for N=8 (inverse of dct8).
/// `x[n] = Σ_{k=0}^{7} α[k] * X[k] * cos(π*k*(2n+1)/16)`
fn idct8(x: &[f32; 8]) -> [f32; 8] {
    const N: f32 = 8.0;
    let mut out = [0.0f32; 8];
    for (n, out_n) in out.iter_mut().enumerate() {
        let sum: f32 = x
            .iter()
            .enumerate()
            .map(|(k, &xk)| {
                let alpha = if k == 0 {
                    1.0 / N.sqrt()
                } else {
                    (2.0 / N).sqrt()
                };
                alpha * xk * (std::f32::consts::PI * k as f32 * (2.0 * n as f32 + 1.0) / 16.0).cos()
            })
            .sum();
        *out_n = sum;
    }
    out
}

/// 2D DCT-II on an 8×8 block (separable: row DCT then column DCT).
pub fn dct8x8_forward(block: &mut [[f32; 8]; 8]) {
    for row in block.iter_mut() {
        *row = dct8(row);
    }
    // Column-wise pass — index is genuinely needed for 2D array access.
    #[allow(clippy::needless_range_loop)]
    for c in 0..8 {
        let mut col_buf = [0.0f32; 8];
        for (r, col_val) in col_buf.iter_mut().enumerate() {
            *col_val = block[r][c];
        }
        col_buf = dct8(&col_buf);
        for (r, &val) in col_buf.iter().enumerate() {
            block[r][c] = val;
        }
    }
}

/// 2D IDCT-II on an 8×8 block (separable: column IDCT then row IDCT).
fn dct8x8_inverse(block: &mut [[f32; 8]; 8]) {
    #[allow(clippy::needless_range_loop)]
    for c in 0..8 {
        let mut col_buf = [0.0f32; 8];
        for (r, col_val) in col_buf.iter_mut().enumerate() {
            *col_val = block[r][c];
        }
        col_buf = idct8(&col_buf);
        for (r, &val) in col_buf.iter().enumerate() {
            block[r][c] = val;
        }
    }
    for row in block.iter_mut() {
        *row = idct8(row);
    }
}

// ─── Bresenham path rasterisation ────────────────────────────────────────────

fn collect_path_pixels(geometry: &GeometryFile, iw: u32, ih: u32) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for path in &geometry.paths {
        if path.points.len() < 2 {
            continue;
        }
        for win in path.points.windows(2) {
            let (x0, y0) = (win[0][0].round() as i32, win[0][1].round() as i32);
            let (x1, y1) = (win[1][0].round() as i32, win[1][1].round() as i32);
            bresenham(x0, y0, x1, y1, iw as i32, ih as i32, |x, y| {
                out.push((x as u32, y as u32));
            });
        }
    }
    out
}

// ─── PRNG fallback for solid-color / zero-path images ────────────────────────

/// Generate a deterministic pseudorandom set of 8×8 block coordinates for use
/// when the skeleton extractor produces no paths (solid colors, logos, flat UIs).
///
/// The seed is derived from a fast hash of the image pixel data, so the same
/// image always produces the same block set — making verification reproducible
/// without storing explicit path geometry.
///
/// Target coverage: ~5% of all 8×8 blocks (similar to skeleton density for
/// natural images at detail=60).
pub fn prng_blocks(
    img: &ImageBuffer<Rgb<u8>, Vec<u8>>,
    iw: u32,
    ih: u32,
    recipient_id: Option<&str>,
) -> std::collections::HashSet<(u32, u32)> {
    let raw = img.as_raw();
    let sample = &raw[..raw.len().min(4096)];
    let mut seed = fnv1a_hash(sample);
    if let Some(id) = recipient_id {
        seed ^= fnv1a_hash(id.as_bytes());
    }

    let total_bx = iw / 8;
    let total_by = ih / 8;
    let total_blocks = total_bx * total_by;
    // Target ~5% of available blocks, minimum 32
    let target = ((total_blocks as f64 * 0.05).round() as u32)
        .max(32)
        .min(total_blocks);

    let mut set = std::collections::HashSet::new();
    let mut state = seed;
    // LCG: simple, deterministic, dependency-free
    while set.len() < target as usize {
        state = lcg_next(state);
        let bx = ((state >> 32) as u32) % total_bx;
        let by = (state as u32) % total_by;
        if (bx + 1) * 8 <= iw && (by + 1) * 8 <= ih {
            set.insert((bx, by));
        }
    }
    set
}

/// FNV-1a 64-bit hash — fast, dependency-free.
fn fnv1a_hash(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x00000100000001b3);
    }
    h
}

/// LCG with Knuth's constants — reproducible across platforms.
fn lcg_next(state: u64) -> u64 {
    state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407)
}

/// Compute the PRNG seed for an image (used for verification without geometry).
pub fn image_seed(img: &ImageBuffer<Rgb<u8>, Vec<u8>>) -> u64 {
    let raw = img.as_raw();
    let sample = &raw[..raw.len().min(4096)];
    fnv1a_hash(sample)
}

/// Regenerate PRNG blocks from a seed (for verification).
pub fn prng_blocks_from_seed(seed: u64, iw: u32, ih: u32) -> std::collections::HashSet<(u32, u32)> {
    let total_bx = iw / 8;
    let total_by = ih / 8;
    let total_blocks = total_bx * total_by;
    let target = ((total_blocks as f64 * 0.05).round() as u32)
        .max(32)
        .min(total_blocks);

    let mut set = std::collections::HashSet::new();
    let mut state = seed;
    while set.len() < target as usize {
        state = lcg_next(state);
        let bx = ((state >> 32) as u32) % total_bx;
        let by = (state as u32) % total_by;
        if (bx + 1) * 8 <= iw && (by + 1) * 8 <= ih {
            set.insert((bx, by));
        }
    }
    set
}

fn bresenham<F>(x0: i32, y0: i32, x1: i32, y1: i32, max_x: i32, max_y: i32, mut emit: F)
where
    F: FnMut(i32, i32),
{
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx = if x0 < x1 { 1i32 } else { -1 };
    let sy = if y0 < y1 { 1i32 } else { -1 };
    let mut err = dx - dy;
    let (mut x, mut y) = (x0, y0);
    loop {
        if x >= 0 && y >= 0 && x < max_x && y < max_y {
            emit(x, y);
        }
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 > -dy {
            err -= dy;
            x += sx;
        }
        if e2 < dx {
            err += dx;
            y += sy;
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// DCT followed by IDCT should reproduce the original signal (round-trip).
    #[test]
    fn dct_roundtrip() {
        let mut block = [[0.0f32; 8]; 8];
        // Fill with arbitrary values
        for (r, row) in block.iter_mut().enumerate() {
            for (c, cell) in row.iter_mut().enumerate() {
                *cell = ((r * 17 + c * 31) % 200) as f32;
            }
        }
        let original = block;
        dct8x8_forward(&mut block);
        dct8x8_inverse(&mut block);
        for r in 0..8 {
            for c in 0..8 {
                assert!(
                    (block[r][c] - original[r][c]).abs() < 0.01,
                    "round-trip error at ({},{}) original={} got={}",
                    r,
                    c,
                    original[r][c],
                    block[r][c]
                );
            }
        }
    }

    /// Modifying TARGET coefficient by EMBED_DELTA should change spatial pixels by < 2/255 max.
    #[test]
    fn dct_perturbation_is_invisible() {
        let mut block_orig = [[128.0f32; 8]; 8];
        // Slight variation so DC isn't the only component
        for (r, row) in block_orig.iter_mut().enumerate() {
            for (c, cell) in row.iter_mut().enumerate() {
                *cell = 100.0 + (r * 5 + c * 3) as f32;
            }
        }
        let mut block_wm = block_orig;

        // Embed
        dct8x8_forward(&mut block_wm);
        block_wm[TARGET_U][TARGET_V] += EMBED_DELTA;
        dct8x8_inverse(&mut block_wm);

        // Reconstruct original
        dct8x8_forward(&mut block_orig);
        dct8x8_inverse(&mut block_orig);

        let mut max_diff = 0.0f32;
        for r in 0..8 {
            for c in 0..8 {
                let d = (block_wm[r][c] - block_orig[r][c]).abs();
                if d > max_diff {
                    max_diff = d;
                }
            }
        }
        // Math: max spatial change = EMBED_DELTA × α_u × α_v × max|cos(u,v)|
        //   = 16 × sqrt(2/8) × sqrt(2/8) × max_cos ≈ 16 × 0.25 × ~0.924 ≈ 3.7 (of 255 range)
        // 3.7/255 ≈ 1.4% per channel — sub-perceptual in textured natural images.
        // JPEG quantization step at quality=75 for this position ≈ 6, giving 2.7× margin.
        assert!(
            max_diff < 5.0,
            "spatial perturbation {:.4} too large (limit 5.0 / 255)",
            max_diff
        );
    }
}
