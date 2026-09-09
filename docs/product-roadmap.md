# CapGlyph Product Roadmap

**Status:** Product direction and historical release milestones, reconciled 2026-09-05. Not a release qualification or commitment to build every listed service. Current checkout manifests are CLI/server 0.1.7 and core 0.1.0; publication status must be checked separately.

## Product boundary

CapGlyph provides image-watermark/payload primitives, a local CLI, optional TrustMark and C2PA integrations, and a token-oriented reference credential/message server. Presence detection, recovered IDs, authenticated payloads, factual provenance and legal ownership are different endpoints.

The core now owns DCT/DWT implementations and three placement arms. JS/Python/Go provide local framing but not complete image SDKs. Their generic HTTP clients currently do not match the reference server. Read the [spec implementation reconciliation](../../capglyph-spec/docs/implementation-reconciliation.md) and [server README](../crates/capglyph-server/README.md) before making product claims.

## Recorded delivery, not blanket certification

- Four carrier modes: alpha, DCT, DWT and feature-gated TrustMark.
- Separate presence, recipient-ID and keyed-signal operations.
- CLI/core migration, local build/CI configuration, license and release-history records.
- Optional C2PA signing/verification; generated self-signed certificates require independent trust establishment.
- Historical Q-series observations and narrower frozen research studies.

The earlier v0.1.0 release checklist is historical; [CHANGELOG.md](../CHANGELOG.md) preserves release history. PSNR ranges and a VLM failing to notice a watermark do not establish human invisibility, resistance to steganalysis, or invisibility to all model versions/settings. Learned robustness and collusion observations are conditional on their exact experiment. No algorithm here generally blocks forgery or proves authorship merely by recovering a signal. Self-hosting still has compute, storage and operational costs.

## Open implementation gates

CarryCtx is authoritative; the following is a snapshot, not a new task assignment:

- `CGCLI-0002` — G11 image issue/verify/consume path: **ready**.
- `CGCLI-0004` — product-gate revalidation: **planned**.
- JS/Python/Go local image-carrier tasks (`CGJS/CGPY/CGGO-0002`): **ready**.
- Explicit SDK/current-server API reconciliation is still needed despite historical completion of framing/API-labelled tasks.

The end-to-end image service, persisted cover vault and production key/ingress deployment are not supplied by token-route success. Provenance retrieval/graph research is a separate track and remains paused for new experiments at the user's request.

## Candidate adoption work

These are proposals, not implemented offerings or an authorization to publish:

- Interactive local demonstration with endpoint-specific success/failure reporting.
- Public benchmark presentation linked to reproducible evidence and known weaknesses.
- Community feedback and integration examples after the supported surface is explicit.
- Hosting, support or custom integration only when demand and operational requirements are established.

Earlier competitor prices and revenue ranges were unverified planning anchors, not current market data or an approved pricing schedule. No exclusivity, competitive-superiority or patentability conclusion is implied.

## Non-goals and deferred directions

Copy-prevention, guaranteed known-cover resistance, anti-training cloaking, generic screenshot/print-scan robustness, and universal regeneration resistance are not product promises. Video/audio/document support and cross-platform capture studies require separate user-authorized scope and evidence.
