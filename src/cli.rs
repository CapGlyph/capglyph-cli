use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

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
}

// ─── embed ───────────────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct EmbedArgs {
    /// Input image path (.png recommended; .jpg is accepted but output is always PNG)
    pub input: PathBuf,

    /// Output path (default: <stem>_sigil.png next to input)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Watermark stroke width in pixels (sub-perceptual default for tiny-skia renderer)
    /// Note: tiny-skia rendering floor is ~0.008px; values below that produce zero signal.
    /// Default 0.010px: signal present (α_nonzero≈3.4%, MAE≈0.087), invisible to humans.
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
}

// ─── verify ──────────────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct VerifyArgs {
    /// Image path to inspect
    pub input: PathBuf,

    /// Alpha nonzero-pixel fraction threshold to report watermark as present
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
