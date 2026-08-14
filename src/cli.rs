use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

/// Embedding mode for `sigil embed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum EmbedMode {
    /// Stage 1: sparse alpha-channel signal. Fast, invisible. Fragile to alpha stripping.
    Alpha,
    /// Stage 2: RGB DCT-domain residual. Survives PNG→JPG at quality≥50.
    Dct,
}

impl std::fmt::Display for EmbedMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EmbedMode::Alpha => write!(f, "alpha"),
            EmbedMode::Dct => write!(f, "dct"),
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
