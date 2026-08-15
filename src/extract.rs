//! `sigil extract` — recover embedded recipient ID from a watermarked image.
//!
//! Reads the spread-spectrum bit stream embedded during `sigil embed --recipient-id`
//! and reconstructs the original ID string.

use anyhow::{Context, Result};
use tracing::info;

use crate::cli::ExtractArgs;

/// Entry point for the `extract` subcommand.
/// Returns the decoded recipient ID string.
pub fn run(args: &ExtractArgs) -> Result<String> {
    info!("Extracting recipient ID from: {:?}", args.input);

    let img = image::open(&args.input)
        .with_context(|| format!("Failed to open image: {:?}", args.input))?;

    let rgb = img.to_rgb8();
    let (w, h) = rgb.dimensions();

    match args.mode {
        // DCT mode is geometry-free: the seed is recovered from self-sync blocks.
        crate::cli::EmbedMode::Dct => extract_from_dct(&rgb, args.id_length, w, h),
        // DWT mode is geometry-free: the seed is recovered from self-sync LH positions.
        crate::cli::EmbedMode::Dwt => extract_from_dwt(&rgb, args.id_length, w, h),
        crate::cli::EmbedMode::Alpha => {
            anyhow::bail!("extract is not supported for alpha mode (alpha mode does not carry recoverable bits)")
        }
        crate::cli::EmbedMode::Learned => extract_from_learned(img, args),
    }
}

/// Learned-mode extraction: TrustMark decode → bitstring → ASCII bytes.
/// With `--key`, the payload is XOR-decrypted with the HMAC keystream
/// before byte packing (payload was keyed at embed time).
#[cfg(feature = "learned")]
fn extract_from_learned(img: image::DynamicImage, args: &ExtractArgs) -> Result<String> {
    let dir = crate::learned::model_dir(args.model_dir.as_deref());
    let bits = crate::learned::decode(img.clone(), &dir)?;
    let bits = match &args.key {
        Some(k) => {
            let seed = crate::learned::image_seed(&img.to_rgb8());
            crate::learned::decrypt_bits(&bits, k, seed)
        }
        None => bits,
    };
    // Convert bitstring to ASCII: 8 bits per byte, stop at null byte.
    let mut bytes = Vec::new();
    for chunk in bits.as_bytes().chunks(8) {
        let mut byte = 0u8;
        for &c in chunk.iter() {
            byte = (byte << 1) | if c == b'1' { 1 } else { 0 };
        }
        bytes.push(byte);
    }
    // Trim trailing nulls
    while bytes.last() == Some(&0) {
        bytes.pop();
    }
    let s = String::from_utf8(bytes)
        .map_err(|_| anyhow::anyhow!("decoded bits are not valid ASCII"))?;
    Ok(s)
}

#[cfg(not(feature = "learned"))]
fn extract_from_learned(_img: image::DynamicImage, _args: &ExtractArgs) -> Result<String> {
    anyhow::bail!(
        "learned mode requires the `learned` cargo feature (build with --features learned)"
    );
}

fn extract_from_dct(img: &image::RgbImage, id_length: usize, _w: u32, _h: u32) -> Result<String> {
    use crate::dct::{
        dct8x8_forward, extract_block, prng_block_list, ID_TARGET_U, ID_TARGET_V, SEED_MAGIC,
    };
    use crate::spread_spectrum::{bits_to_str, REDUNDANCY};

    let (iw, ih) = img.dimensions();

    // Step 1: recover the 64-bit ID block seed from self-sync blocks.
    // These blocks are located via a fixed magic constant, independent of
    // image content — so no geometry file or prior is needed.
    let sync_blocks = prng_block_list(SEED_MAGIC, iw, ih, 64 * 8);
    if sync_blocks.len() < 64 * 8 {
        anyhow::bail!(
            "Image too small for self-sync seed blocks ({} < 256)",
            sync_blocks.len()
        );
    }

    let read_coeffs = |blocks: &[(u32, u32)], count: usize| -> Vec<f32> {
        blocks
            .iter()
            .take(count)
            .map(|(bx, by)| {
                let mut sum = 0.0f32;
                for ch in 0..3 {
                    let mut block = extract_block(img, bx * 8, by * 8, ch);
                    dct8x8_forward(&mut block);
                    sum += block[ID_TARGET_U][ID_TARGET_V];
                }
                sum / 3.0
            })
            .collect()
    };

    let sync_coeffs = read_coeffs(&sync_blocks, 64 * 8);
    let sync_global_mean = sync_coeffs.iter().sum::<f32>() / sync_coeffs.len() as f32;
    let mut seed_bits = Vec::with_capacity(64);
    for bit_idx in 0..64 {
        let start = bit_idx * 8;
        let group_mean: f32 = sync_coeffs[start..start + 8].iter().sum::<f32>() / 8.0;
        seed_bits.push(group_mean > sync_global_mean);
    }
    let mut seed_bytes = [0u8; 8];
    for (i, chunk) in seed_bits.chunks(8).enumerate() {
        seed_bytes[i] = chunk.iter().fold(0u8, |acc, &bit| (acc << 1) | bit as u8);
    }
    let seed = u64::from_le_bytes(seed_bytes);

    // Step 2: reconstruct the ID block list from the recovered seed
    // (excluding sync blocks, matching the embed-time exclusion)
    let bits_needed = id_length * 8 * REDUNDANCY;
    let sync_set: std::collections::HashSet<(u32, u32)> = sync_blocks.iter().copied().collect();
    let blocks: Vec<(u32, u32)> = prng_block_list(seed, iw, ih, bits_needed + sync_set.len())
        .into_iter()
        .filter(|b| !sync_set.contains(b))
        .collect();
    if blocks.len() < bits_needed {
        anyhow::bail!(
            "Insufficient blocks ({}) to extract {} bytes with redundancy={}",
            blocks.len(),
            id_length,
            REDUNDANCY
        );
    }

    // Step 3: decode ID bits (group mean vs global mean)
    let id_coeffs = read_coeffs(&blocks, bits_needed);
    let global_mean = id_coeffs.iter().sum::<f32>() / id_coeffs.len() as f32;
    let mut decoded_bits = Vec::new();
    for bit_idx in 0..(id_length * 8) {
        let start = bit_idx * REDUNDANCY;
        let end = (start + REDUNDANCY).min(id_coeffs.len());
        let group_mean: f32 = id_coeffs[start..end].iter().sum::<f32>() / (end - start) as f32;
        decoded_bits.push(group_mean > global_mean);
    }

    let decoded = bits_to_str(&decoded_bits)?;
    Ok(decoded)
}

fn extract_from_dwt(img: &image::RgbImage, id_length: usize, w: u32, h: u32) -> Result<String> {
    use crate::dwt::haar_2d_forward;
    use crate::dwt_embed::{prng_band_positions, EMBED_BAND, SEED_MAGIC, SYNC_REDUNDANCY};

    let band_w = w / 2;
    let band_h = h / 2;

    // Step 1: recover the 64-bit seed from self-sync positions
    let sync_positions = prng_band_positions(SEED_MAGIC, band_w, band_h, 64 * SYNC_REDUNDANCY);
    if sync_positions.len() < 64 * SYNC_REDUNDANCY {
        anyhow::bail!("Image too small for self-sync positions");
    }

    let mut sync_signals: Vec<f32> = vec![0.0; 64 * SYNC_REDUNDANCY];
    for ch in 0..3usize {
        let channel_matrix: Vec<Vec<f32>> = (0..h)
            .map(|y| (0..w).map(|x| img.get_pixel(x, y)[ch] as f32).collect())
            .collect();
        let decomp = haar_2d_forward(&channel_matrix)?;
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
    let sync_global_mean = sync_signals.iter().sum::<f32>() / sync_signals.len() as f32;
    let mut seed_bits = Vec::with_capacity(64);
    for bit_idx in 0..64 {
        let start = bit_idx * SYNC_REDUNDANCY;
        let group_mean: f32 = sync_signals[start..start + SYNC_REDUNDANCY]
            .iter()
            .sum::<f32>()
            / SYNC_REDUNDANCY as f32;
        seed_bits.push(group_mean > sync_global_mean);
    }
    let mut seed_bytes = [0u8; 8];
    for (i, chunk) in seed_bits.chunks(8).enumerate() {
        seed_bytes[i] = chunk.iter().fold(0u8, |acc, &bit| (acc << 1) | bit as u8);
    }
    let seed = u64::from_le_bytes(seed_bytes);

    // Step 2: reconstruct ID positions from the recovered seed
    let bit_count = id_length * 8;
    let redundancy = crate::spread_spectrum::REDUNDANCY;
    let bits_needed = bit_count * redundancy;
    let sync_set: std::collections::HashSet<(u32, u32)> = sync_positions.iter().copied().collect();
    let positions: Vec<(u32, u32)> =
        prng_band_positions(seed, band_w, band_h, bits_needed + sync_set.len())
            .into_iter()
            .filter(|p| !sync_set.contains(p))
            .collect();
    if positions.len() < bits_needed {
        anyhow::bail!(
            "Not enough DWT positions for ID extraction: need {}, have {}",
            bits_needed,
            positions.len()
        );
    }

    // Step 3: decode ID bits
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
    for s in &mut bit_signals {
        *s /= 3.0;
    }

    let global_id_mean = bit_signals.iter().sum::<f32>() / bit_signals.len() as f32;
    let mut decoded_bits = Vec::new();
    for bit_idx in 0..(id_length * 8) {
        let start = bit_idx * redundancy;
        let end = (start + redundancy).min(bits_needed);
        let group_mean = bit_signals[start..end].iter().sum::<f32>() / (end - start) as f32;
        decoded_bits.push(group_mean > global_id_mean);
    }

    let decoded = crate::spread_spectrum::bits_to_str(&decoded_bits)?;
    Ok(decoded)
}
