# CapGlyph — Technical Roadmap

**Last updated:** 2026-08-31 (renamed from Sigil; version 0.2.0 → 0.1.0 fresh start)

## Status: Core complete (v0.1.0, formerly Sigil v0.2.0)

All planned embedding stages are implemented and verified against the
measured attack matrix:

| Stage                             | Mode              | Status                        |
| --------------------------------- | ----------------- | ----------------------------- |
| Fragile alpha watermark           | `alpha`           | ✅ shipped                    |
| RGB DCT residual watermark        | `dct`             | ✅ shipped                    |
| Haar DWT LH-band watermark        | `dwt`             | ✅ shipped                    |
| Learned CNN watermark (TrustMark) | `learned`         | ✅ shipped (optional feature) |
| HMAC keyed secret layer           | `--key`           | ✅ shipped                    |
| Keyed recipient-ID payload        | `--key` (learned) | ✅ shipped                    |
| Geometry-free ID extraction       | `extract`         | ✅ shipped                    |

## Completed Research Questions

The Q-series attack studies (`Q1.3`–`Q1.14`, `Q4.1`–`Q4.6`) cover:

- Attack thresholds (JPEG quality, blur σ, scale factors)
- Combined attack chains, collusion, known-cover diff
- Generative regeneration (img2img) — the hard limit for all pixel watermarks
- VLM perceptibility and human visibility thresholds
- OCR false-positive analysis
- Competitive analysis (Steg.AI, Imatag, Digimarc, TrustMark)

## Future Directions

### Short term (no research needed)

- [x] C2PA manifest integration (pixel watermark + content credentials)
- [ ] Web monitor/crawler service (embed + periodic leak scanning)
- [ ] `learned` mode model variant selection (Q/C/P)

### Research scale

- [ ] Screen-capture robustness — requires learned embedding trained with
      re-capture augmentation (Steg.AI territory, not incremental work)
- [ ] Video per-frame extension of the DWT layer
- [ ] Adversarial "visual lock" (Stage 3 from the original plan) — defeated
      by ensemble purification; deprioritized after Q1.13 findings

### Explicit non-goals

- Anti-AI-training cloaking (Glaze/Nightshade territory)
- Breaking the known-cover diff limit (information-theoretic)
- Beating generative regeneration (defeats all pixel watermarks)
