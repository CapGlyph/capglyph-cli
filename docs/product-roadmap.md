# CapGlyph Product Roadmap

**Last updated:** 2026-08-31 (renamed from Sigil, formerly Sigil)
**Status:** Open-source CLI shipped (v0.2.0) → open-core monetization

---

## Product Vision

CapGlyph (formerly Sigil) provides invisible, robust image watermarking for leak source
identification, copyright attribution, and tamper detection. Unlike
metadata-based solutions (EXIF/IPTC) that are trivially stripped, CapGlyph
embeds machine-verifiable signals directly into image pixels/frequency data,
with an open, auditable scheme.

**Core value proposition:**

- Invisible to humans (PSNR 42–52 dB measured)
- Invisible to VLMs (validated on Gemini/Claude/GPT at all effort levels)
- Survives aggressive ordinary edits (learned mode: JPEG q30, blur σ2, scale 0.5×)
- Per-recipient tracing with geometry-free extraction
- HMAC-keyed attribution surviving collusion, blocking forgery
- Self-hosted, open source (Apache-2.0), zero per-image cost

## Phase 1: Open-Source Release (complete)

- [x] Four watermark modes (alpha/dct/dwt/learned)
- [x] Three-layer security model (public/ID/secret)
- [x] Attack matrix verified and published (Q-series findings)
- [x] CI/CD, license, docs, README (EN/zh-CN), changelog
- [x] GitHub repository `CapGlyph/capglyph-cli` (formerly under Xuepoo organization, now redirects)

## Phase 2: Adoption & Trust Building (next)

- [ ] GitHub Release v0.2.0 binaries (5 platforms, `capglyph` binary with `sigil` alias)
- [ ] Interactive web demo (upload → attack → extract)
- [ ] Public attack-matrix page (open weakness disclosure vs Steg.AI)
- [ ] Hacker News / security-community launch
- [x] C2PA manifest integration (pixel watermark + content credentials)

## Phase 3: Monetization (demand-driven, after users exist)

| Path               | Offering                                                            | Anchor pricing               |
| ------------------ | ------------------------------------------------------------------- | ---------------------------- |
| Cloud service      | Hosted monitoring: embed at scale + periodic leak crawling + alerts | €99–299/mo (Imatag: €299/mo) |
| Enterprise support | SLA, priority response, tuning consultation                         | $5–50k/yr                    |
| Custom development | Video extension, format support, integration                        | project-based                |

Competitive positioning: "the GnuPG of digital-image forensics" — open,
auditable, self-hosted. Steg.AI cannot respond on transparency (learned
model parameters are its moat) or on price (per-image SaaS cost).

## Explicit Non-Goals

- Anti-AI-training cloaking (Glaze/Nightshade territory)
- Screen-capture robustness (requires learned re-capture training — Steg.AI
  territory; revisit only as research project)
- Video/audio/document modes (not before market demand)
