use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tracing::info;

use crate::cli::EmbedArgs;
use crate::geometry::{AnalysisParams, GeometryFile, PathEntry};

/// Entry point for the `embed` subcommand.
pub fn run(args: &EmbedArgs) -> Result<()> {
    // ── 1. Resolve output path ────────────────────────────────────────────────
    let output_path = resolve_output(&args.input, args.output.as_deref(), "_sigil", "png");

    // ── 2. Load the source image ──────────────────────────────────────────────
    info!("Loading input image: {:?}", args.input);
    let src_img = image::open(&args.input)
        .with_context(|| format!("Failed to open input image: {:?}", args.input))?;
    let (orig_w, orig_h) = (src_img.width(), src_img.height());
    let src_rgba = src_img.to_rgba8();

    // ── 3. Obtain geometry (analyse or load from file) ────────────────────────
    let geometry = if let Some(ref geo_path) = args.from_geometry {
        info!("Loading geometry from: {:?}", geo_path);
        let bytes = std::fs::read(geo_path)
            .with_context(|| format!("Failed to read geometry file: {:?}", geo_path))?;
        GeometryFile::from_json(&bytes)?
    } else {
        info!(
            "Analysing image geometry (detail={}, min_path_len={}, chaikin_iters={})",
            args.detail, args.min_path_len, args.chaikin_iters
        );
        extract_geometry(&src_img, orig_w, orig_h, args)?
    };

    // ── 4. Optionally persist geometry ────────────────────────────────────────
    if let Some(ref save_path) = args.save_geometry {
        info!("Saving geometry to: {:?}", save_path);
        let json = geometry.to_json()?;
        std::fs::write(save_path, &json)
            .with_context(|| format!("Failed to write geometry file: {:?}", save_path))?;
    }

    // ── 5. Render watermark layer ─────────────────────────────────────────────
    info!("Rendering watermark layer (stroke={}px)", args.stroke);
    let wm_layer = render_watermark(&geometry, orig_w, orig_h, args.stroke)?;

    // ── 6. Composite watermark over original ──────────────────────────────────
    info!("Compositing watermark over original image");
    let result = composite(&src_rgba, &wm_layer, orig_w, orig_h);

    // ── 7. Save output ────────────────────────────────────────────────────────
    info!("Saving output to: {:?}", output_path);
    result
        .save(&output_path)
        .with_context(|| format!("Failed to save output: {:?}", output_path))?;

    println!("Watermark embedded → {:?}", output_path);
    Ok(())
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Run the vectomancy-raster pipeline and apply Chaikin smoothing.
fn extract_geometry(
    src: &image::DynamicImage,
    width: u32,
    height: u32,
    args: &EmbedArgs,
) -> Result<GeometryFile> {
    use vectomancy_geometry::{chaikin_smooth_points, simplify_rdp};
    use vectomancy_raster::decode_raster_memory;

    let tolerance = {
        let d = args.detail.clamp(1, 100) as f64;
        5.0 * (1.0 - d / 100.0).powi(2) + 0.1
    };

    let bytes = {
        let mut buf = Vec::new();
        src.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .context("Failed to encode image for raster pipeline")?;
        buf
    };

    let (raw_paths, _dims) = decode_raster_memory(&bytes, args.color)
        .map_err(|e| anyhow::anyhow!("Raster decode failed: {}", e))?;

    let mut paths: Vec<PathEntry> = Vec::new();
    for sp in raw_paths {
        if sp.geometry.points.len() < args.min_path_len {
            continue;
        }
        let simplified = simplify_rdp(&sp.geometry.points, tolerance);
        if simplified.len() < 2 {
            continue;
        }
        let smoothed = if args.chaikin_iters > 0 {
            chaikin_smooth_points(&simplified, args.chaikin_iters, false)
        } else {
            simplified
        };

        let color = sp.color_style.as_deref().and_then(parse_hex_color);
        paths.push(PathEntry {
            color,
            points: smoothed.iter().map(|p| [p.x, p.y]).collect(),
        });
    }

    info!("Extracted {} paths from image", paths.len());

    Ok(GeometryFile {
        version: GeometryFile::CURRENT_VERSION,
        original_width: width,
        original_height: height,
        analysis_params: AnalysisParams {
            detail: args.detail,
            min_path_len: args.min_path_len,
            chaikin_iters: args.chaikin_iters,
            color: args.color,
        },
        paths,
    })
}

/// Render geometry paths onto a transparent RGBA canvas at the given stroke width.
///
/// Uses tiny-skia for software anti-aliased rasterization.
/// Sub-pixel stroke widths (e.g. 0.002px) produce fractional coverage per pixel,
/// which manifests as low-alpha pixels — the "invisible ink" signal.
///
/// Returns a straight-alpha RGBA image.
fn render_watermark(
    geometry: &GeometryFile,
    width: u32,
    height: u32,
    stroke_width: f32,
) -> Result<image::RgbaImage> {
    use tiny_skia::{Paint, PathBuilder, Pixmap, Stroke, Transform};

    // tiny-skia renders to premultiplied RGBA
    let mut pixmap = Pixmap::new(width, height)
        .ok_or_else(|| anyhow::anyhow!("Failed to allocate {}×{} pixmap", width, height))?;

    let stroke = Stroke {
        width: stroke_width,
        ..Default::default()
    };

    let neutral_gray = [0.5f32, 0.5, 0.5];

    for path_entry in &geometry.paths {
        if path_entry.points.len() < 2 {
            continue;
        }

        // Build the path
        let mut builder = PathBuilder::new();
        let first = &path_entry.points[0];
        builder.move_to(first[0] as f32, first[1] as f32);
        for pt in &path_entry.points[1..] {
            builder.line_to(pt[0] as f32, pt[1] as f32);
        }
        let skia_path = match builder.finish() {
            Some(p) => p,
            None => continue,
        };

        // Paint color
        let [r, g, b] = path_entry.color.unwrap_or(neutral_gray);
        let mut paint = Paint::default();
        paint.set_color(
            tiny_skia::Color::from_rgba(r, g, b, 1.0)
                .unwrap_or(tiny_skia::Color::from_rgba(0.5, 0.5, 0.5, 1.0).unwrap()),
        );
        paint.anti_alias = true;

        pixmap.stroke_path(&skia_path, &paint, &stroke, Transform::identity(), None);
    }

    // tiny-skia produces premultiplied RGBA; convert to straight alpha for `image`.
    // premult: R_p = R_s * A / 255  →  straight: R_s = R_p * 255 / A
    let raw = pixmap.data(); // &[u8], RGBA premultiplied, row-major
    let mut out = image::RgbaImage::new(width, height);
    for (i, chunk) in raw.chunks_exact(4).enumerate() {
        let x = (i as u32) % width;
        let y = (i as u32) / width;
        let (rp, gp, bp, a) = (chunk[0], chunk[1], chunk[2], chunk[3]);
        let (rs, gs, bs) = if a == 0 {
            (0u8, 0u8, 0u8)
        } else {
            let af = a as u16;
            (
                ((rp as u16 * 255 + af / 2) / af).min(255) as u8,
                ((gp as u16 * 255 + af / 2) / af).min(255) as u8,
                ((bp as u16 * 255 + af / 2) / af).min(255) as u8,
            )
        };
        out.put_pixel(x, y, image::Rgba([rs, gs, bs, a]));
    }

    Ok(out)
}

/// Alpha-composite `watermark` OVER `base`.
/// For each pixel: out_rgb = wm_alpha * wm_rgb + (1 - wm_alpha) * base_rgb
///                 out_alpha = base_alpha (preserve original alpha if any)
fn composite(
    base: &image::RgbaImage,
    watermark: &image::RgbaImage,
    width: u32,
    height: u32,
) -> image::RgbaImage {
    assert_eq!(base.dimensions(), (width, height));
    assert_eq!(watermark.dimensions(), (width, height));

    let mut out = image::RgbaImage::new(width, height);
    for (x, y, out_px) in out.enumerate_pixels_mut() {
        let b = base.get_pixel(x, y);
        let w = watermark.get_pixel(x, y);
        let wa = w[3] as f32 / 255.0;
        let composite_r = (wa * w[0] as f32 + (1.0 - wa) * b[0] as f32).round() as u8;
        let composite_g = (wa * w[1] as f32 + (1.0 - wa) * b[1] as f32).round() as u8;
        let composite_b = (wa * w[2] as f32 + (1.0 - wa) * b[2] as f32).round() as u8;
        // Alpha channel carries the watermark signal.
        // For opaque originals (b[3]=255), we deliberately use the watermark
        // alpha as the output alpha — this makes the signal visible to VLMs
        // via alpha channel inspection while keeping RGB ≈ original (the
        // watermark RGB contribution is scaled by its tiny alpha).
        // When composited over white in an image viewer, the result looks
        // identical to the original. PNG→JPG conversion drops this alpha
        // channel entirely, which is how tampering is detected.
        let composite_a = w[3];
        *out_px = image::Rgba([composite_r, composite_g, composite_b, composite_a]);
    }
    out
}

/// Parse a CSS hex color string like `#rrggbb` → `[r, g, b]` in 0.0–1.0.
fn parse_hex_color(s: &str) -> Option<[f32; 3]> {
    let s = s.trim().strip_prefix('#')?;
    if s.len() != 6 || !s.is_ascii() {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()? as f32 / 255.0;
    let g = u8::from_str_radix(&s[2..4], 16).ok()? as f32 / 255.0;
    let b = u8::from_str_radix(&s[4..6], 16).ok()? as f32 / 255.0;
    Some([r, g, b])
}

/// Build a default output path: `<parent>/<stem><suffix>.<ext>`
pub fn resolve_output(input: &Path, override_: Option<&Path>, suffix: &str, ext: &str) -> PathBuf {
    if let Some(p) = override_ {
        return p.to_path_buf();
    }
    let stem = input.file_stem().unwrap_or_default().to_string_lossy();
    let parent = input.parent().unwrap_or(Path::new("."));
    parent.join(format!("{}{}.{}", stem, suffix, ext))
}
