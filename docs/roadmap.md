# Sigil — Technical Roadmap

**Last updated:** 2026-08-14

This document describes the three-stage evolution of Sigil from its current MVP to a
production-grade, JPEG-resistant structural watermark. Each stage is self-contained and
ships value independently.

---

## Stage Overview

```
Stage 1 (current)          Stage 2                    Stage 3
─────────────────          ───────────────────        ──────────────────────────────
Fragile Alpha              RGB DCT Residual            Geometry-Guided Adversarial
Watermark                  Watermark                   Visual Lock
─────────────────          ───────────────────        ──────────────────────────────
< 1ms / pure CPU           < 5ms / pure CPU            < 50ms / pure CPU (no GPU)
Invisible to humans        Invisible to humans         Invisible to humans
Detects tamper             Survives JPEG               Blinds VLM encoder
Broken by alpha=255        Broken by DiffPure          Broken by ensemble purification
                           or Gaussian blur            (future research problem)
```

---

## Stage 1: Fragile Alpha Watermark (shipped, v0.1)

### What it does

Encodes a structural fingerprint in the PNG RGBA alpha channel. The fingerprint is derived
from the image's own geometric skeleton (Vectomancy raster pipeline: Sobel → Otsu →
Zhang-Suen → Chaikin). Sparse Bresenham pixel marking at ~0.05% pixel coverage, α≈180.

### Threat model

- **Protected against:** passive observers, non-technical recipients, social platform
  upload detection (JPEG conversion destroys α channel → tamper detected)
- **NOT protected against:**
  - One-liner attack: `img.convert('RGB')` or `convert -alpha off` — zero resistance
  - Screenshot: OS compositor flattens α to RGB before capture
  - Any JPEG output pipeline

### Competitive moat at this stage

- Pure Rust, zero GPU, <1ms — runnable in browser (WASM), mobile, Cloudflare Edge Workers
- Self-structural fingerprint (image's own topology, not random noise)
- Controlled perturbation probe: unique scientific instrument for VLM sensitivity benchmarking

### Known failure modes (from research questions doc)

- Solid color images: Sobel ≈ 0 → no paths extracted → no watermark embeddable
- Extreme-density images (oil paintings): too many paths → α accumulation → visible
- RGBA source images: existing transparency complicates signal/noise separation

### Fallback for zero-path images (planned for v0.1.x)

When `paths.len() < N_MIN` after extraction, fall back to a chaos-PRNG scatter pattern
(seeded from image hash) to guarantee embeddability on solid-color and minimal-content images.

---

## Stage 2: RGB DCT-Domain Residual Watermark (next, v0.2)

### Core idea

Instead of writing signal to the alpha channel, modulate the high-frequency DCT
coefficients of the RGB channels along the geometric skeleton coordinates. The
perturbation is imperceptible but survives JPEG compression (which only quantizes
coefficients, it does not zero them out if the energy is above the quantization step).

### Algorithm sketch

```
For each 8×8 block B intersected by a skeleton path segment:
  1. Forward DCT → coefficient matrix C[u][v]
  2. Identify mid-frequency coefficients along zigzag order (positions 10–25)
     (low-freq = visible artifacts; very-high-freq = killed by JPEG)
  3. Add a small energy δ to selected C[u][v]:
       C'[u][v] = C[u][v] + δ · sign(C[u][v] + ε)   (quantization-index modulation)
  4. Inverse DCT → modified block B'
  5. Spatial RGB difference |B' - B| < 1/255 (invisible to human eye)

Verification:
  Re-extract skeleton from suspect image → locate same 8×8 blocks →
  Forward DCT → measure energy at positions 10–25 → compare to expected modulation pattern
```

### Properties

- Survives JPEG at quality ≥ 50 (quantization step < δ for mid-frequency coefficients)
- Survives PNG→JPG→PNG round-trip
- Broken by: DiffPure, Gaussian blur σ > 1.5, aggressive JPEG quality < 30
- Does NOT rely on alpha channel — works on RGB source images natively
- Pure integer arithmetic, no GPU, <5ms per megapixel

### Threat model upgrade

- `img.convert('RGB')`: no effect (signal is in RGB already)
- JPEG compression at quality ≥ 50: signal survives
- Screenshot (screen capture): analog gap still destroys signal (pixel-level capture at
  non-integer zoom + display gamma — sufficient to wash out sub-1/255 residuals)
- Still broken by: `PIL.ImageFilter.GaussianBlur(2)` before JPEG

### Implementation plan

1. Add `rustdct` crate (pure Rust DCT-II/III, no FFTW dependency)
2. New module `src/dct.rs`: block DCT, quantization-index modulation, inverse DCT
3. `embed.rs`: when `--mode dct` (or v0.2 default), call `dct::embed_blocks()` instead
   of Bresenham alpha marking
4. `verify.rs`: add DCT correlation verification path
5. Update `signal.rs`: new `DctSignalMetrics` struct (coefficient energy, block coverage)

---

## Stage 3: Geometry-Guided Adversarial Visual Lock (research, v0.3+)

### Core idea

Move from "stamp" (passive watermark the image after generation) to "lock" (actively
perturb the image so VLM encoders cannot parse its semantic content — while human
perception remains fully intact).

The key differentiator vs. Glaze/PhotoGuard: **use the geometric skeleton as the
perturbation manifold** rather than running unconstrained gradient descent over all pixels.
This:

- Keeps perturbation spatially concentrated on edges/contours (perceptually safe zones)
- Eliminates the compute bottleneck: no encoder backpropagation, just direct pixel modification
  along pre-computed paths → stays sub-millisecond
- Makes the perturbation structurally coherent with the image (not random high-freq noise)

### Research questions before implementation

These must be answered experimentally before writing code:

1. **Does skeleton-confined perturbation transfer to closed-source VLMs?**
   Perturbations computed on CLIP-ViT degrade ≥70% against GPT-4o (black-box).
   Need to test whether geometry-confined (vs. unconstrained) perturbations transfer better.
   Hypothesis: YES — edge pixels are semantically decisive in all ViT architectures
   (patch boundaries, attention maps concentrate on edges), so targeting edges is
   model-agnostic. Not yet validated.

2. **What minimum perturbation magnitude causes semantic failure?**
   Run VLM scoring sweep at δ ∈ {2, 4, 8, 12, 16}/255 along skeleton pixels only.
   Measure: VLM content description accuracy vs. PSNR. Find Pareto frontier.

3. **Does skeleton-confined perturbation survive JPEG + DiffPure?**
   Apply JPEG quality=75 + DiffPure 3-step after geometry-confined perturbation.
   If skeleton-path pixels are semantically critical AND survive compression, this breaks
   the purification barrier without needing high-frequency noise.

### Conceptual algorithm (subject to experimental validation)

```
Input: image I, skeleton S (set of path pixels from Stage 1/2)
Output: perturbed image I* such that VLM(I*) ≠ VLM(I) while human(I*) ≈ human(I)

For each skeleton pixel p ∈ S:
  Compute local edge normal n_p (perpendicular to path tangent)
  Apply perturbation along n_p direction:
    I*[p] = I[p] + δ · n_p · sign(I[p] - μ_neighborhood(p))
  Clamp to valid pixel range

Constraint: |I* - I|_∞ ≤ ε_max (default: 12/255)
```

This is a single forward pass (O(|S|)), no gradient computation, no GPU.

### Stage 3 deliverables (tentative)

- `--mode adversarial` flag in `sigil embed`
- Experimental results on VLM semantic failure rate vs. PSNR
- Comparison with Glaze/PhotoGuard at equivalent PSNR budget

---

## Cross-Cutting: Solid Color / Zero-Path Fallback

Applies to all stages. When `paths.len() < N_MIN` after skeleton extraction:

```
seed = sha256(image_pixels)[..8]           // deterministic per-image seed
rng  = ChaCha8Rng::from_seed(seed)
scatter_coords = rng.sample(N_SCATTER)     // N_SCATTER ≈ total_pixels * 0.05%
embed at scatter_coords with stage-appropriate signal
```

Properties:

- Deterministic: re-running embed on same image produces identical scatter
- Content-agnostic: works on pure white, solid red, gradient maps
- Non-structural: cannot be confused with real skeleton signal (good for verify disambiguation)

---

## Cross-Cutting: Per-Recipient Unique Watermark (v0.2+)

```
sigil embed photo.png --recipient-id <uuid>
```

Implementation: XOR the skeleton path subset selection (stride computation) and
scatter seed with `hash(recipient_id)`. Two recipients' images differ by ≤1 pixel in
10,000, indistinguishable to humans. `sigil verify --recipient-id <uuid>` matches only
the correct recipient's pattern.

---

## Capability Matrix by Stage

| Capability                          | Stage 1 (v0.1) | Stage 2 (v0.2) | Stage 3 (v0.3+) |
| ----------------------------------- | -------------- | -------------- | --------------- |
| Human invisible                     | ✓              | ✓              | ✓               |
| Zero GPU                            | ✓              | ✓              | ✓               |
| < 5ms per image                     | ✓              | ✓              | ✓ (estimated)   |
| WASM / Edge Workers compatible      | ✓              | ✓              | ✓               |
| Detects PNG→JPG tamper              | ✓              | ✓              | ✓               |
| Survives JPEG quality ≥ 50          | ✗              | ✓              | ✓               |
| Survives `alpha=255` one-liner      | ✗              | ✓              | ✓               |
| Solid color image support           | partial        | ✓ (fallback)   | ✓               |
| VLM semantic content blocking       | ✗              | ✗              | experimental    |
| Per-recipient tracing               | ✗              | ✓              | ✓               |
| Adversarial purification resistance | ✗              | ✗              | research TBD    |
