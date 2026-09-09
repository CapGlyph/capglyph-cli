//! Discrete Wavelet Transform (DWT) for watermark embedding.

pub use capglyph_core::dwt::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_haar_1d_perfect_reconstruction() {
        let image = vec![vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]; 2];
        let original = image.clone();
        let decomp = haar_2d_forward(&image).unwrap();
        let reconstructed = haar_2d_inverse(&decomp).unwrap();
        for (row_orig, row_recon) in original.iter().zip(reconstructed.iter()) {
            for (a, b) in row_orig.iter().zip(row_recon.iter()) {
                assert!((a - b).abs() < 1e-5);
            }
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

        let lh_band = decomp.band_mut(WaveletBand::LH);
        lh_band[0][0] += 10.0;

        let lh_band_read = decomp.band(WaveletBand::LH);
        assert!((lh_band_read[0][0] - (decomp.lh[0][0])).abs() < 1e-5);
    }
}
