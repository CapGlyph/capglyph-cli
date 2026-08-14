use anyhow::{Context, Result};
use tracing::info;

use crate::cli::{EmbedMode, VerifyArgs};
use crate::geometry::GeometryFile;
use crate::signal::SignalMetrics;
use std::collections::HashSet;

/// Exit codes
pub const EXIT_PRESENT: i32 = 0;
pub const EXIT_ABSENT: i32 = 1;

/// Entry point for the `verify` subcommand.
///
/// Returns `Ok(true)` if watermark is present, `Ok(false)` if absent.
/// The caller is responsible for mapping this to a process exit code.
pub fn run(args: &VerifyArgs) -> Result<bool> {
    info!("Verifying watermark in: {:?}", args.input);

    let img = image::open(&args.input)
        .with_context(|| format!("Failed to open image: {:?}", args.input))?;

    match args.mode {
        EmbedMode::Alpha => verify_alpha(&img, args),
        EmbedMode::Dct => verify_dct(&img, args),
    }
}

/// Alpha-channel verification: check semi-transparent pixel fraction.
fn verify_alpha(img: &image::DynamicImage, args: &VerifyArgs) -> Result<bool> {
    // If the colour type has no alpha channel the watermark is absent by definition.
    let has_alpha = matches!(
        img.color(),
        image::ColorType::Rgba8
            | image::ColorType::Rgba16
            | image::ColorType::Rgba32F
            | image::ColorType::La8
            | image::ColorType::La16
    );
    if !has_alpha {
        println!("WATERMARK ABSENT OR DESTROYED");
        if args.verbose {
            let (w, h) = (img.width(), img.height());
            println!(
                "  image:  {}×{} — no alpha channel (colour type: {:?})",
                w, h, img.color()
            );
            println!("  reason: format has no alpha channel (RGB/JPG/stripped)");
        }
        return Ok(false);
    }

    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let pixels = rgba.as_raw();
    let metrics = SignalMetrics::compute(pixels, w, h);

    let present = metrics.is_present(args.threshold);

    if present {
        println!("WATERMARK PRESENT");
        if args.verbose {
            println!(
                "  image:     {}×{} ({} pixels)",
                metrics.width, metrics.height, metrics.total_pixels
            );
            println!("  nonzero:   {} pixels", metrics.nonzero_alpha_count);
            println!("  {}", metrics.summary());
            println!("  threshold: {}", args.threshold);
        }
    } else {
        println!("WATERMARK ABSENT OR DESTROYED");
        if args.verbose {
            println!(
                "  image:     {}×{} ({} pixels)",
                metrics.width, metrics.height, metrics.total_pixels
            );
            println!("  {}", metrics.summary());
            println!("  threshold: {}", args.threshold);
        }
    }

    Ok(present)
}

/// DCT-domain verification: check if marked blocks still have offset in target coefficient.
fn verify_dct(img: &image::DynamicImage, args: &VerifyArgs) -> Result<bool> {
    let geometry_path = args.geometry.as_ref().context(
        "DCT verification requires --geometry <file.json> (same file used during embed)",
    )?;

    let geom: GeometryFile = {
        let s = std::fs::read_to_string(geometry_path)
            .with_context(|| format!("Failed to read geometry: {:?}", geometry_path))?;
        serde_json::from_str(&s)
            .with_context(|| format!("Failed to parse geometry JSON: {:?}", geometry_path))?
    };

    anyhow::ensure!(
        geom.version == 1,
        "Unsupported geometry format version: {}",
        geom.version
    );

    // Reconstruct path pixel set
    let mut path_pixels = HashSet::new();
    for path in &geom.paths {
        for point in &path.points {
            let (x, y) = (point[0], point[1]);
            if x >= 0.0 && y >= 0.0 {
                path_pixels.insert((x as u32, y as u32));
            }
        }
    }

    let rgb = img.to_rgb8();
    let (iw, ih) = rgb.dimensions();

    // Identify blocks that contain skeleton pixels
    let mut skeleton_blocks = HashSet::new();
    for (px, py) in &path_pixels {
        let bx = px / 8;
        let by = py / 8;
        if (bx + 1) * 8 <= iw && (by + 1) * 8 <= ih {
            skeleton_blocks.insert((bx, by));
        }
    }

    if skeleton_blocks.is_empty() {
        anyhow::bail!("No valid skeleton blocks found in geometry");
    }

    // Sample blocks: check how many still have the marker coefficient offset
    let (u, v) = (2, 3);
    let mut marked_count = 0;
    let sample_size = skeleton_blocks.len().min(100);
    let sample: Vec<_> = skeleton_blocks.iter().take(sample_size).copied().collect();

    for (bx, by) in &sample {
        let mut block = crate::dct::extract_block(&rgb, bx * 8, by * 8, 0);
        crate::dct::dct8x8_forward(&mut block);
        if block[u][v].abs() > 8.0 {
            marked_count += 1;
        }
    }

    let detection_rate = marked_count as f64 / sample_size as f64;
    let present = detection_rate >= args.threshold;

    if present {
        println!("WATERMARK PRESENT");
        if args.verbose {
            println!("  mode:      dct");
            println!("  image:     {}×{}", iw, ih);
            println!("  blocks:    {} total, {} sampled", skeleton_blocks.len(), sample_size);
            println!("  coeff:     F[{},{}]", u, v);
            println!(
                "  detection: {}/{} ({:.1}%)",
                marked_count,
                sample_size,
                detection_rate * 100.0
            );
            println!("  threshold: {:.0}%", args.threshold * 100.0);
        }
    } else {
        println!("WATERMARK ABSENT OR DESTROYED");
        if args.verbose {
            println!("  mode:      dct");
            println!("  image:     {}×{}", iw, ih);
            println!("  blocks:    {} total, {} sampled", skeleton_blocks.len(), sample_size);
            println!("  coeff:     F[{},{}]", u, v);
            println!(
                "  detection: {}/{} ({:.1}%)",
                marked_count,
                sample_size,
                detection_rate * 100.0
            );
            println!("  threshold: {:.0}%", args.threshold * 100.0);
        }
    }

    Ok(present)
}
