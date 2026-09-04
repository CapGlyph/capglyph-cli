//! DWT-based watermark embedding and verification.
//!
//! Re-exports core DWT carrier primitives and provides backward-compatible
//! wrappers for legacy CLI `PlacementStrategy` call sites.

pub use capglyph_core::dwt_embed::*;

use anyhow::Result;
use image::{ImageBuffer, Rgb};

use crate::geometry::GeometryFile;

/// Backward-compatible wrapper converting legacy CLI PlacementStrategy to core Placement.
pub fn embed(
    img: &mut ImageBuffer<Rgb<u8>, Vec<u8>>,
    geometry: &GeometryFile,
    recipient_id: Option<&str>,
    secret_key: Option<&str>,
    placement: &crate::cli::PlacementStrategy,
) -> Result<(u64, Vec<(u32, u32)>)> {
    let p = crate::carrier::to_core_placement(placement);
    capglyph_core::dwt_embed::embed(img, geometry, recipient_id, secret_key, &p)
}

/// Backward-compatible wrapper converting legacy CLI PlacementStrategy to core Placement.
pub fn embed_with_strength(
    img: &mut ImageBuffer<Rgb<u8>, Vec<u8>>,
    geometry: &GeometryFile,
    recipient_id: Option<&str>,
    secret_key: Option<&str>,
    placement: &crate::cli::PlacementStrategy,
    strength: f32,
) -> Result<(u64, Vec<(u32, u32)>)> {
    let p = crate::carrier::to_core_placement(placement);
    capglyph_core::dwt_embed::embed_with_strength(
        img,
        geometry,
        recipient_id,
        secret_key,
        &p,
        strength,
    )
}

/// Backward-compatible wrapper converting legacy CLI PlacementStrategy to core Placement.
pub fn verify(
    img: &ImageBuffer<Rgb<u8>, Vec<u8>>,
    geometry: &GeometryFile,
    placement: &crate::cli::PlacementStrategy,
) -> Result<DwtSignalMetrics> {
    let p = crate::carrier::to_core_placement(placement);
    capglyph_core::dwt_embed::verify(img, geometry, &p)
}

/// Backward-compatible wrapper converting legacy CLI PlacementStrategy to core Placement.
pub fn verify_v2(
    img: &ImageBuffer<Rgb<u8>, Vec<u8>>,
    geometry: &GeometryFile,
    placement: &crate::cli::PlacementStrategy,
) -> Result<DwtSignalMetrics> {
    let p = crate::carrier::to_core_placement(placement);
    capglyph_core::dwt_embed::verify_v2(img, geometry, &p)
}

/// Backward-compatible wrapper converting legacy CLI PlacementStrategy to core Placement.
pub fn embed_coded_bits(
    img: &mut ImageBuffer<Rgb<u8>, Vec<u8>>,
    geometry: &GeometryFile,
    coded_bits: &[bool],
    keys: &crate::keying::KeyMaterial,
    placement: &crate::cli::PlacementStrategy,
) -> Result<(u64, Vec<(u32, u32)>)> {
    let p = crate::carrier::to_core_placement(placement);
    capglyph_core::dwt_embed::embed_coded_bits(img, geometry, coded_bits, keys, &p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{AnalysisParams, GeometryFile, PathEntry};

    fn make_test_geometry(w: u32, h: u32) -> GeometryFile {
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

        let (n, _positions) = embed(
            &mut img,
            &geo,
            None,
            None,
            &crate::cli::PlacementStrategy::Skeleton,
        )
        .unwrap();
        assert!(n > 0, "Expected some coefficients to be modified");

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
        let (w, h) = (512u32, 512u32);
        let mut img = ImageBuffer::from_fn(w, h, |x, y| {
            let v = ((x * 3 + y * 7 + x * y) % 255) as u8;
            Rgb([v, (v as u32 + 40) as u8 % 255, (v as u32 + 80) as u8 % 255])
        });

        let points: Vec<[f64; 2]> = (0..50)
            .map(|i| {
                let t = i as f64 / 50.0;
                [
                    t * w as f64,
                    (t * 2.0 * std::f64::consts::PI).sin() * 50.0 + (h as f64 / 2.0),
                ]
            })
            .collect();

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
            paths: vec![PathEntry {
                color: None,
                points,
            }],
            prng_seed: None,
            blocks: None,
        };

        let rid = "hi";
        let (n, _positions) = embed(
            &mut img,
            &geo,
            Some(rid),
            None,
            &crate::cli::PlacementStrategy::Skeleton,
        )
        .unwrap();
        assert!(n > 0, "No coefficients modified");

        assert_eq!(extract_recipient_id(&img, rid.len()).unwrap(), rid);
    }
}
