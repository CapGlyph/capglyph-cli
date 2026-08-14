/// Integration tests for Sigil embed / verify / strip pipeline.
///
/// Each test generates a small synthetic PNG in-process (no fixture files needed)
/// and runs the subcommand logic directly via the public module functions.
use std::path::PathBuf;
use std::sync::OnceLock;
use tempfile::TempDir;

// ── Synthetic test fixture ────────────────────────────────────────────────────

/// A 64×64 solid red RGB PNG — smallest image that exercises the full raster pipeline.
fn make_test_png(dir: &TempDir) -> PathBuf {
    let path = dir.path().join("test_input.png");
    let img = image::RgbImage::from_fn(64, 64, |x, y| {
        // Alternating black/white 4-px checkerboard so Sobel finds edges
        let cell_x = x / 4;
        let cell_y = y / 4;
        if (cell_x + cell_y) % 2 == 0 {
            image::Rgb([255u8, 255, 255])
        } else {
            image::Rgb([0u8, 0, 0])
        }
    });
    img.save(&path).unwrap();
    path
}

// ── Helpers that mirror CLI behaviour ────────────────────────────────────────

use sigil::cli::{EmbedArgs, StripArgs, VerifyArgs};
use sigil::{embed, strip, verify};

fn default_embed_args(input: PathBuf, output: PathBuf) -> EmbedArgs {
    EmbedArgs {
        input,
        output: Some(output),
        stroke: 0.010,
        detail: 60,
        min_path_len: 3, // lower for tiny 64×64 test image
        chaikin_iters: 1,
        color: false,
        save_geometry: None,
        from_geometry: None,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[test]
fn embed_produces_rgba_png() {
    let dir = TempDir::new().unwrap();
    let input = make_test_png(&dir);
    let output = dir.path().join("out_sigil.png");

    embed::run(&default_embed_args(input, output.clone())).unwrap();

    assert!(output.exists(), "output file should be created");
    let img = image::open(&output).unwrap();
    // Output must be RGBA (carries alpha channel)
    assert!(
        matches!(img.color(), image::ColorType::Rgba8),
        "output must be RGBA8, got {:?}",
        img.color()
    );
}

#[test]
fn verify_present_after_embed() {
    let dir = TempDir::new().unwrap();
    let input = make_test_png(&dir);
    let output = dir.path().join("out_sigil.png");

    embed::run(&default_embed_args(input, output.clone())).unwrap();

    let present = verify::run(&VerifyArgs {
        input: output,
        threshold: 0.0001,
        verbose: false,
    })
    .unwrap();

    assert!(present, "watermark should be detected after embed");
}

#[test]
fn verify_absent_for_plain_rgb() {
    let dir = TempDir::new().unwrap();
    let input = make_test_png(&dir); // plain RGB PNG

    let present = verify::run(&VerifyArgs {
        input,
        threshold: 0.0001,
        verbose: false,
    })
    .unwrap();

    assert!(!present, "plain RGB image should report watermark absent");
}

#[test]
fn verify_absent_after_strip() {
    let dir = TempDir::new().unwrap();
    let input = make_test_png(&dir);
    let sigil_out = dir.path().join("out_sigil.png");
    let stripped = dir.path().join("out_stripped.png");

    embed::run(&default_embed_args(input, sigil_out.clone())).unwrap();
    strip::run(&StripArgs {
        input: sigil_out,
        output: Some(stripped.clone()),
    })
    .unwrap();

    // Strip produces RGB (no alpha)
    let img = image::open(&stripped).unwrap();
    assert!(
        matches!(img.color(), image::ColorType::Rgb8),
        "stripped output must be Rgb8, got {:?}",
        img.color()
    );

    let present = verify::run(&VerifyArgs {
        input: stripped,
        threshold: 0.0001,
        verbose: false,
    })
    .unwrap();

    assert!(!present, "watermark should be absent after strip");
}

#[test]
fn from_geometry_matches_full_run() {
    let dir = TempDir::new().unwrap();
    let input = make_test_png(&dir);
    let geo_path = dir.path().join("geometry.json");
    let out1 = dir.path().join("out1.png");
    let out2 = dir.path().join("out2.png");

    // First run: full analysis, save geometry
    embed::run(&EmbedArgs {
        input: input.clone(),
        output: Some(out1.clone()),
        save_geometry: Some(geo_path.clone()),
        ..default_embed_args(input.clone(), out1.clone())
    })
    .unwrap();

    // Second run: load geometry (skip analysis)
    embed::run(&EmbedArgs {
        input: input.clone(),
        output: Some(out2.clone()),
        from_geometry: Some(geo_path),
        // analysis flags ignored when from_geometry is set
        detail: 60,
        min_path_len: 3,
        chaikin_iters: 1,
        color: false,
        stroke: 0.010,
        save_geometry: None,
    })
    .unwrap();

    // Both outputs must have the same dimensions and signal characteristics
    let img1 = image::open(&out1).unwrap().to_rgba8();
    let img2 = image::open(&out2).unwrap().to_rgba8();

    assert_eq!(img1.dimensions(), img2.dimensions());

    // Alpha channels must be identical (geometry → render is deterministic)
    let all_alpha_equal = img1
        .pixels()
        .zip(img2.pixels())
        .all(|(p1, p2)| p1[3] == p2[3]);
    assert!(
        all_alpha_equal,
        "alpha channels must be identical between full-run and from-geometry re-render"
    );
}

#[test]
fn geometry_json_roundtrip() {
    use sigil::geometry::{AnalysisParams, GeometryFile, PathEntry};

    let geo = GeometryFile {
        version: GeometryFile::CURRENT_VERSION,
        original_width: 100,
        original_height: 100,
        analysis_params: AnalysisParams {
            detail: 60,
            min_path_len: 5,
            chaikin_iters: 3,
            color: false,
        },
        paths: vec![
            PathEntry {
                color: Some([1.0, 0.0, 0.0]),
                points: vec![[10.0, 20.0], [30.0, 40.0], [50.0, 60.0]],
            },
            PathEntry {
                color: None,
                points: vec![[0.0, 0.0], [99.0, 99.0]],
            },
        ],
    };

    let json = geo.to_json().unwrap();
    let restored = GeometryFile::from_json(&json).unwrap();

    assert_eq!(restored.version, GeometryFile::CURRENT_VERSION);
    assert_eq!(restored.original_width, 100);
    assert_eq!(restored.paths.len(), 2);
    assert_eq!(restored.paths[0].points.len(), 3);
    assert!(restored.paths[0].color.is_some());
    assert!(restored.paths[1].color.is_none());
}

#[test]
fn signal_metrics_zero_for_blank_rgba() {
    use sigil::signal::SignalMetrics;

    // All-transparent image
    let pixels = vec![0u8; 64 * 64 * 4];
    let m = SignalMetrics::compute(&pixels, 64, 64);

    assert_eq!(m.nonzero_alpha_frac, 0.0);
    assert_eq!(m.alpha_max, 0);
    assert_eq!(m.composite_mae, 0.0);
    assert!(!m.is_present(0.0001));
}

#[test]
fn signal_metrics_full_for_opaque_rgba() {
    use sigil::signal::SignalMetrics;

    // All red, fully opaque
    let mut pixels = vec![0u8; 64 * 64 * 4];
    for chunk in pixels.chunks_exact_mut(4) {
        chunk[0] = 255; // R
        chunk[1] = 0; // G
        chunk[2] = 0; // B
        chunk[3] = 255; // A
    }
    let m = SignalMetrics::compute(&pixels, 64, 64);

    assert_eq!(m.nonzero_alpha_frac, 1.0);
    assert_eq!(m.alpha_max, 255);
    assert!(m.composite_mae > 0.0); // red differs from white
    assert!(m.is_present(0.0001));
}
