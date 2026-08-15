use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

/// Embedding mode for `sigil embed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum EmbedMode {
    /// Stage 1: sparse alpha-channel signal. Fast, invisible. Fragile to alpha stripping.
    Alpha,
    /// Stage 2: RGB DCT-domain residual. Survives PNG→JPG at quality≥50.
    Dct,
    /// Stage 3: Haar DWT LH-band residual. Survives scale and moderate blur.
    Dwt,
    /// Stage 4: learned TrustMark watermark (ONNX). Survives aggressive
    /// ordinary edits (JPEG q30, blur σ2, scale 0.5×). Requires the
    /// `learned` cargo feature and downloaded models.
    Learned,
}

impl std::fmt::Display for EmbedMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EmbedMode::Alpha => write!(f, "alpha"),
            EmbedMode::Dct => write!(f, "dct"),
            EmbedMode::Dwt => write!(f, "dwt"),
            EmbedMode::Learned => write!(f, "learned"),
        }
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "sigil",
    version,
    author,
    about = "Invisible structural watermark for images",
    long_about = "Sigil embeds a sub-perceptual structural watermark derived from the image's own \
                  geometry. The watermark is invisible to humans but detectable by machines, and \
                  is destroyed by PNG→JPG conversion or screenshots — signalling tampering."
)]
pub struct Cli {
    /// Verbose output (debug logging)
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Embed an invisible structural watermark into an image
    Embed(EmbedArgs),
    /// Verify whether a watermark is present in an image
    Verify(VerifyArgs),
    /// Remove the watermark layer from an image (produces clean RGB PNG)
    Strip(StripArgs),
    /// Show signal statistics without verification (no pass/fail threshold)
    Info(InfoArgs),
    /// Batch-process multiple images (embed or strip)
    Batch(BatchArgs),
    /// Extract embedded recipient ID from watermarked image
    Extract(ExtractArgs),
    /// Download learned-mode ONNX models (TrustMark) into the model dir
    #[cfg(feature = "learned")]
    FetchModels(FetchModelsArgs),
}

#[cfg(feature = "learned")]
#[derive(Args, Debug)]
pub struct FetchModelsArgs {
    /// Directory to store models (default: XDG data dir or $SIGIL_MODEL_DIR)
    #[arg(short, long)]
    pub model_dir: Option<PathBuf>,
}

// ─── embed ───────────────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct EmbedArgs {
    /// Input image path (.png recommended; .jpg is accepted but output is always PNG)
    pub input: PathBuf,

    /// Output path (default: <stem>_sigil.png next to input)
    /// For JPEG output, use --output file.jpg or --format jpg in batch mode
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Embedding mode:
    ///   alpha  — Stage 1: sparse Bresenham pixels written to alpha channel (default).
    ///            Fast, invisible, detects PNG→JPG tamper. Fragile: `convert -alpha off` kills it.
    ///   dct    — Stage 2: modulate mid-frequency DCT coefficients of 8×8 RGB blocks.
    ///            Survives PNG→JPG at quality≥50. Works on both RGB and RGBA sources.
    #[arg(long, default_value = "alpha")]
    pub mode: EmbedMode,

    /// Watermark stroke width in pixels (controls path density for geometry extraction).
    /// Used in alpha mode only for embed_alpha scaling.
    #[arg(long, default_value_t = 0.010)]
    pub stroke: f32,

    /// Detail level for path extraction 1–100 (higher = more paths)
    #[arg(long, default_value_t = 60)]
    pub detail: u8,

    /// Minimum path length in points
    #[arg(long, default_value_t = 5)]
    pub min_path_len: usize,

    /// Number of Chaikin smoothing iterations
    #[arg(long, default_value_t = 3)]
    pub chaikin_iters: usize,

    /// Sample original image colors for watermark paths (default: neutral gray)
    #[arg(long, default_value_t = false)]
    pub color: bool,

    /// Secret key for the key-derived secret layer (dct/dwt modes).
    /// The layer adds +EMBED_DELTA at HMAC(key, image)-derived positions.
    /// Verification of this layer requires the same key — it provides
    /// forgery resistance and blocks cross-image parameter learning.
    #[arg(long)]
    pub key: Option<String>,

    /// Save extracted geometry to this JSON file for later re-use
    #[arg(long)]
    pub save_geometry: Option<PathBuf>,

    /// Load geometry from a previously saved JSON file (skips re-analysis)
    #[arg(long, conflicts_with_all = ["detail", "min_path_len", "chaikin_iters", "color"])]
    pub from_geometry: Option<PathBuf>,

    /// Recipient ID for per-recipient tracking watermarks.
    /// Different IDs produce different watermarks on the same image — useful for
    /// identifying which recipient leaked a copy. The ID is mixed into the PRNG seed.
    #[arg(long)]
    pub recipient_id: Option<String>,

    /// Learned-mode model directory (TrustMark ONNX files).
    /// Default: $SIGIL_MODEL_DIR or the XDG data dir. Run `sigil fetch-models`.
    #[arg(long)]
    pub model_dir: Option<PathBuf>,

    /// Learned-mode watermark strength (0–1, default 0.95).
    #[arg(long, default_value_t = 0.95)]
    pub strength: f32,
}

// ─── verify ──────────────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct VerifyArgs {
    /// Image path to inspect
    pub input: PathBuf,

    /// Embedding mode used when this image was watermarked.
    /// `alpha` checks semi-transparent pixel fraction; `dct` checks DCT coefficient offset.
    #[arg(long, default_value = "alpha")]
    pub mode: EmbedMode,

    /// Optional geometry file for DCT verification (improves accuracy).
    /// If omitted, geometry is re-extracted from the watermarked image.
    #[arg(long)]
    pub geometry: Option<PathBuf>,

    /// Alpha nonzero-pixel fraction threshold (alpha mode only, default: 0.0001)
    #[arg(long, default_value_t = 0.0001)]
    pub threshold: f64,

    /// Mean-signal threshold for dct/dwt modes (default: 4.0).
    /// The watermark adds a positive bias to marked coefficients; a clean
    /// image has near-zero mean signal at those positions.
    #[arg(long, default_value_t = 4.0)]
    pub mean_threshold: f64,

    /// Secret key for verifying the key-derived secret layer.
    /// With a key, verify additionally checks the HMAC(key, image)-derived
    /// positions and reports the secret layer mean signal.
    #[arg(long)]
    pub key: Option<String>,

    /// Learned-mode model directory (TrustMark ONNX files).
    #[arg(long)]
    pub model_dir: Option<PathBuf>,

    /// Recipient ID to compare against (learned mode).
    #[arg(long)]
    pub recipient_id: Option<String>,

    /// Print full signal statistics
    #[arg(long)]
    pub verbose: bool,
}

// ─── strip ───────────────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct StripArgs {
    /// Input image path (must be RGBA PNG)
    pub input: PathBuf,

    /// Output path (default: <stem>_stripped.png)
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

// ─── info ────────────────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct InfoArgs {
    /// Input image path
    pub input: PathBuf,

    /// Detection mode (alpha or dct)
    #[arg(long, default_value = "alpha")]
    pub mode: EmbedMode,

    /// Optional geometry file for DCT mode (if not provided, re-extracts from image)
    #[arg(long)]
    pub geometry: Option<PathBuf>,
}

// ─── batch ───────────────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct BatchArgs {
    /// Operation: embed or strip
    #[arg(value_enum)]
    pub operation: BatchOperation,

    /// Input pattern (glob, e.g., "./images/*.png")
    pub input_pattern: String,

    /// Output directory (created if missing)
    #[arg(short, long)]
    pub output_dir: PathBuf,

    /// Embedding mode for batch embed (alpha or dct)
    #[arg(long, default_value = "alpha")]
    pub mode: EmbedMode,

    /// Output format for batch embed: png or jpg
    #[arg(long, default_value = "png")]
    pub format: OutputFormat,

    /// JPEG quality (10-100) when format=jpg
    #[arg(long, default_value_t = 85)]
    pub jpeg_quality: u8,

    /// Stroke width for batch embed
    #[arg(long, default_value_t = 0.010)]
    pub stroke: f32,

    /// Detail level for batch embed
    #[arg(long, default_value_t = 60)]
    pub detail: u8,

    /// Recipient ID for batch embed (same for all images in batch)
    #[arg(long)]
    pub recipient_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum BatchOperation {
    Embed,
    Strip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Png,
    Jpg,
}

// ─── extract ─────────────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct ExtractArgs {
    /// Watermarked image to extract recipient ID from
    pub input: PathBuf,

    /// Watermark mode (must match embed mode: alpha/dct/dwt)
    #[arg(short, long, value_enum, default_value_t = EmbedMode::Dct)]
    pub mode: EmbedMode,

    /// Optional geometry file (speeds up extraction; auto-extracted if omitted)
    #[arg(short, long)]
    pub geometry: Option<PathBuf>,

    /// Expected recipient ID length in characters (for bit extraction)
    #[arg(short = 'l', long, default_value_t = 16)]
    pub id_length: usize,

    /// Learned-mode model directory (TrustMark ONNX files).
    #[arg(long)]
    pub model_dir: Option<PathBuf>,
}
