# CapGlyph

Image watermark, payload recovery, and signed-provenance tooling.

[简体中文](README.zh-CN.md)

## What it does

CapGlyph embeds a watermark into PNG/JPEG images and can later measure its
presence, extract a per-recipient ID, or test a keyed signal. Results depend on
the carrier, image, payload and transformations; none independently proves
authorship, ownership, or factual provenance. Four embedding technologies:

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
brew install capglyph
```

**Windows — Scoop:**

```powershell
scoop bucket add capglyph https://github.com/CapGlyph/scoop-bucket
scoop install capglyph
```

**Arch Linux — AUR:**

```bash
yay -S capglyph-bin      # prebuilt binary (recommended)
# or build from source:
yay -S capglyph
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

# Verify
capglyph verify photo_wm.png --mode dwt; echo $?        # 0 = present

# Extract the ID (geometry-free — works on the leaked copy)
capglyph extract leaked.png --mode dwt --id-length 8

# Keyed signal verification (not a general collusion guarantee)
capglyph embed photo.png --mode dwt --recipient-id "bob" --key "mysecret" --output photo_keyed.png
capglyph verify photo_keyed.png --mode dwt --key "mysecret"

# Learned mode (robustness depends on the evaluated transformation profile)
capglyph fetch-models                          # downloads TrustMark ONNX (~65MB)
capglyph embed photo.png --mode learned --recipient-id "carol"
capglyph extract leaked.png --mode learned
```

## Historical attack observations

| Attack                    |      alpha      | dct |      dwt       | learned |
| ------------------------- | :-------------: | :-: | :------------: | :-----: |
| JPEG q50                  |        ✗        |  ✓  |       ✓        |    ✓    |
| JPEG q30                  |        ✗        |  ✗  |       ✗        |  **✓**  |
| blur σ2.0                 |        ✗        |  ✗  |  verify✓/ID✗   |  **✓**  |
| scale 0.5×                |        ✗        |  ✗  |       ✗        |  **✓**  |
| collusion (5-copy median) |        —        |  —  | secret layer ✓ |    —    |
| known-cover diff          | ✗ (unavoidable) |  ✗  |       ✗        |    ✗    |
| img2img regeneration      |        ✗        |  ✗  |       ✗        |    ✗    |

This qualitative legacy summary mixes presence, ID recovery and keyed-signal
outcomes; it has no common trial denominator or calibrated false-positive
operating point. It is not a release guarantee or a comparison of full
authenticated Credential transport. Consult the frozen protocol/result records
for a specific claim. A recorded failure or success does not establish a
universal threshold across images, codecs, models or regeneration settings.

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
```

The manifest's `com.capglyph.watermark` assertion records the watermark mode,
recipient ID, and keyed flag — so
the pixel layer and the manifest cross-reference each other. The
`c2pa.created` action's digital source type defaults to `digitalCapture`;
override with `--source-type` (`capture | algorithmic | composite | trained` or
a full IPTC URI).

**Trust model:** the generated certificate is self-signed. Signature validation
does not establish a known signer or the truth of its claims. Trust requires an
independently authenticated certificate/public-key fingerprint or a configured
trust chain; a matching CN and validity window alone are insufficient. The HMAC
secret is not written into the manifest (only a `keyed: true` flag).

## Placement Strategies (Evaluation)

For empirical evaluation and baseline comparisons, CapGlyph supports three block-placement strategies (configurable via `--placement`):

- `skeleton` (default): Places the watermark along the image's geometric topology paths (edges and ridges).
- `edge`: A competitive baseline that targets standard high-variance edge blocks.
- `prng`: An internal control that distributes the watermark uniform-randomly across the image.

## Security model

- **Public layer** — presence detection (`verify`)
- **ID layer** — per-recipient tracing (`extract`, geometry-free)
- **Secret layer** — key-derived signal detection (`--key`), not a substitute
  for authenticated payload validation or a general forgery/collusion guarantee

Possession of the original lets an attacker replace a marked image with the
unmarked original. Regeneration can destroy the watermark, but a denoising
value such as `0.3` is model- and experiment-specific, not a universal cutoff.
An extracted recipient ID is a label, not independent proof of the holder's
identity or legal attribution.

## Compatibility

CapGlyph replaces the retired Sigil name. Existing integrations retain these
compatibility paths:

- Linux packages and Homebrew retain the `sigil` command alias.
- `SIGIL_MODEL_DIR` and `SIGIL_RECIPIENT_ID` remain fallbacks for
  `CAPGLYPH_MODEL_DIR` and `CAPGLYPH_RECIPIENT_ID`, respectively. Without an
  explicit model directory or environment override, learned mode uses the
  legacy XDG `sigil/models` directory if it exists and `capglyph/models` does not.
- C2PA signing also writes `com.sigil.watermark` for older readers. Verification
  prefers `com.capglyph.watermark` and falls back to the legacy assertion.
  Existing certificate/key files remain usable via explicit `--cert`/`--pkey`.
- The CLI library retains its `sigil_core` re-export for downstream Rust callers.

## Documentation

- `docs/mvp-spec.md` — historical MVP design; current normative wire rules live in [`capglyph-spec`](../capglyph-spec/README.md)
- `docs/product-roadmap.md` — product/B2B direction
- `CHANGELOG.md` — release history

## License

Apache-2.0. Learned mode embeds Adobe TrustMark models (MIT, downloaded
separately from Adobe's CDN — not distributed with CapGlyph).
