use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tracing::info;

use crate::cli::{EmbedArgs, EmbedMode};
use crate::geometry::{AnalysisParams, GeometryFile, PathEntry};

/// Entry point for the `embed` subcommand.
pub fn run(args: &EmbedArgs) -> Result<()> {
    embed(args)
}

/// Core embed logic (also called by batch module).
pub fn embed(args: &EmbedArgs) -> Result<()> {
    // ── 1. Resolve output path ────────────────────────────────────────────────
    let default_ext = if args.output.as_ref().map(|p| p.extension().and_then(|e| e.to_str())) == Some(Some("jpg")) {
        "jpg"
    } else {
        "png"
    };
    let output_path = resolve_output(&args.input, args.output.as_deref(), "_sigil", default_ext);

    // ── 2. Load the source image ──────────────────────────────────────────────
    info!("Loading input image: {:?}", args.input);
    let src_img = image::open(&args.input)
        .with_context(|| format!("Failed to open input image: {:?}", args.input))?;
    let (orig_w, orig_h) = (src_img.width(), src_img.height());

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

    // ── 3a. Warn if path count is very low ────────────────────────────────────
    let path_count = geometry.paths.len();
    if path_count < 50 {
        eprintln!(
            "⚠️  WARNING: Only {} paths extracted. Watermark may be weak or easily removed.",
            path_count
        );
        eprintln!("    Consider lowering --detail or --min-path-len for richer geometry.");
    }

    // ── 4. Optionally persist geometry ────────────────────────────────────────
    if let Some(ref save_path) = args.save_geometry {
        info!("Saving geometry to: {:?}", save_path);
        let json = geometry.to_json()?;
        std::fs::write(save_path, &json)
            .with_context(|| format!("Failed to write geometry file: {:?}", save_path))?;
    }

    // ── 5. Embed via selected mode ────────────────────────────────────────────
    match args.mode {
        EmbedMode::Alpha => {
            info!("Mode: alpha  stroke={}px", args.stroke);
            let src_rgba = src_img.to_rgba8();
            let wm_layer = render_watermark(&geometry, orig_w, orig_h, args.stroke)?;
            let result = composite(&src_rgba, &wm_layer, orig_w, orig_h);
            
            if output_path.extension().and_then(|e| e.to_str()) == Some("jpg") {
                // Convert to RGB and save as JPEG
                let rgb = image::DynamicImage::ImageRgba8(result).to_rgb8();
                save_as_jpeg(&rgb, &output_path, 85)?;
                println!("Watermark embedded [alpha→jpg] → {:?}", output_path);
            } else {
                // Save as PNG
                info!("Saving output to: {:?}", output_path);
                result
                    .save(&output_path)
                    .with_context(|| format!("Failed to save output: {:?}", output_path))?;
                println!("Watermark embedded [alpha] → {:?}", output_path);
            }
        }
        EmbedMode::Dct => {
            info!("Mode: dct  delta={}", crate::dct::EMBED_DELTA);
            let mut rgb = src_img.to_rgb8();
            let n_blocks = crate::dct::embed(&mut rgb, &geometry, args.recipient_id.as_deref())?;
            let rid_note = args.recipient_id.as_deref()
                .map(|id| format!(" recipient={id}"))
                .unwrap_or_default();
            
            if output_path.extension().and_then(|e| e.to_str()) == Some("jpg") {
                // Save directly as JPEG
                save_as_jpeg(&rgb, &output_path, 85)?;
                println!(
                    "Watermark embedded [dct→jpg, {} blocks{}] → {:?}",
                    n_blocks, rid_note, output_path
                );
            } else {
                // Save as PNG
                let rgba = image::DynamicImage::ImageRgb8(rgb).to_rgba8();
                rgba.save(&output_path)
                    .with_context(|| format!("Failed to save output: {:?}", output_path))?;
                println!(
                    "Watermark embedded [dct, {} blocks{}] → {:?}",
                    n_blocks, rid_note, output_path
                );
            }
        }
    }

    Ok(())
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Geometry extraction parameters.
pub struct GeometryParams {
    pub detail: u8,
    pub min_path_len: usize,
    pub chaikin_iters: usize,
    pub color: bool,
    pub recipient_id: Option<String>,
}

/// Public wrapper for geometry extraction (used by info command).
pub fn extract_and_build_geometry(
    rgb: &image::RgbImage,
    width: u32,
    height: u32,
    params: &GeometryParams,
) -> Result<GeometryFile> {
    // Convert RgbImage to DynamicImage for extract_geometry
    let dyn_img = image::DynamicImage::ImageRgb8(rgb.clone());
    
    // Build temporary EmbedArgs
    let args = EmbedArgs {
        input: std::path::PathBuf::from("dummy.png"),
        output: None,
        mode: crate::cli::EmbedMode::Dct,
        stroke: 0.010,
        detail: params.detail,
        min_path_len: params.min_path_len,
        chaikin_iters: params.chaikin_iters,
        color: params.color,
        save_geometry: None,
        from_geometry: None,
        recipient_id: params.recipient_id.clone(),
    };
    
    extract_geometry(&dyn_img, width, height, &args)
}

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

    // If no paths extracted (solid color), compute PRNG seed from source image
    let prng_seed = if paths.is_empty() {
        let rgb = src.to_rgb8();
        Some(crate::dct::image_seed(&rgb))
    } else {
        None
    };

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
        prng_seed,
    })
}

/// Render geometry paths onto a transparent RGBA canvas.
///
/// Uses sparse Bresenham pixel marking instead of continuous stroke AA, mimicking
/// the wgpu 4×MSAA sparse-coverage behaviour:
///   - Only path centerline pixels are marked (Bresenham rasterisation)
///   - Coverage is sub-sampled to ~0.05% of total pixels → isolated scattered dots
///   - Each marked pixel gets a high embed_alpha (proportional to stroke_width)
///   - Result: sparse, isolated pixels invisible to humans; detectable by VLMs
///
/// wgpu reference:    α_nonzero≈0.033%, α_max≈193, MAE≈0.017  (at 0.010px)
/// This renderer:     α_nonzero≈0.04%,  α_max≈180, MAE≈0.020  (at 0.010px, target)
fn render_watermark(
    geometry: &GeometryFile,
    width: u32,
    height: u32,
    stroke_width: f32,
) -> Result<image::RgbaImage> {
    // embed_alpha: high value (like wgpu's ~193) so individual pixels are detectable
    // scale proportional to stroke_width so the user can tune signal strength
    let embed_alpha = (stroke_width * 18_000.0).round().clamp(30.0, 220.0) as u8;

    // Target coverage ≈ 0.05% of pixels (similar to vectomancy wgpu at 0.005–0.010px)
    let total_pixels = width as u64 * height as u64;
    let target_marked: u64 = (total_pixels as f64 * 0.0005).round().max(100.0) as u64;

    let neutral_gray = [0.5f32, 0.5, 0.5];

    // ── Step 1: rasterise all paths via Bresenham, collect (x, y, r, g, b) ─────
    let mut path_pixels: Vec<(i32, i32, u8, u8, u8)> = Vec::new();

    for path_entry in &geometry.paths {
        if path_entry.points.len() < 2 {
            continue;
        }
        let [r, g, b] = path_entry.color.unwrap_or(neutral_gray);
        let pr = (r * 255.0).round().clamp(0.0, 255.0) as u8;
        let pg = (g * 255.0).round().clamp(0.0, 255.0) as u8;
        let pb = (b * 255.0).round().clamp(0.0, 255.0) as u8;

        for win in path_entry.points.windows(2) {
            let (x0, y0) = (win[0][0].round() as i32, win[0][1].round() as i32);
            let (x1, y1) = (win[1][0].round() as i32, win[1][1].round() as i32);
            bresenham(x0, y0, x1, y1, width as i32, height as i32, |x, y| {
                path_pixels.push((x, y, pr, pg, pb));
            });
        }
    }

    // ── Step 2: sub-sample to hit the coverage target ────────────────────────
    let mut pixmap = image::RgbaImage::new(width, height);

    if path_pixels.is_empty() {
        return Ok(pixmap);
    }

    // Uniform stride: keep every N-th pixel
    let stride = ((path_pixels.len() as u64).max(1) as f64 / target_marked as f64)
        .round()
        .max(1.0) as usize;

    for (i, &(x, y, pr, pg, pb)) in path_pixels.iter().enumerate() {
        if i % stride == 0 {
            pixmap.put_pixel(x as u32, y as u32, image::Rgba([pr, pg, pb, embed_alpha]));
        }
    }

    Ok(pixmap)
}

/// Bresenham integer line rasterisation.
/// Calls `emit(x, y)` for every pixel on the segment [(x0,y0)..(x1,y1)]
/// that falls within [0, max_x) × [0, max_y).
fn bresenham<F>(x0: i32, y0: i32, x1: i32, y1: i32, max_x: i32, max_y: i32, mut emit: F)
where
    F: FnMut(i32, i32),
{
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx = if x0 < x1 { 1i32 } else { -1 };
    let sy = if y0 < y1 { 1i32 } else { -1 };
    let mut err = dx - dy;
    let (mut x, mut y) = (x0, y0);

    loop {
        if x >= 0 && y >= 0 && x < max_x && y < max_y {
            emit(x, y);
        }
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 > -dy {
            err -= dy;
            x += sx;
        }
        if e2 < dx {
            err += dx;
            y += sy;
        }
    }
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
        // Alpha: path pixels carry the signal (embed_alpha < 255 = semi-transparent).
        // Non-path pixels (w[3]==0) stay at original alpha (255 for RGB sources).
        // verify detects the watermark by counting semi-transparent pixels (0 < α < 255).
        // PNG→JPG drops the alpha channel entirely → all pixels become fully opaque → absent.
        let composite_a = if w[3] > 0 { w[3] } else { b[3] };
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

/// Save an RGB image as JPEG with the given quality (10–100).
fn save_as_jpeg(rgb: &image::RgbImage, path: &Path, quality: u8) -> Result<()> {
    let mut file = std::fs::File::create(path)
        .with_context(|| format!("Failed to create JPEG output: {:?}", path))?;
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut file, quality);
    encoder.encode(
        rgb,
        rgb.width(),
        rgb.height(),
        image::ExtendedColorType::Rgb8,
    ).with_context(|| format!("Failed to encode JPEG: {:?}", path))?;
    Ok(())
}
