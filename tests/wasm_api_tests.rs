//! Native unit tests for the byte-in/byte-out API used by the wasm bridge.
//! These exercise the exact functions the wasm-engine exposes, no wasm needed.
use sigil::wasm_api::{embed_bytes, extract_bytes, verify_bytes};

/// Encode an in-memory 512×512 checkerboard PNG (plenty of skeleton blocks).
fn checkerboard_png(size: u32) -> Vec<u8> {
    let img = image::RgbImage::from_fn(size, size, |x, y| {
        let cx = x / 4;
        let cy = y / 4;
        if (cx + cy) % 2 == 0 {
            image::Rgb([255u8, 255, 255])
        } else {
            image::Rgb([0u8, 0, 0])
        }
    });
    let mut buf = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut buf, image::ImageFormat::Png)
        .unwrap();
    buf.into_inner()
}

#[test]
fn dct_embed_verify_roundtrip() {
    let src = checkerboard_png(512);
    let wm = embed_bytes(&src, "dct", None, None).unwrap();
    assert!(
        verify_bytes(&wm, "dct").unwrap(),
        "dct watermark should verify"
    );
}

#[test]
fn dwt_embed_verify_roundtrip() {
    let src = checkerboard_png(512);
    let wm = embed_bytes(&src, "dwt", None, None).unwrap();
    assert!(
        verify_bytes(&wm, "dwt").unwrap(),
        "dwt watermark should verify"
    );
}

#[test]
fn alpha_embed_verify_roundtrip() {
    let src = checkerboard_png(512);
    let wm = embed_bytes(&src, "alpha", None, None).unwrap();
    assert!(
        verify_bytes(&wm, "alpha").unwrap(),
        "alpha watermark should verify"
    );
}

#[test]
fn clean_image_reports_absent() {
    let src = checkerboard_png(512);
    assert!(
        !verify_bytes(&src, "dct").unwrap(),
        "clean image must not verify"
    );
}

#[test]
fn recipient_id_roundtrip() {
    let src = checkerboard_png(512);
    let wm = embed_bytes(&src, "dct", Some("alice"), None).unwrap();
    assert_eq!(extract_bytes(&wm, "dct", 5).unwrap(), "alice");
}

#[test]
fn extract_alpha_is_error() {
    let src = checkerboard_png(512);
    let wm = embed_bytes(&src, "alpha", None, None).unwrap();
    assert!(
        extract_bytes(&wm, "alpha", 5).is_err(),
        "alpha has no extractable id"
    );
}

#[test]
fn unknown_mode_is_error() {
    let src = checkerboard_png(512);
    assert!(embed_bytes(&src, "bogus", None, None).is_err());
    assert!(verify_bytes(&src, "bogus").is_err());
    assert!(extract_bytes(&src, "bogus", 5).is_err());
}

#[test]
fn learned_mode_is_error_without_feature() {
    let src = checkerboard_png(512);
    assert!(embed_bytes(&src, "learned", None, None).is_err());
}
