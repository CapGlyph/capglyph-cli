//! Spread-spectrum encoding for recoverable recipient ID watermarking.
//!
//! Encodes a string (recipient ID) as binary bits, embeds each bit redundantly
//! across multiple coefficients (DCT or DWT), and extracts the ID via averaging.

use anyhow::{Context, Result};

/// Redundancy factor: how many coefficients encode each bit
pub const REDUNDANCY: usize = 8;

/// Encode a recipient ID string into binary bits
pub fn encode_bits(recipient_id: &str) -> Vec<u8> {
    recipient_id
        .bytes()
        .flat_map(|b| (0..8).rev().map(move |i| (b >> i) & 1))
        .collect()
}

/// Convert string to bool bits (for DCT/DWT direct embedding)
pub fn str_to_bits(s: &str) -> Vec<bool> {
    s.bytes()
        .flat_map(|b| (0..8).rev().map(move |i| (b >> i) & 1 == 1))
        .collect()
}

/// Convert bool bits back to string
pub fn bits_to_str(bits: &[bool]) -> Result<String> {
    anyhow::ensure!(
        bits.len().is_multiple_of(8),
        "Bit count must be multiple of 8"
    );

    let bytes: Vec<u8> = bits
        .chunks(8)
        .map(|chunk| chunk.iter().fold(0u8, |acc, &bit| (acc << 1) | (bit as u8)))
        .collect();

    String::from_utf8(bytes).context("Invalid UTF-8 in decoded bits")
}

/// Decode binary bits back to string (legacy u8 API for DWT)
pub fn decode_bits(bits: &[u8]) -> Result<String> {
    anyhow::ensure!(
        bits.len().is_multiple_of(8),
        "Bit count must be multiple of 8"
    );

    let bytes: Vec<u8> = bits
        .chunks(8)
        .map(|chunk| chunk.iter().fold(0u8, |acc, &bit| (acc << 1) | bit))
        .collect();

    String::from_utf8(bytes).context("Invalid UTF-8 in decoded bits")
}

/// Embed bits into coefficient array via ±strength modulation
///
/// Each bit is embedded into REDUNDANCY consecutive coefficients:
/// - bit=1 → coeff += strength
/// - bit=0 → coeff -= strength
pub fn embed_into_coeffs(coeffs: &mut [f32], bits: &[u8], strength: f32, seed: u64) {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let total_needed = bits.len() * REDUNDANCY;
    if coeffs.len() < total_needed {
        tracing::warn!(
            "Not enough coefficients ({}) for {} bits with redundancy {}. Truncating.",
            coeffs.len(),
            bits.len(),
            REDUNDANCY
        );
    }

    // Pseudo-random permutation of coefficient indices (seeded by seed)
    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    let perm_seed = hasher.finish();

    let mut indices: Vec<usize> = (0..coeffs.len()).collect();
    // Simple Fisher-Yates shuffle with seed
    let mut rng_state = perm_seed;
    for i in (1..indices.len()).rev() {
        rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let j = (rng_state as usize) % (i + 1);
        indices.swap(i, j);
    }

    for (bit_idx, &bit) in bits.iter().enumerate() {
        let delta = if bit == 1 { strength } else { -strength };
        for r in 0..REDUNDANCY {
            let coeff_idx = bit_idx * REDUNDANCY + r;
            if coeff_idx >= indices.len() {
                break;
            }
            coeffs[indices[coeff_idx]] += delta;
        }
    }
}

/// Extract bits from coefficient array via correlation detection
pub fn extract_from_coeffs(coeffs: &[f32], bit_count: usize, seed: u64) -> Vec<u8> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let total_needed = bit_count * REDUNDANCY;
    if coeffs.len() < total_needed {
        tracing::warn!(
            "Not enough coefficients ({}) for {} bits. Extracting partial.",
            coeffs.len(),
            bit_count
        );
    }

    // Same pseudo-random permutation as embed
    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    let perm_seed = hasher.finish();

    let mut indices: Vec<usize> = (0..coeffs.len()).collect();
    let mut rng_state = perm_seed;
    for i in (1..indices.len()).rev() {
        rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let j = (rng_state as usize) % (i + 1);
        indices.swap(i, j);
    }

    let mut bits = Vec::with_capacity(bit_count);
    for bit_idx in 0..bit_count {
        let mut sum = 0.0;
        let mut count = 0;
        for r in 0..REDUNDANCY {
            let coeff_idx = bit_idx * REDUNDANCY + r;
            if coeff_idx >= indices.len() {
                break;
            }
            sum += coeffs[indices[coeff_idx]];
            count += 1;
        }

        let avg = if count > 0 { sum / count as f32 } else { 0.0 };
        bits.push(if avg > 0.0 { 1 } else { 0 });
    }

    bits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_roundtrip() {
        let id = "user123";
        let bits = encode_bits(id);
        assert_eq!(bits.len(), id.len() * 8);

        let decoded = decode_bits(&bits).unwrap();
        assert_eq!(decoded, id);
    }

    #[test]
    fn test_embed_extract_roundtrip() {
        let id = "alice";
        let bits = encode_bits(id);

        let mut coeffs = vec![0.0f32; bits.len() * REDUNDANCY + 100];
        embed_into_coeffs(&mut coeffs, &bits, 10.0, 42);

        let extracted_bits = extract_from_coeffs(&coeffs, bits.len(), 42);
        let decoded = decode_bits(&extracted_bits).unwrap();

        assert_eq!(decoded, id);
    }

    #[test]
    fn test_noisy_extraction() {
        let id = "bob";
        let bits = encode_bits(id);

        let mut coeffs = vec![0.0f32; bits.len() * REDUNDANCY + 50];
        embed_into_coeffs(&mut coeffs, &bits, 8.0, 99);

        // Add noise
        for c in &mut coeffs {
            *c += ((*c as i32 * 7) % 5) as f32 - 2.0; // ±2 noise
        }

        let extracted_bits = extract_from_coeffs(&coeffs, bits.len(), 99);
        let decoded = decode_bits(&extracted_bits).unwrap();

        assert_eq!(decoded, id, "Should survive moderate noise via redundancy");
    }
}
