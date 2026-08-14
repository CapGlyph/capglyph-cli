# Sigil — MVP Specification v0.1

**Project:** Sigil
**Version:** 0.1.0
**Date:** 2026-08-14
**Status:** Draft

---

## 1. What Sigil Is

Sigil embeds an invisible structural watermark into images.

The watermark is derived from the image's own geometry (via the Vectomancy raster pipeline) and
rendered at sub-perceptual stroke width onto a transparent RGBA layer that is composited onto
the original image. The result is visually identical to the original for human viewers, but
carries a machine-readable structural signature that survives as long as the PNG alpha channel
is preserved.

**Core value proposition:**

- Human: sees the original image, unmodified in appearance
- VLM / detector at effort=none: cannot recover watermark content (signal below T_V)
- VLM / detector at effort=medium: can detect structural signature (above T_V)
- After PNG→JPG conversion or screenshot: watermark destroyed (detects tampering)

Sigil is NOT:

- A steganographic message encoder (no payload bits — structural fingerprint only)
- A general image editor or converter
- A VLM bypass / adversarial attack tool

---

## 2. MVP Scope (v0.1)

Three subcommands only. Nothing else.

### 2.1 `sigil embed`

```
sigil embed <input.png> [options]
  --stroke <f32>         Watermark stroke width (default: 0.002)
  --output <path>        Output path (default: <stem>_sigil.png)
  --save-geometry <path> Save extracted geometry JSON for re-use (optional)
  --color                Sample original colors (default: false, use neutral gray)
  --detail <1-100>       Vectomancy detail level (default: 60)
  --min-path-len <n>     Minimum path length (default: 5)
  --from-geometry <path> Skip re-analysis, load geometry JSON (optional)
```

**Behavior:**

1. Load input PNG (RGBA or RGB)
2. Run Vectomancy raster pipeline (Sobel + Otsu + Zhang-Suen + Chaikin) on the image
3. Render skeleton paths at `--stroke` width onto a transparent RGBA canvas same size as input
4. Alpha-composite the watermark layer OVER the original image
5. Save result as PNG (RGBA, lossless)
6. If `--save-geometry` is specified, save the extracted path data as JSON
7. If `--from-geometry` is specified, skip steps 2 and load JSON instead (fast re-render)

Output file is always PNG RGBA. No JPG output — that would destroy the watermark.

### 2.2 `sigil verify`

```
sigil verify <image.png> [options]
  --threshold <f32>    Alpha nonzero fraction threshold for "present" (default: 0.0001)
  --verbose            Print full signal statistics
```

**Behavior:**

1. Load image (RGBA only; RGB = no alpha channel = watermark absent)
2. Compute signal metrics on the alpha channel:
   - `alpha_nonzero_frac`: fraction of pixels with alpha > 0
   - `alpha_mean`, `alpha_max`, `alpha_p99`
   - `composite_mae`: mean absolute error vs pure white after compositing
3. If `alpha_nonzero_frac >= threshold`: print "WATERMARK PRESENT" + metrics
4. If `alpha_nonzero_frac < threshold`: print "WATERMARK ABSENT OR DESTROYED"
5. Exit code: 0 = present, 1 = absent, 2 = error

**Signal thresholds (from benchmark data):**

- w=0.001px → alpha_nonzero ≈ 0% (below rendering floor — absent)
- w=0.002px → alpha_nonzero ≈ 0.00% but alpha_max=192 (borderline)
- w=0.005px → alpha_nonzero ≈ 0.03% (present, default embed width is 0.002)
- After PNG→JPG: alpha channel dropped → absent → tamper detected

### 2.3 `sigil strip`

```
sigil strip <image.png> [options]
  --output <path>   Output path (default: <stem>_stripped.png)
```

**Behavior:**

1. Load image as RGBA
2. Set alpha channel to 255 for all pixels (fully opaque)
3. Save as PNG RGB (no alpha)

This produces a clean version with the watermark destroyed, primarily useful for testing
and for generating the white-composite preview.

---

## 3. Technical Architecture

```
sigil/
├── src/
│   ├── main.rs          # CLI entry point (clap)
│   ├── cli.rs           # Subcommand definitions
│   ├── embed.rs         # embed subcommand implementation
│   ├── verify.rs        # verify subcommand implementation
│   ├── strip.rs         # strip subcommand implementation
│   ├── geometry.rs      # Geometry JSON serialization/deserialization
│   └── signal.rs        # Alpha channel signal metrics
├── tests/
│   └── integration.rs   # End-to-end tests
├── docs/
│   └── mvp-spec.md      # This file
└── Cargo.toml
```

### 3.1 Dependencies

```toml
clap = "4.5"                    # CLI
image = "0.25"                  # PNG/JPEG I/O
vectomancy = { path = "../vectomancy", default-features = false }
vectomancy-raster = { path = "../vectomancy/crates/vectomancy-raster" }
vectomancy-geometry = { path = "../vectomancy/crates/vectomancy-geometry" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"
tracing = "0.1"
tracing-subscriber = "0.3"
```

### 3.2 Geometry JSON Format (internal, v1)

```json
{
  "version": 1,
  "original_width": 1254,
  "original_height": 1254,
  "analysis_params": {
    "detail": 60,
    "min_path_len": 5,
    "chaikin_iters": 3,
    "color": false
  },
  "paths": [
    {
      "color": [0.5, 0.5, 0.5],
      "points": [[x0,y0],[x1,y1],...]
    }
  ]
}
```

This is Sigil's own format, not Vectomancy's. Intentionally simple — polyline points only,
no Fourier/Spline math. Fast to serialize/deserialize, stable across versions.

### 3.3 Rendering

The watermark layer is rendered using the same wgpu/lyon pipeline as Vectomancy's native
raster emitter:

- `lyon::StrokeTessellator` with `StrokeOptions::default().with_line_width(stroke_width)`
- wgpu render into `Rgba8UnormSrgb` with 4× MSAA, ALPHA_BLENDING, transparent clear color
- Result composited over original via `image::imageops::overlay`

For v0.1, no GPU dependency is required — fall back to CPU software rasterization if wgpu
is unavailable.

---

## 4. What MVP Deliberately Excludes

These are out of scope for v0.1 and may be addressed in later versions:

- Per-recipient unique watermark variants (distribution fingerprinting)
- Adversarial perturbation mode (requires gradient-based optimization)
- Batch processing of directories
- Web API / service mode
- WASM compilation target
- Any output format other than PNG

---

## 5. Success Criteria for v0.1

- [ ] `sigil embed photo.png` produces a PNG visually identical to input (PSNR > 60dB at w=0.002)
- [ ] `sigil verify photo_sigil.png` exits 0 and prints "WATERMARK PRESENT"
- [ ] `sigil verify photo_sigil.jpg` (after convert) exits 1 and prints "WATERMARK ABSENT"
- [ ] `sigil strip photo_sigil.png` produces a clean RGB PNG with no alpha
- [ ] `sigil embed photo.png --from-geometry photo.json` produces identical output to
      the original embed (deterministic geometry re-render)
- [ ] `cargo test` passes all integration tests
- [ ] `cargo clippy -- -D warnings` clean
- [ ] Binary size < 20MB release build

---

## 6. CarryCtx Tasks

| Task              | ID       | Priority |
| ----------------- | -------- | -------- |
| Write MVP spec    | CTX-0001 | high     |
| Wire CLI skeleton | CTX-0005 | high     |
| Implement embed   | CTX-0002 | high     |
| Implement verify  | CTX-0003 | high     |
| Implement strip   | CTX-0004 | normal   |
| Integration tests | CTX-0006 | normal   |
