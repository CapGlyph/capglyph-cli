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

## Content Credentials (C2PA)

Sigil can also sign images with C2PA content credentials — a standards-based
provenance manifest. Build with the `c2pa` cargo feature (release binaries
include it):

```bash
sigil c2pa init --org "My Studio"          # self-signed ES256 cert + key
sigil c2pa sign photo.jpg -o signed.jpg \
  --cert sigil-certs/cert.pem --pkey sigil-certs/private.key \
  --recipient-id alice01 --mode dct
sigil c2pa verify signed.jpg               # JSON report (0 = valid, 1 = invalid, 2 = unsigned)

# Dual-layer: pixel watermark + provenance manifest in one step
sigil embed photo.png -m dct --recipient-id alice01 --c2pa \
  --cert sigil-certs/cert.pem --pkey sigil-certs/private.key
sigil verify photo_sigil.png --c2pa
```

The manifest's `com.sigil.watermark` assertion records the watermark mode,
recipient ID, and keyed flag — so the pixel layer and the manifest
cross-reference each other. The `c2pa.created` action's digital source type
defaults to `digitalCapture`; override with `--source-type`
(`capture | algorithmic | composite | trained` or a full IPTC URI).

**Trust model:** certificates are self-signed, so a valid signature proves
"was signed by the holder of this key", not "was signed by a known entity".
Pin the reported signer CN + validity window for real provenance. The `--key`
HMAC secret is never written into the manifest (only a `keyed: true` flag).

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
