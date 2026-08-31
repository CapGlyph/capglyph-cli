# CapGlyph Organization Repositories

**Date:** 2026-08-31
**Task:** CTX-0036
**Org:** https://github.com/CapGlyph

Canonical repository inventory for the CapGlyph migration (see DEC-0004).
Managed via `HTTPS_PROXY=$NETWORK_PROXY gh` per project conventions.

## Inventory (verified 2026-08-31T11:59:12Z via `gh repo list CapGlyph` + `gh api repos/CapGlyph/<name>`)

| Repository              | Visibility | Description                                                                                                                        | URL                                               | Created              |
| ----------------------- | ---------- | ---------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------- | -------------------- |
| `capglyph-cli`          | public     | Invisible structural watermark for images — proof of origin, leak tracing, tamper detection                                        | https://github.com/CapGlyph/capglyph-cli          | 2026-08-31T11:55:56Z |
| `capglyph-docs`         | private    | CapGlyph docs (private)                                                                                                            | https://github.com/CapGlyph/capglyph-docs         | 2026-08-31T10:57:31Z |
| `capglyph-core`         | public     | CapGlyph core — canonical Rust implementation (watermark codec, framing, ECC, keying, registration) — source of truth for all SDKs | https://github.com/CapGlyph/capglyph-core         | 2026-08-31T11:59:02Z |
| `capglyph-spec`         | public     | CapGlyph specification — protocol, credential model, and carrier conformance spec                                                  | https://github.com/CapGlyph/capglyph-spec         | 2026-08-31T11:59:06Z |
| `capglyph-test-vectors` | public     | CapGlyph test vectors — cross-language conformance fixtures for core/spec verification                                             | https://github.com/CapGlyph/capglyph-test-vectors | 2026-08-31T11:59:11Z |

## Creation (CTX-0036)

Missing repos (`capglyph-core`, `capglyph-spec`, `capglyph-test-vectors`) were created on 2026-08-31:

```bash
HTTPS_PROXY=$NETWORK_PROXY gh repo create CapGlyph/capglyph-core --public \
  --description "CapGlyph core — canonical Rust implementation (watermark codec, framing, ECC, keying, registration) — source of truth for all SDKs"

HTTPS_PROXY=$NETWORK_PROXY gh repo create CapGlyph/capglyph-spec --public \
  --description "CapGlyph specification — protocol, credential model, and carrier conformance spec"

HTTPS_PROXY=$NETWORK_PROXY gh repo create CapGlyph/capglyph-test-vectors --public \
  --description "CapGlyph test vectors — cross-language conformance fixtures for core/spec verification"
```

Idempotent: `gh api repos/CapGlyph/<name>` returns 404 before creation, 200 after; `gh repo list` re-lists.

## Verification

```bash
HTTPS_PROXY=$NETWORK_PROXY gh repo list CapGlyph --limit 30
HTTPS_PROXY=$NETWORK_PROXY gh repo view CapGlyph/capglyph-core --json name,description,url,visibility,createdAt
HTTPS_PROXY=$NETWORK_PROXY gh repo view CapGlyph/capglyph-spec --json name,description,url,visibility,createdAt
HTTPS_PROXY=$NETWORK_PROXY gh repo view CapGlyph/capglyph-test-vectors --json name,description,url,visibility,createdAt
```

All three return `visibility: PUBLIC` and match descriptions above.

## Next steps

- CTX-0040: populate `capglyph-core` with extracted `crates/sigil-core` (canonical Rust Core)
- CTX-0041: populate `capglyph-spec` + `capglyph-test-vectors` with spec and conformance suite
- CTX-0037/CTX-0038/CTX-0039: filesystem isolation + code/docs rename `sigil` → `capglyph`
