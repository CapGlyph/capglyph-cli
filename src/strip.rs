use anyhow::{Context, Result};
use tracing::info;

use crate::cli::StripArgs;
use crate::embed::resolve_output;

/// Entry point for the `strip` subcommand.
pub fn run(args: &StripArgs) -> Result<()> {
    let output_path = resolve_output(&args.input, args.output.as_deref(), "_stripped", "png");

    info!("Loading image for strip: {:?}", args.input);
    let img = image::open(&args.input)
        .with_context(|| format!("Failed to open image: {:?}", args.input))?;

    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();

    // Set every pixel to fully opaque; convert to RGB (drop alpha entirely)
    let rgb: image::RgbImage = image::RgbImage::from_fn(w, h, |x, y| {
        let p = rgba.get_pixel(x, y);
        let af = p[3] as f32 / 255.0;
        // Composite over white background, then discard alpha
        let r = (af * p[0] as f32 + (1.0 - af) * 255.0).round() as u8;
        let g = (af * p[1] as f32 + (1.0 - af) * 255.0).round() as u8;
        let b = (af * p[2] as f32 + (1.0 - af) * 255.0).round() as u8;
        image::Rgb([r, g, b])
    });

    info!("Saving stripped output to: {:?}", output_path);
    rgb.save(&output_path)
        .with_context(|| format!("Failed to save stripped image: {:?}", output_path))?;

    println!("Watermark stripped → {:?}", output_path);
    Ok(())
}
