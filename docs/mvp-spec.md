# Sigil — Specification v1.0

**Version:** 0.1.0 (code) / 1.0 (spec)
**Date:** 2026-08-15
**Status:** Implemented & verified (28 tests; attack matrices Q1.3–Q1.14, Q4.1–Q4.6)

---

## 1. What Sigil Is

Sigil embeds invisible, machine-detectable watermarks into images using four
independent embedding technologies ("modes"):

| Mode      | Technology                             | Payload                        | Strengths                                    | Weaknesses                                 |
| --------- | -------------------------------------- | ------------------------------ | -------------------------------------------- | ------------------------------------------ |
| `alpha`   | sparse pixels in alpha channel         | none (presence only)           | zero visual impact; tamper signal on PNG→JPG | killed by alpha strip/screenshot           |
| `dct`     | mid-frequency 8×8 DCT coefficients     | recipient ID (spread spectrum) | survives JPEG q50+                           | blur σ2, scale 0.5×                        |
| `dwt`     | Haar LH-band coefficients              | recipient ID (spread spectrum) | survives blur σ2, scale 70–150%              | scale 0.5×, img2img                        |
| `learned` | TrustMark CNN (ONNX, optional feature) | 61-bit ID (BCH_5 ECC)          | **survives JPEG q30, blur σ2, scale 0.5×**   | needs model download (~65MB), ONNX runtime |

All modes share a **three-layer security architecture**:

1. **Public layer** — watermark presence detection (`verify`)
2. **ID layer** — recipient ID extraction for leak tracing (`extract`)
3. **Secret layer** — HMAC(key, image)-derived attribution signal (`--key`),
   verifiable only with the key; survives collusion attacks (Q1.12)

Known limit (all modes, all pixel watermarks): generative regeneration
(img2img / SD-edit) defeats the watermark at denoising strength ≥0.3 —
Q1.13/Q1.14 verified this for both classical and learned embedding.

---

## 2. Commands

```
sigil embed   <input> [--mode alpha|dct|dwt|learned] [--recipient-id ID]
              [--key SECRET] [--output OUT] [--save-geometry GEO.json]
              [--stroke S] [--detail D] [--strength 0.95]
sigil verify  <input> [--mode M] [--recipient-id ID] [--key SECRET]
              [--mean-threshold T] [--verbose]
sigil extract <input> [--mode dct|dwt|learned] [--id-length L]
sigil strip   <input> [--output OUT]
sigil info    <input> [--mode alpha|dct]
sigil batch   embed|strip <glob> --output-dir DIR [--mode M] [--format png|jpg]
sigil fetch-models [--model-dir DIR]   # learned feature only
```

Exit codes: `verify` returns 0 (PRESENT) / 1 (ABSENT).

## 3. Embedding Details

### 3.1 alpha

- Geometry extracted via vectomancy-raster (Sobel → Otsu → Zhang-Suen →
  endpoint tracing → RDP/Chaikin)
- Sparse Bresenham pixels written to alpha; non-path alpha preserved

### 3.2 dct

- 8×8 blocks along skeleton; F[2,3] += 16
- ID bits at F[3,4] ±64, self-sync seed blocks (SEED_MAGIC) + redundancy 8
- Geometry-free extraction (seed recovered from self-sync blocks)

### 3.3 dwt

- Single-level 2D Haar; LH band positions along geometry
- Primary +8.0; ID ±256 (flat regions ±32); self-sync + ID positions
  PRNG-derived (SEED_MAGIC / stable_seed) — geometry-free extraction
- Secret layer: 256 differential pairs ±8 at HMAC(key, seed) positions

### 3.4 learned (feature `learned`)

- Adobe TrustMark (MIT) variant Q, BCH_5 (61 data bits ≈ 7 ASCII bytes)
- Payload = recipient ID packed into bits
- Models: encoder_Q.onnx (17MB) + decoder_Q.onnx (47MB) from Adobe CDN,
  cached in XDG data dir (`$SIGIL_MODEL_DIR` overrides)
- verify: bit accuracy ≥ 90% ⇒ PRESENT

## 4. Measured Attack Matrix (fixture suite, 1254×1254 + extreme set)

| Attack                      |       alpha        | dct |         dwt          | learned |
| --------------------------- | :----------------: | :-: | :------------------: | :-----: |
| JPEG q75                    |  ✗(tamper signal)  |  ✓  |          ✓           |    ✓    |
| JPEG q50                    |         ✗          |  ✓  |          ✓           |    ✓    |
| JPEG q30                    |         ✗          |  ✗  |          ✗           |  **✓**  |
| blur σ1.0                   |         —          |  ✓  |          ✓           |    ✓    |
| blur σ2.0                   |         —          |  ✗  |     verify✓/ID✗      |  **✓**  |
| scale 0.7×                  |         —          |  ✓  |          ✓           |    ✓    |
| scale 0.5×                  |         —          |  ✗  |          ✗           |  **✓**  |
| collusion median (5 copies) |         —          |  —  | verify✓/ID✗, secret✓ |    —    |
| known-cover diff            | ✗ (info-theoretic) |  ✗  |          ✗           |    ✗    |
| img2img ≥0.3                |         ✗          |  ✗  |          ✗           |    ✗    |

## 5. Security Model

- **Detection** (public layer): survives ordinary distribution
- **Tracing** (ID layer): per-recipient watermark; extraction is
  geometry-free; survives JPEG/blur/scale within the matrix above
- **Attribution** (secret layer): HMAC keyed, survives collusion; defeats
  forgery (can't add a valid secret layer without the key)
- **Hard limits**: known-cover diff removes everything (attacker has the
  original); generative regeneration defeats all pixel watermarks
