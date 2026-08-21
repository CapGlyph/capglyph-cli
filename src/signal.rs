/// Alpha-channel signal metrics for a RGBA image buffer.
#[derive(Debug, Clone)]
pub struct SignalMetrics {
    /// Image dimensions
    pub width: u32,
    pub height: u32,
    /// Total pixel count
    pub total_pixels: u64,
    /// Number of pixels where alpha > 0
    pub nonzero_alpha_count: u64,
    /// Fraction of pixels with alpha > 0
    pub nonzero_alpha_frac: f64,
    /// Number of pixels where 0 < alpha < 255 (semi-transparent = watermark signal)
    pub semi_transparent_count: u64,
    /// Fraction of semi-transparent pixels
    pub semi_transparent_frac: f64,
    /// Mean alpha value across all pixels (0.0–255.0)
    pub alpha_mean: f64,
    /// Maximum alpha value observed (0–255)
    pub alpha_max: u8,
    /// 99th-percentile alpha value
    pub alpha_p99: u8,
    /// Mean absolute error vs pure white after compositing alpha over white background
    pub composite_mae: f64,
}

impl SignalMetrics {
    /// Compute metrics from a flat RGBA byte buffer (row-major, 4 bytes per pixel).
    ///
    /// `pixels` must have length `width * height * 4`.
    pub fn compute(pixels: &[u8], width: u32, height: u32) -> Self {
        assert_eq!(
            pixels.len(),
            (width as usize) * (height as usize) * 4,
            "pixel buffer size mismatch"
        );

        let total = (width as u64) * (height as u64);
        let mut nonzero: u64 = 0;
        let mut semi: u64 = 0;
        let mut alpha_sum: u64 = 0;
        let mut alpha_max: u8 = 0;
        let mut mae_sum: f64 = 0.0;
        let mut alpha_hist = [0u64; 256];

        #[allow(unknown_lints, clippy::chunks_exact_to_as_chunks)]
        for chunk in pixels.chunks_exact(4) {
            let r = chunk[0] as f64;
            let g = chunk[1] as f64;
            let b = chunk[2] as f64;
            let a = chunk[3];
            let af = a as f64 / 255.0;

            if a > 0 {
                nonzero += 1;
            }
            if a > 0 && a < 255 {
                semi += 1;
            }
            alpha_sum += a as u64;
            if a > alpha_max {
                alpha_max = a;
            }
            alpha_hist[a as usize] += 1;

            let cr = af * r + (1.0 - af) * 255.0;
            let cg = af * g + (1.0 - af) * 255.0;
            let cb = af * b + (1.0 - af) * 255.0;
            mae_sum += (255.0 - cr + (255.0 - cg) + (255.0 - cb)) / 3.0;
        }

        let p99_target = (0.99 * total as f64).ceil() as u64;
        let mut cumulative: u64 = 0;
        let mut alpha_p99: u8 = 0;
        for (val, &count) in alpha_hist.iter().enumerate() {
            cumulative += count;
            if cumulative >= p99_target {
                alpha_p99 = val as u8;
                break;
            }
        }

        SignalMetrics {
            width,
            height,
            total_pixels: total,
            nonzero_alpha_count: nonzero,
            nonzero_alpha_frac: nonzero as f64 / total as f64,
            semi_transparent_count: semi,
            semi_transparent_frac: semi as f64 / total as f64,
            alpha_mean: alpha_sum as f64 / total as f64,
            alpha_max,
            alpha_p99,
            composite_mae: mae_sum / total as f64,
        }
    }

    /// Whether the watermark signal is present: sufficient semi-transparent pixels.
    /// Uses semi-transparent fraction (0 < α < 255) as the signal indicator,
    /// since normal opaque images have 0% semi-transparent pixels.
    pub fn is_present(&self, threshold: f64) -> bool {
        self.semi_transparent_frac >= threshold
    }

    /// Human-readable summary line.
    pub fn summary(&self) -> String {
        format!(
            "α_semi={:.4}%  α_mean={:.4}  α_max={}  α_p99={}  composite_MAE={:.6}",
            self.semi_transparent_frac * 100.0,
            self.alpha_mean,
            self.alpha_max,
            self.alpha_p99,
            self.composite_mae,
        )
    }
}
