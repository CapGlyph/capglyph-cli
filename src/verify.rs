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
        EmbedMode::Dwt => verify_dwt(&img, args),
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
                w,
                h,
                img.color()
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
    let rgb = img.to_rgb8();
    let (iw, ih) = rgb.dimensions();

    // Determine block set: from geometry file (if provided) or re-extract skeleton
    let skeleton_blocks: HashSet<(u32, u32)> = if let Some(geometry_path) = &args.geometry {
        // Legacy path: use provided geometry file
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

        // Identify blocks from stored paths
        let mut path_pixels = HashSet::new();
        for path in &geom.paths {
            for point in &path.points {
                let (x, y) = (point[0], point[1]);
                if x >= 0.0 && y >= 0.0 {
                    path_pixels.insert((x as u32, y as u32));
                }
            }
        }

        if path_pixels.is_empty() {
            // Solid-color fallback: use stored PRNG seed from geometry file
            let seed = geom.prng_seed.ok_or_else(|| {
                anyhow::anyhow!(
                    "No skeleton paths and no PRNG seed in geometry file. \
                 Re-embed with current Sigil version to generate a seed."
                )
            })?;
            crate::dct::prng_blocks_from_seed(seed, iw, ih)
        } else {
            let mut set = HashSet::new();
            for (px, py) in &path_pixels {
                let bx = px / 8;
                let by = py / 8;
                if (bx + 1) * 8 <= iw && (by + 1) * 8 <= ih {
                    set.insert((bx, by));
                }
            }
            set
        }
    } else {
        // New path: re-extract skeleton from watermarked image
        info!("No geometry file provided, re-extracting skeleton from image");
        let geom = extract_geometry_from_image(img)?;

        let mut path_pixels = HashSet::new();
        for path in &geom.paths {
            for point in &path.points {
                let (x, y) = (point[0], point[1]);
                if x >= 0.0 && y >= 0.0 {
                    path_pixels.insert((x as u32, y as u32));
                }
            }
        }

        if path_pixels.is_empty() {
            // Solid-color image: use PRNG with image hash as seed
            let seed = crate::dct::image_seed(&rgb);
            crate::dct::prng_blocks_from_seed(seed, iw, ih)
        } else {
            let mut set = HashSet::new();
            for (px, py) in &path_pixels {
                let bx = px / 8;
                let by = py / 8;
                if (bx + 1) * 8 <= iw && (by + 1) * 8 <= ih {
                    set.insert((bx, by));
                }
            }
            set
        }
    };

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
            println!(
                "  blocks:    {} total, {} sampled",
                skeleton_blocks.len(),
                sample_size
            );
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
            println!(
                "  blocks:    {} total, {} sampled",
                skeleton_blocks.len(),
                sample_size
            );
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

/// DWT-domain verification: check LH sub-band coefficient bias at geometry positions.
fn verify_dwt(img: &image::DynamicImage, args: &VerifyArgs) -> Result<bool> {
    let geometry = match &args.geometry {
        Some(p) => {
            let json = std::fs::read_to_string(p)
                .with_context(|| format!("Failed to read geometry file: {:?}", p))?;
            serde_json::from_str::<GeometryFile>(&json).context("Failed to parse geometry JSON")?
        }
        None => {
            info!("No geometry file — re-extracting skeleton from image");
            extract_geometry_from_image(img)?
        }
    };

    let rgb = img.to_rgb8();
    let metrics = crate::dwt_embed::verify(&rgb, &geometry)?;

    let present = metrics.detection_rate >= args.threshold;

    if present {
        println!("WATERMARK PRESENT");
    } else {
        println!("WATERMARK ABSENT OR DESTROYED");
    }

    if args.verbose {
        println!("  mode:       dwt");
        println!("  image:      {}×{}", img.width(), img.height());
        println!("  paths:      {}", geometry.paths.len());
        println!("  coefficients: {}", metrics.total_coefficients);
        println!(
            "  detection:  {}/{} ({:.1}%)",
            metrics.detected_count,
            metrics.total_coefficients,
            metrics.detection_rate * 100.0
        );
        println!("  mean signal: {:.2}", metrics.mean_signal);
        println!("  threshold:  {:.0}%", args.threshold * 100.0);
    }

    Ok(present)
}
/// Uses the same vectomancy raster pipeline as embed, with default params.
/// Accuracy may be slightly lower than using the saved geometry file.
pub fn extract_geometry_from_image(
    img: &image::DynamicImage,
) -> Result<crate::geometry::GeometryFile> {
    use crate::geometry::{AnalysisParams, GeometryFile, PathEntry};
    use vectomancy_geometry::{chaikin_smooth_points, simplify_rdp};
    use vectomancy_raster::decode_raster_memory;

    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .context("Failed to re-encode image for geometry extraction")?;

    let (raw_paths, _) = decode_raster_memory(&buf, false)
        .map_err(|e| anyhow::anyhow!("Raster decode failed: {}", e))?;

    let tolerance = 1.5;
    let min_len = 5;
    let chaikin = 2;

    let mut paths: Vec<PathEntry> = Vec::new();
    for sp in raw_paths {
        if sp.geometry.points.len() < min_len {
            continue;
        }
        let simp = simplify_rdp(&sp.geometry.points, tolerance);
        if simp.len() < 2 {
            continue;
        }
        let smooth = chaikin_smooth_points(&simp, chaikin, false);
        paths.push(PathEntry {
            color: None,
            points: smooth.iter().map(|p| [p.x, p.y]).collect(),
        });
    }

    Ok(GeometryFile {
        version: GeometryFile::CURRENT_VERSION,
        original_width: img.width(),
        original_height: img.height(),
        analysis_params: AnalysisParams {
            detail: 60,
            min_path_len: min_len,
            chaikin_iters: chaikin,
            color: false,
        },
        paths,
        prng_seed: None,
        blocks: None,
    })
}
