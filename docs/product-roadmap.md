# Sigil Product Roadmap

**Last updated:** 2026-08-15  
**Status:** Pre-MVP → Closed-source B2B API Service

---

## Product Vision

Sigil provides invisible, robust image watermarking for copyright tracking, AI training data attribution, and leak source identification. Unlike metadata-based solutions (EXIF/IPTC) that are trivially stripped, Sigil embeds cryptographic fingerprints directly into image pixel/frequency data.

**Core Value Proposition:**

- **Invisible to humans** (PSNR 42–50 dB)
- **Invisible to VLMs** (recognition accuracy unchanged, validated on Gemini/Claude/GPT)
- **Survives JPEG compression** (DCT mode: quality ≥50)
- **Recipient tracking** (unique seed per distribution target)

---

## Phase 1: Technical Foundation (Current → Week 2)

**Objective:** Complete P0/P1 technical debt, validate core algorithms.

### P0 Tasks (Release Blockers)

- [x] DCT watermark implementation (JPEG survival)
- [x] PRNG fallback for solid-color images (Q2.1)
- [ ] `verify --mode dct` without geometry file (re-extract skeleton from watermarked image)
- [ ] Path count warning (< 50 paths → low signal quality)
- [ ] Q1.3 attack resistance benchmark (alpha strip script vs DCT tamper threshold)

### P1 Tasks (Feature Completeness)

- [ ] `--recipient-id <string>` tracking mode (derive seed from recipient ID hash)
- [ ] Q1.5 OCR false-positive test (Tesseract, PaddleOCR)
- [ ] Batch mode (`sigil embed ./images/*.png --output-dir ./protected/`)
- [ ] JPEG direct output (`--output-format jpeg --jpeg-quality 85`)
- [ ] Signal quality report (`--psnr`, `--coverage`, `--confidence`)

### P2 Tasks (UX Polish)

- [ ] Progress bar for large images (> 2048×2048)
- [ ] `sigil info <image>` — display watermark metadata without verification
- [ ] Cross-platform builds (Linux x64/arm64, macOS Intel/Apple Silicon, Windows x64)

---

## Phase 2: B2B API Service (Week 3–8)

**Objective:** Launch closed-source REST API with multi-tenant seed management.

### Product: `api.sigil.io`

**Core Endpoints:**

```http
POST /v1/watermark/embed
  Content-Type: multipart/form-data
  - image: file (PNG/JPEG, max 10MB)
  - mode: "alpha" | "dct" (default: dct)
  - recipient_id: string (optional)
  → Response: watermarked image + tracking_token (UUID)

POST /v1/watermark/verify
  Content-Type: multipart/form-data
  - image: file
  - tracking_token: string
  → Response: {
      detected: bool,
      confidence: float,
      recipient_id: string | null,
      metadata: {blocks_embedded: int, psnr: float}
    }

POST /v1/watermark/batch
  Content-Type: multipart/form-data
  - images[]: file[] (max 100 images)
  - mode, recipient_id (shared params)
  → Response: [{tracking_token, url}]
```

**Backend Stack:**

- **Runtime:** Cloudflare Workers (Rust → WASM, vectomancy already WASM-compatible)
- **Storage:** Cloudflare R2 (watermarked images) + D1 (tracking_token → metadata)
- **Auth:** API key (Bearer token) via Cloudflare API Shield

**Pricing Tiers:**

```
Free Tier:     100 images/month       (rate: 10 req/min)
Starter:    $99/mo   10K images/month  (rate: 60 req/min)
Pro:       $499/mo  100K images/month  (rate: 300 req/min)
Enterprise: $2999/mo unlimited          (rate: custom) + SLA + priority support
```

**Stripe Integration:**

- Monthly subscription + usage-based overage ($0.01/image beyond quota)
- Self-service dashboard at `dashboard.sigil.io` (Next.js + Cloudflare Pages)

---

## Phase 3: Web UI (Week 9–12)

**Objective:** Self-service playground for non-technical users.

### Product: `app.sigil.io`

**Features:**

- Drag-and-drop image upload
- Real-time watermark preview (client-side WASM)
- Batch upload (ZIP support, process in Workers)
- Tracking dashboard (view all watermarked images + verification history)
- Export audit report (CSV: tracking_token, recipient_id, upload_time, verification_count)

**Tech Stack:**

- **Frontend:** SvelteKit (lighter than React for image processing UI)
- **WASM Engine:** `sigil-wasm` crate (wasm-bindgen, similar to `vectomancy-web/wasm-engine`)
- **Deployment:** Cloudflare Pages (global edge deployment)

**Freemium Model:**

```
Free:  5 images/day  (client-side WASM only, no tracking_token storage)
Pro:  Unlimited      (server-side API, persistent tracking, batch support)
```

---

## Phase 4: Enterprise Private Deployment (Month 4+)

**Objective:** On-premise Docker container for high-security customers.

### Product: Sigil Enterprise

**Delivery:**

- Docker Compose stack (API server + PostgreSQL + Redis + management UI)
- Kubernetes Helm chart (for large-scale deployments)
- Admin dashboard (user management, seed database, audit logs)
- SSO integration (LDAP, SAML 2.0, OAuth2)

**Target Customers:**

- Game studios (art asset leak prevention)
- Ad agencies (client draft version tracking)
- Government/Finance (document screenshot attribution, compliance requirements)

**Pricing:**

```
Basic:      $20K/year — single-node deployment, 5 admin seats
Enterprise: $50K/year — HA cluster, unlimited seats, dedicated support, SLA
```

**Technical Requirements:**

- Air-gap deployment support (no internet dependency)
- FIPS 140-2 compliance mode (crypto libraries)
- Audit log export (SIEM integration: Splunk, ELK)

---

## Phase 5: Platform Integrations (Month 6+)

**Objective:** Embed Sigil as a feature in existing content platforms.

### Target Partners:

1. **Content platforms** (Xiaohongshu, Zhihu, Weibo) — creator anti-theft tool
2. **Design tools** (Canva China, Gaoding) — export with watermark option
3. **Photo libraries** (Unsplash, Pexels) — track image reuse in AI training datasets
4. **Document tools** (Notion, Confluence) — screenshot leak prevention

### Business Model:

- **SDK License:** $10K–50K/year (white-label branding)
- **Revenue Share:** $0.01/active user/month (platforms with > 1M users)
- **Consulting:** Custom integration (priced per project)

---

## Technical Debt & Research (Ongoing)

### Open Research Questions

- **Q1.5:** OCR tool false-positive rate (Tesseract, PaddleOCR, EasyOCR)
- **Q3.1:** Text-path embedding (embed watermark along detected text strokes)
- **Q4.1:** Human perceptual threshold curve (user study, n=50)
- **Q5.1:** Multi-layer watermark (alpha + DCT dual-stage, alpha as tamper seal)

### Long-term R&D (Stage 3)

- **Adversarial cloaking** (Glaze/PhotoGuard direction, high compute cost)
- **Video watermark** (per-frame DCT + temporal coherence)
- **3D model watermark** (mesh geometry perturbation, for game assets)

---

## Success Metrics

**Phase 2 (API Launch):**

- 50 registered API keys (first month)
- 10 paying customers ($99+ tier)
- $5K MRR (Month 3)

**Phase 3 (Web UI):**

- 500 free-tier signups (first month)
- 50 Pro conversions (10% conversion rate)
- $20K MRR (Month 6)

**Phase 4 (Enterprise):**

- 2 enterprise contracts signed
- $100K ARR (Year 1)

---

## Competitive Landscape

**Direct Competitors:**

- **Digimarc** (NASDAQ: DMRC) — incumbent, black-box, B2B only, expensive
- **Imatag** (France) — SaaS watermark API, €199–999/mo, JPEG-focused
- **Steg.AI** (USA) — AI-native watermark, research-stage

**Indirect Competitors:**

- **C2PA** (Adobe/Microsoft) — metadata-based, easily stripped
- **Glaze/Nightshade** (UChicago) — adversarial cloaking, free but compute-heavy

**Sigil Differentiation:**

- Open algorithm (trust via transparency, closed implementation)
- VLM-validated invisibility (competitors lack published VLM benchmarks)
- Cloudflare edge deployment (lower latency than AWS-based competitors)
- Freemium API tier (competitors all B2B enterprise sales only)

---

## Notes

- All English docs per user request (2026-08-14)
- Prior art survey (Q6.1) deferred until post-MVP
- Patent landscape research (Q6.2) deferred
