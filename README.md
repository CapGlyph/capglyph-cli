# Sigil

Invisible structural watermark for images — proof of origin, leak tracing,
and tamper detection.

[简体中文](README.zh-CN.md)

## What it does

Sigil embeds a sub-perceptual watermark into PNG/JPEG images and can later
verify its presence, extract a per-recipient ID, or prove attribution with a
secret key. Four independent embedding technologies:

| Mode      | Built with                     | Feature flag         |
| --------- | ------------------------------ | -------------------- |
| `alpha`   | sparse alpha-channel pixels    | default              |
| `dct`     | 8×8 DCT coefficient modulation | default              |
| `dwt`     | Haar LH-band modulation        | default              |
| `learned` | Adobe TrustMark CNN (ONNX)     | `--features learned` |

## Build

```bash
cargo build --release                    # alpha/dct/dwt
cargo build --release --features learned # + learned mode (ONNX runtime)
```

## Quick start

```bash
# Embed a recipient-specific watermark
sigil embed photo.png --mode dwt --recipient-id "alice001" --output photo_wm.png

# Verify
sigil verify photo_wm.png --mode dwt; echo $?        # 0 = present

# Extract the ID (geometry-free — works on the leaked copy)
sigil extract leaked.png --mode dwt --id-length 8

# Keyed attribution (survives collusion attacks)
sigil embed photo.png --mode dwt --recipient-id "bob" --key "mysecret"
sigil verify photo_wm.png --mode dwt --key "mysecret"   # + SECRET LAYER PRESENT

# Learned mode (aggressive-edit resistance: JPEG q30, blur σ2, scale 0.5×)
sigil fetch-models                          # downloads TrustMark ONNX (~65MB)
sigil embed photo.png --mode learned --recipient-id "carol"
sigil extract leaked.png --mode learned
```

## Attack matrix (measured)

| Attack                    |      alpha      | dct |      dwt       | learned |
| ------------------------- | :-------------: | :-: | :------------: | :-----: |
| JPEG q50                  |        ✗        |  ✓  |       ✓        |    ✓    |
| JPEG q30                  |        ✗        |  ✗  |       ✗        |  **✓**  |
| blur σ2.0                 |        ✗        |  ✗  |  verify✓/ID✗   |  **✓**  |
| scale 0.5×                |        ✗        |  ✗  |       ✗        |  **✓**  |
| collusion (5-copy median) |        —        |  —  | secret layer ✓ |    —    |
| known-cover diff          | ✗ (unavoidable) |  ✗  |       ✗        |    ✗    |
| img2img regeneration      |        ✗        |  ✗  |       ✗        |    ✗    |

## Security model

- **Public layer** — presence detection (`verify`)
- **ID layer** — per-recipient tracing (`extract`, geometry-free)
- **Secret layer** — HMAC-keyed attribution (`--key`), survives collusion,
  blocks forgery

Hard limits (shared by all pixel watermarks): an attacker with the original
can always diff-remove the watermark, and generative regeneration (img2img)
defeats it at denoising strength ≥0.3.

## Documentation

- `docs/mvp-spec.md` — full specification
- `docs/product-roadmap.md` — product/B2B direction
- `CHANGELOG.md` — release history

## License

Apache-2.0. Learned mode embeds Adobe TrustMark models (MIT, downloaded
separately from Adobe's CDN — not distributed with Sigil).
