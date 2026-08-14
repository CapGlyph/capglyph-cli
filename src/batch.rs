use crate::cli::{BatchArgs, BatchOperation, EmbedArgs, OutputFormat, StripArgs};
use crate::{embed, strip};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Batch process multiple images (embed or strip).
pub fn batch(args: &BatchArgs) -> Result<()> {
    // Expand glob pattern
    let paths: Vec<PathBuf> = glob::glob(&args.input_pattern)
        .context("Invalid glob pattern")?
        .filter_map(Result::ok)
        .collect();

    if paths.is_empty() {
        anyhow::bail!("No files matched pattern: {}", args.input_pattern);
    }

    // Create output directory
    std::fs::create_dir_all(&args.output_dir)
        .context("Failed to create output directory")?;

    println!("Processing {} images...", paths.len());

    match args.operation {
        BatchOperation::Embed => batch_embed(args, &paths),
        BatchOperation::Strip => batch_strip(args, &paths),
    }
}

fn batch_embed(args: &BatchArgs, paths: &[PathBuf]) -> Result<()> {
    let mut success = 0;
    let mut failed = 0;

    for (i, input) in paths.iter().enumerate() {
        let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
        let ext = match args.format {
            OutputFormat::Png => "png",
            OutputFormat::Jpg => "jpg",
        };
        let output = args.output_dir.join(format!("{}_sigil.{}", stem, ext));

        print!("[{}/{}] {} → {} ... ", i + 1, paths.len(), input.display(), output.display());

        let embed_args = EmbedArgs {
            input: input.clone(),
            output: Some(output.clone()),
            mode: args.mode,
            stroke: args.stroke,
            detail: args.detail,
            min_path_len: 5,
            chaikin_iters: 3,
            color: false,
            save_geometry: None,
            from_geometry: None,
            recipient_id: args.recipient_id.clone(),
        };

        match embed::embed(&embed_args) {
            Ok(_) => {
                // Convert to JPEG if requested
                if args.format == OutputFormat::Jpg {
                    if let Err(e) = convert_to_jpeg(&output, args.jpeg_quality) {
                        println!("FAILED (JPEG conversion: {})", e);
                        failed += 1;
                        continue;
                    }
                }
                println!("OK");
                success += 1;
            }
            Err(e) => {
                println!("FAILED ({})", e);
                failed += 1;
            }
        }
    }

    println!("\nBatch embed complete: {} success, {} failed", success, failed);
    Ok(())
}

fn batch_strip(args: &BatchArgs, paths: &[PathBuf]) -> Result<()> {
    let mut success = 0;
    let mut failed = 0;

    for (i, input) in paths.iter().enumerate() {
        let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
        let output = args.output_dir.join(format!("{}_stripped.png", stem));

        print!("[{}/{}] {} → {} ... ", i + 1, paths.len(), input.display(), output.display());

        let strip_args = StripArgs {
            input: input.clone(),
            output: Some(output),
        };

        match strip::run(&strip_args) {
            Ok(_) => {
                println!("OK");
                success += 1;
            }
            Err(e) => {
                println!("FAILED ({})", e);
                failed += 1;
            }
        }
    }

    println!("\nBatch strip complete: {} success, {} failed", success, failed);
    Ok(())
}

fn convert_to_jpeg(png_path: &Path, quality: u8) -> Result<()> {
    let img = image::open(png_path)?;
    let rgb = img.to_rgb8();
    
    // Replace .png with .jpg
    let jpg_path = png_path.with_extension("jpg");
    
    // Save as JPEG
    let mut jpg_file = std::fs::File::create(&jpg_path)?;
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg_file, quality);
    encoder.encode(
        &rgb,
        rgb.width(),
        rgb.height(),
        image::ExtendedColorType::Rgb8,
    )?;
    
    // Remove intermediate PNG
    std::fs::remove_file(png_path)?;
    
    Ok(())
}
