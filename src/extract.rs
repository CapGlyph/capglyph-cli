//! `sigil extract` — recover embedded recipient ID from a watermarked image.
//!
//! Reads the spread-spectrum bit stream embedded during `sigil embed --recipient-id`
//! and reconstructs the original ID string.

use anyhow::{Context, Result};
use tracing::info;

use crate::cli::ExtractArgs;
use crate::geometry::GeometryFile;

/// Entry point for the `extract` subcommand.
/// Returns the decoded recipient ID string.
pub fn run(args: &ExtractArgs) -> Result<String> {
    info!("Extracting recipient ID from: {:?}", args.input);

    let img = image::open(&args.input)
        .with_context(|| format!("Failed to open image: {:?}", args.input))?;

    let geometry = match &args.geometry {
        Some(p) => {
            let json = std::fs::read_to_string(p)
                .with_context(|| format!("Failed to read geometry file: {:?}", p))?;
            serde_json::from_str::<GeometryFile>(&json).context("Failed to parse geometry JSON")?
        }
        None => {
            info!("No geometry file — re-extracting skeleton from image");
            crate::verify::extract_geometry_from_image(&img)?
        }
    };

    let rgb = img.to_rgb8();
    let (w, h) = rgb.dimensions();

    match args.mode {
        crate::cli::EmbedMode::Dct => extract_from_dct(&rgb, &geometry, args.id_length, w, h),
        crate::cli::EmbedMode::Dwt => extract_from_dwt(&rgb, &geometry, args.id_length, w, h),
        crate::cli::EmbedMode::Alpha => {
            anyhow::bail!("extract is not supported for alpha mode (alpha mode does not carry recoverable bits)")
        }
    }
}

fn extract_from_dct(
    img: &image::RgbImage,
    geometry: &GeometryFile,
    id_length: usize,
    _w: u32,
    _h: u32,
) -> Result<String> {
    use crate::dct::{dct8x8_forward, extract_block, prng_blocks, ID_TARGET_U, ID_TARGET_V};
    use crate::spread_spectrum::{bits_to_str, REDUNDANCY};

    let (iw, ih) = img.dimensions();

    // Use stored blocks if available (exact embed order), otherwise re-extract from paths
    let blocks: Vec<(u32, u32)> = if let Some(ref stored_blocks) = geometry.blocks {
        stored_blocks.clone()
    } else {
        // Fallback: re-extract skeleton blocks from geometry paths
        let mut skeleton_blocks: std::collections::HashSet<(u32, u32)> =
            std::collections::HashSet::new();
        for path in &geometry.paths {
            for point in &path.points {
                let (px, py) = (point[0] as u32, point[1] as u32);
                let bx = px / 8;
                let by = py / 8;
                if (bx + 1) * 8 <= iw && (by + 1) * 8 <= ih {
                    skeleton_blocks.insert((bx, by));
                }
            }
        }

        if skeleton_blocks.is_empty() {
            // Fallback: PRNG blocks
            let mut v: Vec<_> = prng_blocks(img, iw, ih, None).into_iter().collect();
            v.sort_unstable();
            v
        } else {
            let mut v: Vec<_> = skeleton_blocks.into_iter().collect();
            v.sort_unstable();
            v
        }
    };

    let bits_needed = id_length * 8 * REDUNDANCY;
    if blocks.len() < bits_needed {
        anyhow::bail!(
            "Insufficient blocks ({}) to extract {} bytes with redundancy={}",
            blocks.len(),
            id_length,
            REDUNDANCY
        );
    }

    // Extract ID coefficients from ID_TARGET position
    let id_coeffs: Vec<f32> = blocks
        .iter()
        .take(bits_needed)
        .map(|(bx, by)| {
            let mut sum = 0.0f32;
            for ch in 0..3 {
                let mut block = extract_block(img, bx * 8, by * 8, ch);
                dct8x8_forward(&mut block);
                sum += block[ID_TARGET_U][ID_TARGET_V];
            }
            sum / 3.0
        })
        .collect();

    // Decode using differential pair encoding
    let mut decoded_bits = Vec::new();
    for bit_idx in 0..(id_length * 8) {
        let start = bit_idx * REDUNDANCY;
        let end = (start + REDUNDANCY).min(id_coeffs.len());

        // Majority vote across redundancy group
        let mut ones = 0;
        let mut zeros = 0;
        for &coeff in &id_coeffs[start..end] {
            if coeff > 0.0 {
                ones += 1;
            } else {
                zeros += 1;
            }
        }
        decoded_bits.push(ones > zeros);
    }

    let decoded = bits_to_str(&decoded_bits)?;
    Ok(decoded)
}

fn extract_from_dwt(
    img: &image::RgbImage,
    geometry: &GeometryFile,
    id_length: usize,
    w: u32,
    h: u32,
) -> Result<String> {
    use crate::dwt::haar_2d_forward;
    use crate::dwt_embed::EMBED_BAND;

    let positions = geometry.blocks.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "No block coordinates in geometry file. Re-run embed with --recipient-id to generate them."
        )
    })?;

    let bit_count = id_length * 8;
    let redundancy = crate::spread_spectrum::REDUNDANCY;
    let bits_needed = bit_count * redundancy;

    if positions.len() < bits_needed {
        anyhow::bail!(
            "Not enough DWT positions for ID extraction: need {}, have {}",
            bits_needed,
            positions.len()
        );
    }

    // Average over 3 channels
    let mut bit_signals: Vec<f32> = vec![0.0; bits_needed];

    for ch in 0..3usize {
        let channel_matrix: Vec<Vec<f32>> = (0..h)
            .map(|y| (0..w).map(|x| img.get_pixel(x, y)[ch] as f32).collect())
            .collect();
        let decomp = haar_2d_forward(&channel_matrix)?;
        let band = decomp.band(EMBED_BAND);
        let (bh, bw) = (band.len(), band[0].len());

        for (i, &(bx, by)) in positions.iter().enumerate().take(bits_needed) {
            let bx = bx as usize;
            let by = by as usize;
            if bx < bw && by < bh {
                bit_signals[i] += band[by][bx];
            }
        }
    }

    // Average bit_signals across 3 channels
    for s in &mut bit_signals {
        *s /= 3.0;
    }

    // Decode: compare each group mean against the GLOBAL mean of the ID region only.
    // We must NOT include primary watermark positions (index >= bits_needed) because
    // those all have +DWT_EMBED_STRENGTH bias that would skew the reference upward.
    let global_id_mean = bit_signals.iter().sum::<f32>() / bit_signals.len() as f32;
    let redundancy = crate::spread_spectrum::REDUNDANCY;
    let mut decoded_bits = Vec::new();
    for bit_idx in 0..(id_length * 8) {
        let start = bit_idx * redundancy;
        let end = (start + redundancy).min(bits_needed);
        let group_mean = bit_signals[start..end].iter().sum::<f32>() / (end - start) as f32;
        // bit=1 → +DWT_ID_EMBED_STRENGTH → group_mean > global_id_mean
        // bit=0 → -DWT_ID_EMBED_STRENGTH → group_mean < global_id_mean
        decoded_bits.push(group_mean > global_id_mean);
    }

    let decoded = crate::spread_spectrum::bits_to_str(&decoded_bits)?;
    Ok(decoded)
}
