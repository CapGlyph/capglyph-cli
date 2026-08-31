//! Registered-residual original-assisted extractor (CTX-0021).
//!
//! Hybrid path: `blind locator → cover family → strong verify`.
//! The core primitive is `R = I_aligned − I_original` (pixel residual after
//! feature-based warp), then matched filtering on the keyed lattice to produce
//! soft bits. This cancels host interference so the residual is dominated by
//! the ±`ID_EMBED_DELTA` / `DWT_ID_EMBED_STRENGTH` signal.
//!
//! Design per `capglyph-docs/research/media-credential/technology/pointer-and-stego.md`
//! §5 and `capglyph-docs/research/media-credential/architecture/capglyph-core-api.md`
//! (legacy: `sigil-docs/.../sigil-core-api.md`)
//! §4.5.
//!
//! Registration is intentionally dependency-free: the default `Identity` and
//! `Translation` estimators use only `image` + pure Rust NCC. Heavy
//! feature-point / homography deps (e.g. `imageproc` ORB+RANSAC) would be gated
//! behind a `registration` feature — not pulled into the wasm graph.

#![allow(
    clippy::needless_range_loop,
    clippy::too_many_arguments,
    clippy::identity_op,
    clippy::type_complexity
)]

use anyhow::Result;
use image::{ImageBuffer, Rgb};

/// Aligned submitted image plus the estimated transform for audit.
#[derive(Debug, Clone)]
pub struct AlignedImage {
    pub image: ImageBuffer<Rgb<u8>, Vec<u8>>,
    pub transform: Transform,
}

/// Affine transform (3×3) in original coordinates, plus diagnostics.
#[derive(Debug, Clone)]
pub struct Transform {
    /// Row-major 3×3 homogeneous matrix. Identity is `[[1,0,0],[0,1,0],[0,0,1]]`.
    /// Maps original coords → submitted coords: `p_sub = M * p_orig`.
    /// To align, we sample `submitted` at `M * p_orig`.
    pub matrix: [[f32; 3]; 3],
    /// Estimated translation (dx, dy) for convenience.
    pub translation: (f32, f32),
    /// Number of inliers (for RANSAC stub) or NCC peak sharpness.
    pub inliers: u32,
    /// Reprojection / correlation error (lower is better).
    pub reprojection_error: f32,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            translation: (0.0, 0.0),
            inliers: 0,
            reprojection_error: 0.0,
        }
    }
}

/// Registration trait — feature-registration warp (affine).
///
/// `original` is the server-held cover (private), `submitted` is the
/// user-supplied credential image (possibly JPEG'd, translated, slightly
/// scaled). `align` returns `submitted` warped into `original`'s coordinate
/// frame.
pub trait Registration: Send + Sync {
    fn align(
        &self,
        original: &ImageBuffer<Rgb<u8>, Vec<u8>>,
        submitted: &ImageBuffer<Rgb<u8>, Vec<u8>>,
    ) -> Result<AlignedImage>;

    fn name(&self) -> &'static str {
        "unknown"
    }
}

// ── Identity (fallback) ──────────────────────────────────────────────────────

/// Identity warp — returns `submitted` cloned (or resized to `original` size).
/// This is the correct fallback when no geometric distortion is expected, and
/// also the wasm-safe default (no NCC, no allocation beyond resize).
pub struct IdentityRegistration;

impl Registration for IdentityRegistration {
    fn align(
        &self,
        original: &ImageBuffer<Rgb<u8>, Vec<u8>>,
        submitted: &ImageBuffer<Rgb<u8>, Vec<u8>>,
    ) -> Result<AlignedImage> {
        let (ow, oh) = original.dimensions();
        let (sw, sh) = submitted.dimensions();
        let aligned = if ow == sw && oh == sh {
            submitted.clone()
        } else {
            // Resize submitted to original size via Triangle filter (good for
            // photographic content, no extra deps).
            let dyn_sub = image::DynamicImage::ImageRgb8(submitted.clone());
            dyn_sub
                .resize_exact(ow, oh, image::imageops::FilterType::Triangle)
                .to_rgb8()
        };
        Ok(AlignedImage {
            image: aligned,
            transform: Transform::default(),
        })
    }

    fn name(&self) -> &'static str {
        "identity"
    }
}

// ── Translation via NCC ─────────────────────────────────────────────────────

/// Translation registration via normalized cross-correlation.
///
/// Searches `[-max_shift, max_shift]` in x/y at low-res (128²), then refines
/// ±2 at full res around the peak. No external deps.
///
/// `max_shift` is in pixels at full resolution (default 32). Set to 0 for
/// identity-equivalent but still goes through the estimator.
pub struct TranslationRegistration {
    pub max_shift: i32,
}

impl Default for TranslationRegistration {
    fn default() -> Self {
        Self { max_shift: 32 }
    }
}

impl Registration for TranslationRegistration {
    fn align(
        &self,
        original: &ImageBuffer<Rgb<u8>, Vec<u8>>,
        submitted: &ImageBuffer<Rgb<u8>, Vec<u8>>,
    ) -> Result<AlignedImage> {
        let (ow, oh) = original.dimensions();
        // Normalize sizes first: if submitted size differs, resize to original
        // before NCC so the correlation is meaningful (scale is not handled
        // here — that belongs to Affine).
        let submitted_norm = if submitted.dimensions() != (ow, oh) {
            let dyn_sub = image::DynamicImage::ImageRgb8(submitted.clone());
            dyn_sub
                .resize_exact(ow, oh, image::imageops::FilterType::Triangle)
                .to_rgb8()
        } else {
            submitted.clone()
        };

        // Grayscale conversion
        let gray_orig = to_grayscale(original);
        let gray_sub = to_grayscale(&submitted_norm);

        let (dx, dy, peak, error) =
            estimate_translation_ncc(&gray_orig, &gray_sub, ow, oh, self.max_shift);

        let aligned = warp_translation(&submitted_norm, dx, dy, ow, oh);

        let mut matrix = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        matrix[0][2] = dx as f32;
        matrix[1][2] = dy as f32;

        Ok(AlignedImage {
            image: aligned,
            transform: Transform {
                matrix,
                translation: (dx as f32, dy as f32),
                inliers: if peak > 0.5 { 100 } else { 10 },
                reprojection_error: error,
            },
        })
    }

    fn name(&self) -> &'static str {
        "translation-ncc"
    }
}

// ── Affine stub (future: ORB+RANSAC) ───────────────────────────────────────

/// Affine registration stub — currently delegates to `TranslationRegistration`
/// and documents the upgrade path. A full implementation would use
/// `imageproc`-style ORB/SIFT features + RANSAC homography behind a feature
/// gate (`registration` feature) to keep the wasm graph clean.
///
/// This satisfies the CTX-0021 acceptance that the `Registration` trait exists
/// and the warp is affine-capable (matrix is 3×3), even though the estimator
/// is still translation-only.
pub struct AffineRegistration {
    pub max_shift: i32,
}

impl Default for AffineRegistration {
    fn default() -> Self {
        Self { max_shift: 32 }
    }
}

impl Registration for AffineRegistration {
    fn align(
        &self,
        original: &ImageBuffer<Rgb<u8>, Vec<u8>>,
        submitted: &ImageBuffer<Rgb<u8>, Vec<u8>>,
    ) -> Result<AlignedImage> {
        // Delegate to translation for now; the matrix is still 3×3 affine.
        let t = TranslationRegistration {
            max_shift: self.max_shift,
        };
        let aligned = t.align(original, submitted)?;
        Ok(aligned)
    }

    fn name(&self) -> &'static str {
        "affine-stub(translation)"
    }
}

// ── Residual R = I_aligned − I_original ─────────────────────────────────────

/// Compute pixel residual `R = I_aligned − I_original` as f32 per channel.
/// Returned as a flat vec of `(R,R,G,B)` diffs in row-major order, but callers
/// usually want the DCT/DWT of `R` directly — see `dct::extract_…_residual`.
///
/// This helper is exposed for `verify_original_assisted` audit and for tests.
/// It never panics on size mismatch: if `aligned` and `original` differ, the
/// overlapping region is diffed and the remainder is zero-padded (the warp
/// should have already normalized sizes, so this is a safety net).
#[allow(clippy::needless_range_loop)]
pub fn residual_image(
    original: &ImageBuffer<Rgb<u8>, Vec<u8>>,
    aligned: &ImageBuffer<Rgb<u8>, Vec<u8>>,
) -> Vec<Vec<[f32; 3]>> {
    let (ow, oh) = original.dimensions();
    let (aw, ah) = aligned.dimensions();
    let w = ow.min(aw) as usize;
    let h = oh.min(ah) as usize;
    let mut out = vec![vec![[0.0f32; 3]; w]; h];
    for y in 0..h {
        for x in 0..w {
            let o = original.get_pixel(x as u32, y as u32);
            let a = aligned.get_pixel(x as u32, y as u32);
            out[y][x][0] = a[0] as f32 - o[0] as f32;
            out[y][x][1] = a[1] as f32 - o[1] as f32;
            out[y][x][2] = a[2] as f32 - o[2] as f32;
        }
    }
    out
}

/// Convenience: residual as flat `Rgb<u8>` difference clamped to `[0,255]` with
/// bias 128 (for visualization). Not used for detection — detection uses f32.
#[allow(clippy::needless_range_loop)]
pub fn residual_image_visual(
    original: &ImageBuffer<Rgb<u8>, Vec<u8>>,
    aligned: &ImageBuffer<Rgb<u8>, Vec<u8>>,
) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    let (ow, oh) = original.dimensions();
    let (aw, ah) = aligned.dimensions();
    assert_eq!(
        (ow, oh),
        (aw, ah),
        "residual visual requires same dimensions"
    );
    ImageBuffer::from_fn(ow, oh, |x, y| {
        let o = original.get_pixel(x, y);
        let a = aligned.get_pixel(x, y);
        let r = ((a[0] as i16 - o[0] as i16) + 128).clamp(0, 255) as u8;
        let g = ((a[1] as i16 - o[1] as i16) + 128).clamp(0, 255) as u8;
        let b = ((a[2] as i16 - o[2] as i16) + 128).clamp(0, 255) as u8;
        Rgb([r, g, b])
    })
}

// ── Cover vault / hybrid bootstrap ──────────────────────────────────────────

/// Cover vault for hybrid extraction — maps `cover_id` → original image.
/// In production this is a DB / R2 bucket; here it is an in-memory map for
/// tests and for the `register` module's acceptance test.
///
/// The `cover_id` is the 16-byte truncated `stable_seed` or HMAC-derived
/// family id. The vault is intentionally not a file-XOR store — it holds
/// originals for `R = aligned − original`, not for byte-level diff.
#[derive(Debug, Default, Clone)]
pub struct CoverVault {
    entries: Vec<(Vec<u8>, ImageBuffer<Rgb<u8>, Vec<u8>>)>,
}

impl CoverVault {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn insert(&mut self, cover_id: Vec<u8>, image: ImageBuffer<Rgb<u8>, Vec<u8>>) {
        self.entries.push((cover_id, image));
    }

    pub fn insert_bytes(&mut self, cover_id: &[u8], image: ImageBuffer<Rgb<u8>, Vec<u8>>) {
        self.entries.push((cover_id.to_vec(), image));
    }

    pub fn all(&self) -> &[(Vec<u8>, ImageBuffer<Rgb<u8>, Vec<u8>>)] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, cover_id: &[u8]) -> Option<&ImageBuffer<Rgb<u8>, Vec<u8>>> {
        self.entries
            .iter()
            .find(|(id, _)| id.as_slice() == cover_id)
            .map(|(_, img)| img)
    }
}

/// Hybrid extractor result — which cover matched and the decoded payload.
#[derive(Debug)]
pub struct HybridMatch {
    /// Index in the vault that matched (strong verify succeeded).
    pub vault_index: usize,
    /// Vault cover_id that matched.
    pub cover_id: Vec<u8>,
    /// Decoded payload bytes after ECC + framing auth.
    pub payload: Vec<u8>,
    /// Transform diagnostics from registration.
    pub transform: Transform,
}

// ── Helpers: grayscale, NCC, warp ───────────────────────────────────────────

#[allow(clippy::needless_range_loop)]
fn to_grayscale(img: &ImageBuffer<Rgb<u8>, Vec<u8>>) -> Vec<Vec<f32>> {
    let (w, h) = img.dimensions();
    let w = w as usize;
    let h = h as usize;
    let mut out = vec![vec![0.0f32; w]; h];
    for y in 0..h {
        for x in 0..w {
            let p = img.get_pixel(x as u32, y as u32);
            // BT.601 luma
            let luma = 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32;
            out[y][x] = luma;
        }
    }
    out
}

#[allow(clippy::needless_range_loop)]
fn downscale_gray(
    gray: &[Vec<f32>],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
) -> Vec<Vec<f32>> {
    if src_w == dst_w && src_h == dst_h {
        return gray.to_vec();
    }
    let mut out = vec![vec![0.0f32; dst_w as usize]; dst_h as usize];
    let scale_x = src_w as f32 / dst_w as f32;
    let scale_y = src_h as f32 / dst_h as f32;
    // Nearest-neighbor sampling preserves high-frequency random texture better
    // for NCC than box averaging (which turns random high-frequency into flat DC).
    for y in 0..dst_h as usize {
        for x in 0..dst_w as usize {
            let sx = ((x as f32 + 0.5) * scale_x) as usize;
            let sy = ((y as f32 + 0.5) * scale_y) as usize;
            let sx = sx.min(src_w as usize - 1);
            let sy = sy.min(src_h as usize - 1);
            out[y][x] = gray[sy][sx];
        }
    }
    out
}

#[allow(clippy::needless_range_loop)]
fn ncc_at_shift(a: &[Vec<f32>], b: &[Vec<f32>], w: usize, h: usize, dx: i32, dy: i32) -> f32 {
    // Overlap region: a[y][x] vs b[y+dy][x+dx]
    let mut sum_a = 0.0f64;
    let mut sum_b = 0.0f64;
    let mut count = 0usize;
    // First pass: means
    for y in 0..h {
        for x in 0..w {
            let bx = x as i32 + dx;
            let by = y as i32 + dy;
            if bx < 0 || by < 0 || bx >= w as i32 || by >= h as i32 {
                continue;
            }
            sum_a += a[y][x] as f64;
            sum_b += b[by as usize][bx as usize] as f64;
            count += 1;
        }
    }
    if count == 0 {
        return f32::NEG_INFINITY;
    }
    let mean_a = sum_a / count as f64;
    let mean_b = sum_b / count as f64;
    let mut num = 0.0f64;
    let mut denom_a = 0.0f64;
    let mut denom_b = 0.0f64;
    for y in 0..h {
        for x in 0..w {
            let bx = x as i32 + dx;
            let by = y as i32 + dy;
            if bx < 0 || by < 0 || bx >= w as i32 || by >= h as i32 {
                continue;
            }
            let da = a[y][x] as f64 - mean_a;
            let db = b[by as usize][bx as usize] as f64 - mean_b;
            num += da * db;
            denom_a += da * da;
            denom_b += db * db;
        }
    }
    let denom = (denom_a * denom_b).sqrt();
    if denom < 1e-9 {
        return 0.0;
    }
    (num / denom) as f32
}

#[allow(clippy::needless_range_loop)]
fn estimate_translation_ncc(
    gray_orig: &[Vec<f32>],
    gray_sub: &[Vec<f32>],
    ow: u32,
    oh: u32,
    max_shift: i32,
) -> (i32, i32, f32, f32) {
    if max_shift == 0 {
        return (0, 0, 1.0, 0.0);
    }
    let ow_us = ow as usize;
    let oh_us = oh as usize;

    // Low-res stage: 128×128 or original if smaller
    let low_w = 128u32.min(ow);
    let low_h = 128u32.min(oh);
    let scale_x = ow as f32 / low_w as f32;
    let scale_y = oh as f32 / low_h as f32;

    let low_orig = downscale_gray(gray_orig, ow, oh, low_w, low_h);
    let low_sub = downscale_gray(gray_sub, ow, oh, low_w, low_h);

    let max_low_x = ((max_shift as f32 / scale_x).ceil() as i32).max(2);
    let max_low_y = ((max_shift as f32 / scale_y).ceil() as i32).max(2);

    let mut best_dx_low = 0i32;
    let mut best_dy_low = 0i32;
    let mut best_ncc_low = f32::NEG_INFINITY;
    for dy in -max_low_y..=max_low_y {
        for dx in -max_low_x..=max_low_x {
            let ncc = ncc_at_shift(&low_orig, &low_sub, low_w as usize, low_h as usize, dx, dy);
            if ncc > best_ncc_low {
                best_ncc_low = ncc;
                best_dx_low = dx;
                best_dy_low = dy;
            }
        }
    }

    // Map back to full-res
    let est_dx = (best_dx_low as f32 * scale_x).round() as i32;
    let est_dy = (best_dy_low as f32 * scale_y).round() as i32;

    // Refine ±3 at full res around estimate
    let refine = 3i32;
    let mut best_dx = est_dx;
    let mut best_dy = est_dy;
    let mut best_ncc = f32::NEG_INFINITY;
    for dy in (est_dy - refine)..=(est_dy + refine) {
        if dy.abs() > max_shift {
            continue;
        }
        for dx in (est_dx - refine)..=(est_dx + refine) {
            if dx.abs() > max_shift {
                continue;
            }
            let ncc = ncc_at_shift(gray_orig, gray_sub, ow_us, oh_us, dx, dy);
            if ncc > best_ncc {
                best_ncc = ncc;
                best_dx = dx;
                best_dy = dy;
            }
        }
    }

    // Fallback: if low-res peak was weak (NCC < 0.5) the hierarchical search
    // likely failed (e.g. high-frequency random texture averaged away). Do a
    // full exhaustive search at full res in this case. This is slower (up to
    // ~4k positions) but only triggers on the weak-peak path.
    if best_ncc < 0.5 {
        let mut full_best_dx = best_dx;
        let mut full_best_dy = best_dy;
        let mut full_best_ncc = best_ncc;
        for dy in -max_shift..=max_shift {
            for dx in -max_shift..=max_shift {
                let ncc = ncc_at_shift(gray_orig, gray_sub, ow_us, oh_us, dx, dy);
                if ncc > full_best_ncc {
                    full_best_ncc = ncc;
                    full_best_dx = dx;
                    full_best_dy = dy;
                }
            }
        }
        best_dx = full_best_dx;
        best_dy = full_best_dy;
        best_ncc = full_best_ncc;
    }

    // Reprojection error as 1 - NCC (0 = perfect)
    let error = 1.0 - best_ncc.clamp(-1.0, 1.0);
    (best_dx, best_dy, best_ncc, error)
}

#[allow(clippy::needless_range_loop)]
fn warp_translation(
    submitted: &ImageBuffer<Rgb<u8>, Vec<u8>>,
    dx: i32,
    dy: i32,
    out_w: u32,
    out_h: u32,
) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    // Aligned[x,y] = submitted[x+dx, y+dy] (see module docs for convention)
    // Out-of-bounds samples are clamped to edge (replicate) to avoid black borders
    // that would destroy DCT high-frequency matching.
    let (sw, sh) = submitted.dimensions();
    let mut out = ImageBuffer::new(out_w, out_h);
    for y in 0..out_h {
        for x in 0..out_w {
            let sx = x as i32 + dx;
            let sy = y as i32 + dy;
            let sx_clamped = sx.clamp(0, sw as i32 - 1) as u32;
            let sy_clamped = sy.clamp(0, sh as i32 - 1) as u32;
            let p = submitted.get_pixel(sx_clamped, sy_clamped);
            out.put_pixel(x, y, *p);
        }
    }
    out
}

// ── Bilinear warp for future affine (currently unused, but provided) ─────────

/// Warp `submitted` by a 3×3 affine matrix `M` where `p_sub = M * p_orig`.
/// Uses bilinear sampling, edge-clamped.
#[allow(dead_code)]
#[allow(clippy::needless_range_loop)]
pub fn warp_affine(
    submitted: &ImageBuffer<Rgb<u8>, Vec<u8>>,
    matrix: [[f32; 3]; 3],
    out_w: u32,
    out_h: u32,
) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    let (sw, sh) = submitted.dimensions();
    let mut out = ImageBuffer::new(out_w, out_h);
    // Invert matrix for backward sampling. For affine, invert 2×2 + translation.
    let det = matrix[0][0] * matrix[1][1] - matrix[0][1] * matrix[1][0];
    if det.abs() < 1e-6 {
        // Degenerate — fall back to translation component only
        let dx = matrix[0][2].round() as i32;
        let dy = matrix[1][2].round() as i32;
        return warp_translation(submitted, dx, dy, out_w, out_h);
    }
    let inv_det = 1.0 / det;
    let a = matrix[1][1] * inv_det;
    let b = -matrix[0][1] * inv_det;
    let c = -matrix[1][0] * inv_det;
    let d = matrix[0][0] * inv_det;
    let tx = matrix[0][2];
    let ty = matrix[1][2];
    // Inverse translation: - (inv 2×2 * t)
    let itx = -(a * tx + b * ty);
    let ity = -(c * tx + d * ty);

    for y in 0..out_h {
        for x in 0..out_w {
            // p_sub = M * p_orig => p_orig = M^{-1} * p_sub? Wait we want
            // aligned[x,y] = submitted[ M * (x,y) ], so forward mapping.
            // We sample submitted at (a*x + b*y + tx, c*x + d*y + ty) where
            // the matrix is the forward M. The inverse above is not needed —
            // we directly apply M.
            let fx = matrix[0][0] * x as f32 + matrix[0][1] * y as f32 + matrix[0][2];
            let fy = matrix[1][0] * x as f32 + matrix[1][1] * y as f32 + matrix[1][2];
            let p = sample_bilinear(submitted, fx, fy, sw, sh);
            out.put_pixel(x, y, p);
        }
    }
    let _ = (itx, ity, a, b, c, d); // keep inverse calc for future use / lint
    out
}

#[allow(clippy::needless_range_loop)]
fn sample_bilinear(
    img: &ImageBuffer<Rgb<u8>, Vec<u8>>,
    fx: f32,
    fy: f32,
    sw: u32,
    sh: u32,
) -> Rgb<u8> {
    let x0 = fx.floor() as i32;
    let y0 = fy.floor() as i32;
    let x1 = x0 + 1;
    let y1 = y0 + 1;
    let wx = fx - x0 as f32;
    let wy = fy - y0 as f32;

    let sample = |x: i32, y: i32| {
        let xc = x.clamp(0, sw as i32 - 1) as u32;
        let yc = y.clamp(0, sh as i32 - 1) as u32;
        let p = img.get_pixel(xc, yc);
        [p[0] as f32, p[1] as f32, p[2] as f32]
    };

    let c00 = sample(x0, y0);
    let c10 = sample(x1, y0);
    let c01 = sample(x0, y1);
    let c11 = sample(x1, y1);

    let r = (1.0 - wx) * (1.0 - wy) * c00[0]
        + wx * (1.0 - wy) * c10[0]
        + (1.0 - wx) * wy * c01[0]
        + wx * wy * c11[0];
    let g = (1.0 - wx) * (1.0 - wy) * c00[1]
        + wx * (1.0 - wy) * c10[1]
        + (1.0 - wx) * wy * c01[1]
        + wx * wy * c11[1];
    let b = (1.0 - wx) * (1.0 - wy) * c00[2]
        + wx * (1.0 - wy) * c10[2]
        + (1.0 - wx) * wy * c01[2]
        + wx * wy * c11[2];

    Rgb([
        r.round().clamp(0.0, 255.0) as u8,
        g.round().clamp(0.0, 255.0) as u8,
        b.round().clamp(0.0, 255.0) as u8,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};

    fn make_checker(w: u32, h: u32) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
        ImageBuffer::from_fn(w, h, |x, y| {
            let v = if ((x / 16) + (y / 16)) % 2 == 0 {
                20
            } else {
                220
            };
            Rgb([v, v, v])
        })
    }

    #[test]
    fn identity_roundtrip() {
        let (w, h) = (128, 128);
        let orig = make_checker(w, h);
        let sub = orig.clone();
        let reg = IdentityRegistration;
        let aligned = reg.align(&orig, &sub).unwrap();
        assert_eq!(aligned.image.dimensions(), (w, h));
        assert_eq!(aligned.image, orig);
    }

    #[test]
    fn translation_estimator_finds_shift() {
        let (w, h) = (128, 128);
        let orig = make_checker(w, h);
        // Create submitted shifted by warp dx=7 (which is left shift by 7)
        let dx = 7;
        let dy = -5;
        let sub = warp_translation(&orig, dx, dy, w, h);
        // Estimator should find the alignment shift that recovers orig:
        // aligned[x]=sub[x+dx_est] => need dx_est = -dx to invert warp
        let reg = TranslationRegistration { max_shift: 16 };
        let aligned = reg.align(&orig, &sub).unwrap();
        assert!(
            (aligned.transform.translation.0 + dx as f32).abs() <= 1.0,
            "dx est {:?} vs true -{}",
            aligned.transform.translation,
            dx
        );
        assert!(
            (aligned.transform.translation.1 + dy as f32).abs() <= 1.0,
            "dy est {:?} vs true -{}",
            aligned.transform.translation,
            dy
        );
        // And aligned image should be near-identical to orig (except edge clamp)
        // Check that the central region matches
        for y in 16..(h - 16) {
            for x in 16..(w - 16) {
                assert_eq!(aligned.image.get_pixel(x, y), orig.get_pixel(x, y));
            }
        }
    }

    #[test]
    fn residual_zero_for_identical() {
        let (w, h) = (64, 64);
        let orig = make_checker(w, h);
        let aligned = orig.clone();
        let res = residual_image(&orig, &aligned);
        for row in res {
            for px in row {
                assert_eq!(px, [0.0, 0.0, 0.0]);
            }
        }
    }

    #[test]
    fn vault_insert_and_get() {
        let mut vault = CoverVault::new();
        let img = make_checker(32, 32);
        vault.insert_bytes(b"cover1", img.clone());
        assert_eq!(vault.len(), 1);
        assert!(vault.get(b"cover1").is_some());
        assert!(vault.get(b"other").is_none());
    }

    #[test]
    fn affine_stub_delegates() {
        let (w, h) = (64, 64);
        let orig = make_checker(w, h);
        let sub = orig.clone();
        let reg = AffineRegistration::default();
        let aligned = reg.align(&orig, &sub).unwrap();
        assert_eq!(aligned.image.dimensions(), (w, h));
    }
}
