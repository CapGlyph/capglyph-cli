use serde::{Deserialize, Serialize};

/// Sigil geometry JSON format v1.
///
/// Stores the Chaikin-smoothed polyline paths extracted from an image so they
/// can be re-rendered at a different stroke width without re-running the full
/// raster analysis pipeline.
///
/// This is Sigil's own format — intentionally simpler than Vectomancy's
/// `MathExpressionAST` (no Fourier/Spline math, just polyline points).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeometryFile {
    /// Format version (always 1 for now)
    pub version: u32,
    /// Original image width in pixels
    pub original_width: u32,
    /// Original image height in pixels
    pub original_height: u32,
    /// Analysis parameters used to produce these paths
    pub analysis_params: AnalysisParams,
    /// Extracted polyline paths
    pub paths: Vec<PathEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisParams {
    pub detail: u8,
    pub min_path_len: usize,
    pub chaikin_iters: usize,
    pub color: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathEntry {
    /// Stroke color as `[r, g, b]` in 0.0–1.0 (None → neutral gray 0.5)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<[f32; 3]>,
    /// Polyline vertices as `[[x, y], ...]` in image pixel coordinates
    pub points: Vec<[f64; 2]>,
}

impl GeometryFile {
    pub const CURRENT_VERSION: u32 = 1;

    /// Load a geometry file from JSON bytes.
    pub fn from_json(bytes: &[u8]) -> anyhow::Result<Self> {
        let gf: GeometryFile = serde_json::from_slice(bytes)?;
        if gf.version != Self::CURRENT_VERSION {
            anyhow::bail!(
                "Unsupported geometry file version {} (expected {})",
                gf.version,
                Self::CURRENT_VERSION
            );
        }
        Ok(gf)
    }

    /// Serialize to pretty-printed JSON bytes.
    pub fn to_json(&self) -> anyhow::Result<Vec<u8>> {
        Ok(serde_json::to_vec_pretty(self)?)
    }
}
