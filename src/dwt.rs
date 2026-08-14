//! Discrete Wavelet Transform (DWT) for watermark embedding.
//!
//! Implements 2D Haar wavelet transform for multi-resolution watermark embedding.
//! DWT decomposes an image into sub-bands (LL, LH, HL, HH) that survive scaling
//! and moderate filtering better than DCT blocks.

use anyhow::{Context, Result};

/// Wavelet sub-band selection for embedding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaveletBand {
    /// Low-Low: coarse approximation (most robust, most visible)
    LL,
    /// Low-High: horizontal edges (good tradeoff)
    LH,
    /// High-Low: vertical edges (good tradeoff)
    HL,
    /// High-High: diagonal details (most invisible, most fragile)
    HH,
}

/// 2D Haar DWT decomposition result.
#[derive(Debug, Clone)]
pub struct DwtDecomposition {
    pub ll: Vec<Vec<f32>>, // Low-Low (coarse approximation)
    pub lh: Vec<Vec<f32>>, // Low-High (horizontal edges)
    pub hl: Vec<Vec<f32>>, // High-Low (vertical edges)
    pub hh: Vec<Vec<f32>>, // High-High (diagonal details)
    pub width: usize,      // Original image width
    pub height: usize,     // Original image height
}

impl DwtDecomposition {
    /// Get a mutable reference to the specified sub-band.
    pub fn band_mut(&mut self, band: WaveletBand) -> &mut Vec<Vec<f32>> {
        match band {
            WaveletBand::LL => &mut self.ll,
            WaveletBand::LH => &mut self.lh,
            WaveletBand::HL => &mut self.hl,
            WaveletBand::HH => &mut self.hh,
        }
    }

    /// Get an immutable reference to the specified sub-band.
    pub fn band(&self, band: WaveletBand) -> &Vec<Vec<f32>> {
        match band {
            WaveletBand::LL => &self.ll,
            WaveletBand::LH => &self.lh,
            WaveletBand::HL => &self.hl,
            WaveletBand::HH => &self.hh,
        }
    }
}

/// Perform 1D Haar wavelet transform in-place on a single row/column.
///
/// Splits the data into approximation (low-pass) and detail (high-pass) coefficients.
/// Output layout: [approx[0], approx[1], ..., detail[0], detail[1], ...]
fn haar_1d_forward(data: &mut [f32]) {
    let n = data.len();
    if n < 2 {
        return;
    }

    let mut temp = vec![0.0; n];
    let half = n / 2;

    // Compute approximation and detail coefficients
    for i in 0..half {
        let a = data[2 * i];
        let b = data[2 * i + 1];
        temp[i] = (a + b) / 2.0_f32.sqrt();         // Approximation (low-pass)
        temp[half + i] = (a - b) / 2.0_f32.sqrt();  // Detail (high-pass)
    }

    data.copy_from_slice(&temp);
}

/// Perform 1D inverse Haar wavelet transform in-place.
fn haar_1d_inverse(data: &mut [f32]) {
    let n = data.len();
    if n < 2 {
        return;
    }

    let mut temp = vec![0.0; n];
    let half = n / 2;

    // Reconstruct from approximation and detail coefficients
    for i in 0..half {
        let approx = data[i];
        let detail = data[half + i];
        temp[2 * i] = (approx + detail) / 2.0_f32.sqrt();
        temp[2 * i + 1] = (approx - detail) / 2.0_f32.sqrt();
    }

    data.copy_from_slice(&temp);
}

/// Perform 2D Haar DWT on a single-channel image (grayscale or single RGB channel).
///
/// Returns the four sub-bands: LL, LH, HL, HH.
pub fn haar_2d_forward(image: &[Vec<f32>]) -> Result<DwtDecomposition> {
    let height = image.len();
    let width = image.first().context("Empty image")?.len();

    if width < 2 || height < 2 {
        anyhow::bail!("Image too small for DWT (min 2×2 required)");
    }

    // Copy input to working buffer
    let mut working = image.to_vec();

    // Step 1: Apply 1D Haar transform to each row
    for row in &mut working {
        haar_1d_forward(row);
    }

    // Step 2: Apply 1D Haar transform to each column
    for col_idx in 0..width {
        let mut column: Vec<f32> = working.iter().map(|row| row[col_idx]).collect();
        haar_1d_forward(&mut column);
        for (row_idx, val) in column.iter().enumerate() {
            working[row_idx][col_idx] = *val;
        }
    }

    // Step 3: Extract the four sub-bands
    let half_width = width / 2;
    let half_height = height / 2;

    let mut ll = vec![vec![0.0; half_width]; half_height];
    let mut lh = vec![vec![0.0; half_width]; half_height];
    let mut hl = vec![vec![0.0; half_width]; half_height];
    let mut hh = vec![vec![0.0; half_width]; half_height];

    for y in 0..half_height {
        for x in 0..half_width {
            ll[y][x] = working[y][x];
            lh[y][x] = working[y][half_width + x];
            hl[y][x] = working[half_height + y][x];
            hh[y][x] = working[half_height + y][half_width + x];
        }
    }

    Ok(DwtDecomposition {
        ll,
        lh,
        hl,
        hh,
        width,
        height,
    })
}

/// Perform 2D inverse Haar DWT to reconstruct the image from sub-bands.
pub fn haar_2d_inverse(decomp: &DwtDecomposition) -> Result<Vec<Vec<f32>>> {
    let half_width = decomp.ll[0].len();
    let half_height = decomp.ll.len();
    let width = decomp.width;
    let height = decomp.height;

    // Step 1: Merge sub-bands back into a single buffer
    let mut working = vec![vec![0.0; width]; height];

    for y in 0..half_height {
        for x in 0..half_width {
            working[y][x] = decomp.ll[y][x];
            working[y][half_width + x] = decomp.lh[y][x];
            working[half_height + y][x] = decomp.hl[y][x];
            working[half_height + y][half_width + x] = decomp.hh[y][x];
        }
    }

    // Step 2: Apply inverse 1D Haar transform to each column
    for col_idx in 0..width {
        let mut column: Vec<f32> = working.iter().map(|row| row[col_idx]).collect();
        haar_1d_inverse(&mut column);
        for (row_idx, val) in column.iter().enumerate() {
            working[row_idx][col_idx] = *val;
        }
    }

    // Step 3: Apply inverse 1D Haar transform to each row
    for row in &mut working {
        haar_1d_inverse(row);
    }

    Ok(working)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_haar_1d_perfect_reconstruction() {
        let mut data = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let original = data.clone();

        haar_1d_forward(&mut data);
        haar_1d_inverse(&mut data);

        for (a, b) in original.iter().zip(data.iter()) {
            assert!((a - b).abs() < 1e-5, "Perfect reconstruction failed: {} vs {}", a, b);
        }
    }

    #[test]
    fn test_haar_2d_perfect_reconstruction() {
        let image = vec![
            vec![1.0, 2.0, 3.0, 4.0],
            vec![5.0, 6.0, 7.0, 8.0],
            vec![9.0, 10.0, 11.0, 12.0],
            vec![13.0, 14.0, 15.0, 16.0],
        ];
        let original = image.clone();

        let decomp = haar_2d_forward(&image).unwrap();
        let reconstructed = haar_2d_inverse(&decomp).unwrap();

        for (row_orig, row_recon) in original.iter().zip(reconstructed.iter()) {
            for (a, b) in row_orig.iter().zip(row_recon.iter()) {
                assert!(
                    (a - b).abs() < 1e-5,
                    "2D perfect reconstruction failed: {} vs {}",
                    a,
                    b
                );
            }
        }
    }

    #[test]
    fn test_dwt_band_access() {
        let image = vec![
            vec![1.0, 2.0, 3.0, 4.0],
            vec![5.0, 6.0, 7.0, 8.0],
            vec![9.0, 10.0, 11.0, 12.0],
            vec![13.0, 14.0, 15.0, 16.0],
        ];

        let mut decomp = haar_2d_forward(&image).unwrap();

        // Test mutable access
        let lh_band = decomp.band_mut(WaveletBand::LH);
        lh_band[0][0] += 10.0;

        // Test immutable access
        let lh_band_read = decomp.band(WaveletBand::LH);
        assert!((lh_band_read[0][0] - (decomp.lh[0][0])).abs() < 1e-5);
    }
}
