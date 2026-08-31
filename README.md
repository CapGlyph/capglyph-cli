# CapGlyph

Invisible structural watermark for images — proof of origin, leak tracing,
and tamper detection.

> Formerly **Sigil** — the `sigil` binary, `com.sigil.watermark` assertion,
> and `SIGIL_*` env vars remain as aliases for compatibility.

[简体中文](README.zh-CN.md)

## What it does

CapGlyph embeds a sub-perceptual watermark into PNG/JPEG images and can later
verify its presence, extract a per-recipient ID, or prove attribution with a
secret key. Four independent embedding technologies:

| Mode      | Built with                     | Feature flag         |
| --------- | ------------------------------ | -------------------- |
| `alpha`   | sparse alpha-channel pixels    | default              |
| `dct`     | 8×8 DCT coefficient modulation | default              |
| `dwt`     | Haar LH-band modulation        | default              |
| `learned` | Adobe TrustMark CNN (ONNX)     | `--features learned` |

## Install

Prebuilt binaries for Linux, macOS, and Windows (including the `learned` and
`c2pa` features) are attached to each
[GitHub Release](https://github.com/CapGlyph/capglyph-cli/releases).

**macOS / Linux — Homebrew:**

```bash
brew tap CapGlyph/tap
brew install capglyph   # alias `sigil` still available via shim
```

**Windows — Scoop:**

```powershell
scoop bucket add capglyph https://github.com/CapGlyph/scoop-bucket
scoop install capglyph
```

**Arch Linux — AUR:**

```bash
yay -S capglyph-bin      # prebuilt binary (recommended, formerly sigil-wm-bin)
# or build from source:
yay -S capglyph          # formerly sigil-wm
```

**Linux — deb / rpm / pkg.tar.zst:** download from the
[latest release](https://github.com/CapGlyph/capglyph-cli/releases/latest).

## Build from source

```bash
cargo build --release                    # alpha/dct/dwt
cargo build --release --features learned # + learned mode (ONNX runtime)
cargo build --release --features c2pa     # + C2PA content credentials
```

## Quick start

```bash
# Embed a recipient-specific watermark
capglyph embed photo.png --mode dwt --recipient-id "alice001" --output photo_wm.png
# alias `sigil` still works: sigil embed ...

# Verify
capglyph verify photo_wm.png --mode dwt; echo $?        # 0 = present

# Extract the ID (geometry-free — works on the leaked copy)
capglyph extract leaked.png --mode dwt --id-length 8

# Keyed attribution (survives collusion attacks)
capglyph embed photo.png --mode dwt --recipient-id "bob" --key "mysecret"
capglyph verify photo_wm.png --mode dwt --key "mysecret"   # + SECRET LAYER PRESENT

# Learned mode (aggressive-edit resistance: JPEG q30, blur σ2, scale 0.5×)
capglyph fetch-models                          # downloads TrustMark ONNX (~65MB)
capglyph embed photo.png --mode learned --recipient-id "carol"
capglyph extract leaked.png --mode learned
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

CapGlyph can also sign images with C2PA content credentials — a standards-based
provenance manifest. Build with the `c2pa` cargo feature (release binaries
include it):

```bash
capglyph c2pa init --org "My Studio"          # self-signed ES256 cert + key
capglyph c2pa sign photo.jpg -o signed.jpg \
  --cert capglyph-certs/cert.pem --pkey capglyph-certs/private.key \
  --recipient-id alice01 --mode dct
capglyph c2pa verify signed.jpg               # JSON report (0 = valid, 1 = invalid, 2 = unsigned)

# Dual-layer: pixel watermark + provenance manifest in one step
capglyph embed photo.png -m dct --recipient-id alice01 --c2pa \
  --cert capglyph-certs/cert.pem --pkey capglyph-certs/private.key
capglyph verify photo_capglyph.png --c2pa
# legacy paths/certs at sigil-certs/ and `com.sigil.watermark` still read
```

The manifest's `com.capglyph.watermark` assertion (legacy `com.sigil.watermark`
still recognized) records the watermark mode, recipient ID, and keyed flag — so
the pixel layer and the manifest cross-reference each other. The
`c2pa.created` action's digital source type defaults to `digitalCapture`;
override with `--source-type` (`capture | algorithmic | composite | trained` or
a full IPTC URI).

**Trust model:** certificates are self-signed, so a valid signature proves
"was signed by the holder of this key", not "was signed by a known entity".
Pin the reported signer CN + validity window for real provenance. The `--key`
HMAC secret is never written into the manifest (only a `keyed: true` flag).

## Placement Strategies (Evaluation)

For empirical evaluation and baseline comparisons, CapGlyph supports three block-placement strategies (configurable via `--placement`):

- `skeleton` (default): Places the watermark along the image's geometric topology paths (edges and ridges).
- `edge`: A competitive baseline that targets standard high-variance edge blocks.
- `prng`: An internal control that distributes the watermark uniform-randomly across the image.

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
separately from Adobe's CDN — not distributed with CapGlyph).
