# C2PA Integration Design (2026-08-16, rev 3)

## Goals

Add C2PA (Content Credentials) sign + verify capability to Sigil, as a
complementary provenance layer to the pixel watermark. Dual-layer forensic
story: the invisible watermark proves _attribution_ (who leaked), the C2PA
manifest proves _provenance_ (who created / who last signed).

## Non-Goals

- No CA-issued certificate workflow (self-signed only; trust model documented)
- No remote manifest fetching / cloud verification services
- No thumbnail assertion, no ingredient reconstruction (c2pa `add_thumbnails`
  not enabled)
- No PDF/AVIF/TIFF support — JPEG and PNG only, matching Sigil's image stack
- No Content Credentials validation of third-party trust lists (this is not a
  validator product)
- Signing a file that already contains a C2PA manifest: out of scope — the
  c2pa crate's default behavior for signing already-manifested assets is
  left as-is (no explicit ingredient/chaining control in v1).
- `strip`/`extract`/`batch`/`info` do not interact with C2PA. `strip` removes
  the pixel watermark but leaves any C2PA manifest untouched (documented).

## CLI Surface

```
sigil c2pa init [--org <name>] [--out <dir>] [--force]
sigil c2pa sign <in> --cert <c.pem> --pkey <private.key> [-o <out>]
                  [--manifest-json <meta.json>]
                  [--recipient-id <id> --mode <dct|dwt|learned>] [--key <sigil-secret>]
sigil c2pa verify <in>                            # read + verify, JSON report

sigil embed <in> -m dct --recipient-id alice --c2pa --cert <c.pem> --pkey <private.key>
sigil verify <out> --c2pa                         # watermark + C2PA dual-layer report
```

Flag naming (collision resolution):

- `--pkey` = C2PA signing private key (PEM file). Never named `--key`.
- `--key` keeps its existing meaning: Sigil HMAC secret (used only for keyed
  payload; C2PA signing itself never uses it).

Defaults and guards:

- `c2pa init` default `--out ./sigil-certs/`; refuses to overwrite existing
  cert/key without `--force`.
- `c2pa sign` requires `-o` (distinct output); signing in-place is rejected
  because `Builder::sign_file` needs distinct source/dest and in-place
  signing would destroy the source on failure. Error message says so.
- `c2pa sign` on unsupported inputs (paletted/interlaced PNG, progressive/CMYK
  JPEG) fails with a clear CLI error naming the unsupported format variant.
- `embed --c2pa` requires `--recipient-id` and `--cert`/`--pkey`.
- `c2pa sign --recipient-id <id> --mode <m> [--key <secret>]` (optional
  trio; clap `requires` coupling: `--recipient-id` requires `--mode` and
  vice versa). When given, signs the _same_ payload metadata the watermark
  carries (see Dual-Layer). Without them, `c2pa sign` is a pure provenance
  signature with no watermark claim.
- `c2pa verify` exit codes: `0` = manifest present and signature valid;
  `1` = manifest present but signature invalid; `2` = no manifest found
  (unsigned); `3` = file/parse error. `verify --c2pa` exit code remains
  watermark-based (documented in README).
- All three subcommands live behind `#[cfg(feature = "c2pa")]`; when the
  feature is absent the `c2pa` command group prints
  "recompile with --features c2pa" (same pattern as `learned`).

## Cargo Feature & Dependencies

```toml
[dependencies]
c2pa = { version = "0.90", default-features = false, features = ["rust_native_crypto", "file_io"], optional = true }
rcgen = { version = "0.13", default-features = false, features = ["ring", "pem"], optional = true }
x509-parser = { version = "0.16", optional = true }

[features]
default = []
learned = ["dep:trustmark"]
c2pa = ["dep:c2pa", "dep:rcgen", "dep:x509-parser"]
```

Rationale:

- `rust_native_crypto` (pure-Rust ECDSA/P256/RSA) instead of default `openssl`:
  no system OpenSSL dependency → the 5-platform CI matrix (esp. Windows)
  keeps building without vcpkg/choco.
- `default_http` disabled: no remote fetch features, no tokio/reqwest tree.
- `rcgen` for built-in ES256 self-signed cert generation; backend pinned to
  `ring` (no aws-lc-rs), `pem` for key serialization.
- `x509-parser` (pure Rust) to extract signer CN + validity window from the
  cert chain in the verify report.
- Signer construction uses `c2pa::create_signer::from_files` — verified
  against c2pa 0.90.15 source: this module is available under
  `rust_native_crypto` (not openssl-only, as earlier believed) and its native
  `EcdsaSigner` handles ES256 with a PKCS-8 PEM P256 key internally. No
  manual `Signer` trait implementation and no direct `p256` dependency.
- MSRV note: c2pa 0.90 requires rustc ≥ 1.88. CI uses `stable`, fine.

## Module Layout

New file `src/c2pa.rs` (flat module, matching `learned.rs` style).

### Cert generation

```rust
pub fn init_cert(org: Option<&str>, out_dir: &Path, force: bool) -> anyhow::Result<(PathBuf, PathBuf)>
```

rcgen `generate_simple_self_signed`, ECDSA P256 via `CertificateParams` API
(note: rcgen 0.13 has no algorithm argument on `generate_simple_self_signed`;
the key algorithm is set through `params.alg = &rcgen::PKCS_ECDSA_P256_SHA256`),
CN = org.unwrap_or("Sigil User"), validity 2 years, writes `cert.pem` +
`private.key` (0600 perms on key). Returns both paths. With `--force`,
overwrites.

### Signer construction

```rust
let signer = c2pa::create_signer::from_files(&cert_path, &pkey_path,
                                             c2pa::SigningAlg::Es256, None)?;
// BoxedSigner: derefs to &dyn c2pa::Signer, native EcdsaSigner under the hood
```

Verified against c2pa 0.90.15 source: `crate::create_signer` is exported
regardless of crypto backend; under `rust_native_crypto` it dispatches to the
native `EcdsaSigner` (loads PKCS-8 PEM P256 key via
`p256::ecdsa::SigningKey::from_pkcs8_pem`, hashes internally, emits DER
signatures which c2pa's COSE layer auto-converts to P1363).

### Sign

```rust
pub struct WatermarkClaim { pub mode: String, pub recipient_id: Option<String>, pub keyed: bool }

pub fn sign_image(input: &Path, output: &Path, cert: &Path, key: &Path,
                  claim: &WatermarkClaim, manifest_json: Option<&Path>) -> anyhow::Result<()>
```

Uses the c2pa 0.90 redesigned API (not the pre-redesign `Manifest::new`/
`ManifestStore` API). Verified against 0.90.15 source:

```rust
let mut builder = c2pa::Builder::from_context(c2pa::Context::new())
    .with_definition(r#"{"claim_generator": "sigil/<ver>"}"#)?;
// NOTE: Builder::from_json is deprecated in 0.90; use from_context + with_definition
// set_claim_generator_info is authoritative for the claim generator field and
// overrides any value in the definition JSON.
builder.set_claim_generator_info(c2pa::ClaimGeneratorInfo::new("sigil", &[env!("CARGO_PKG_VERSION")]));
builder.add_assertion("com.sigil.watermark", claim)?;            // T: Serialize
// optional: --manifest-json extra assertions merged as additional labels
builder.sign_file(signer.as_ref(), input, output)?;              // signer: &dyn Signer
```

`--manifest-json` payload: a JSON object `{"label": value, ...}`; each entry
becomes an assertion `label -> serde_json::to_vec(value)`.

### Verify

```rust
#[derive(Serialize)]
pub struct C2paReport {
    pub present: bool,
    pub claim_generator: Option<String>,
    pub signature_status: String,          // "valid" | "invalid" | "unsigned"
    pub signer_org: Option<String>,        // cert subject CN
    pub valid_from: Option<String>,        // RFC 3339
    pub valid_to: Option<String>,          // RFC 3339
    pub watermark_claim: Option<WatermarkClaim>,
}

pub fn verify_image(input: &Path) -> anyhow::Result<C2paReport>
```

`c2pa::Reader::from_file(path)` + `reader.active_manifest()` +
`reader.validation_status()`; signer CN/validity extracted from
`manifest.cert_chain()` via x509-parser. `watermark_claim` deserialized back
from the assertion when present.

Status mapping: any `ValidationStatus` entry whose code indicates a failure
(not a warning) maps `signature_status` to `"invalid"`; all warnings/empty
maps to `"valid"`; no active manifest maps to `"unsigned"`.

CLI wiring in `cli.rs`: `Commands::C2pa(C2paArgs)` with sub-subcommand enum
`C2paCommand::{Init, Sign, Verify}`. Embed/verify args gain `--c2pa`,
`--cert`, `--pkey`. `embed.rs` / `verify.rs` gain a small post-step that calls
into `c2pa.rs` when the flag is set (cfg-gated).

## Dual-Layer Linkage

- `embed --c2pa`: run the pixel watermark embed first, then sign the _output_
  image with a manifest whose `com.sigil.watermark` assertion mirrors the
  embed parameters (mode, recipient-id, keyed flag). Never sign the input.
- `verify --c2pa`: run watermark verification as usual, then call
  `verify_image`; the final JSON report contains both the watermark section
  and the `c2pa` section. Exit code remains watermark-based; C2PA status is
  informational. (JSON schema change documented in README changelog.)
- Keyed payloads: the HMAC secret never enters the manifest; the manifest
  only records `"keyed": true`.

## Trust Model

Self-signed certs mean the C2PA layer proves "signed by the holder of this
key", not "signed by a known entity". The report includes signer CN + validity
window so users can pin a known cert. This mirrors c2patool's personal-cert
workflow and is documented in README.

## CI Changes

1. `ci.yml` ubuntu job: `--features learned` → `--features learned,c2pa`
   (build, clippy, test).
2. `ci.yml` `test-learned-systems` job (macos-14, windows-latest): same
   feature upgrade — the Windows build story is the entire justification for
   `rust_native_crypto`, so c2pa must compile+test on that matrix.
3. `release.yml`: 5-platform build command gains `,c2pa`; nfpm config and AUR
   PKGBUILD (`sigil-wm`) `cargo build --features learned,c2pa`.
4. AUR `_vmver` unchanged; no new system deps thanks to rust_native_crypto.

## Testing

- Unit: WatermarkClaim serde round-trip; rcgen cert/key PEM pair loads into
  `c2pa::create_signer::from_files`; signed output validates.
- Integration (tests/c2pa_tests.rs, gated on feature):
  - init → sign → verify: signature valid, claim round-trips, signer CN +
    validity window populated
  - verify on unsigned image: present=false, signature_status="unsigned"
  - embed --c2pa → verify --c2pa: watermark + manifest claim consistent
  - sign with a different (regenerated) key than the cert → validation fails
  - JPEG + PNG both round-trip (JUMBF APP11 / iTXt paths)
  - embed -m learned --c2pa on PNG (TrustMark output is PNG)
  - init refuses overwrite without --force; sign without -o errors
- No network access in tests (c2pa http features disabled by construction).

## Release Notes / Docs

- README: "Content Credentials" section with 3-command quickstart, the
  self-signed trust caveat, and the verify JSON schema addition.
- roadmap.md / product-roadmap.md: check the C2PA box.
