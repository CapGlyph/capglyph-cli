# C2PA Integration Design (2026-08-16)

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

## CLI Surface

```
sigil c2pa init [--org <name>] [--out <dir>]      # rcgen ES256 self-signed cert + key (PEM)
sigil c2pa sign <in> --cert <c.pem> --key <k.key> [-o <out>] [--manifest-json <meta.json>]
sigil c2pa verify <in>                            # read + verify, JSON report

sigil embed <in> -m dct --recipient-id alice --c2pa --cert <c.pem> --key <k.key>
sigil verify <out> --c2pa                         # watermark + C2PA dual-layer report
```

- All three subcommands live behind `#[cfg(feature = "c2pa")]`; when the
  feature is absent the `c2pa` command group prints
  "recompile with --features c2pa" (same pattern as `learned`).
- `sigil embed --c2pa` requires `--recipient-id` and `--cert`/`--key`.
- `sigil c2pa sign` also accepts `--recipient-id` and `--key <sigil-secret>`
  (optional): when given, it signs the _same_ payload metadata that the
  watermark carries, so the manifest and pixel layer cross-reference.

## Cargo Feature & Dependencies

```toml
[dependencies]
c2pa = { version = "0.90", default-features = false, features = ["rust_native_crypto", "file_io"], optional = true }
rcgen = { version = "0.13", optional = true }

[features]
default = []
learned = ["dep:trustmark"]
c2pa = ["dep:c2pa", "dep:rcgen"]
```

Rationale:

- `rust_native_crypto` (pure-Rust ECDSA/P256/RSA) instead of default `openssl`:
  no system OpenSSL dependency → the 5-platform CI matrix (esp. Windows)
  keeps building without vcpkg/choco.
- `default_http` disabled: no remote fetch features, no tokio/reqwest tree.
- `rcgen` for built-in ES256 self-signed cert generation (PEM output),
  replacing c2patool's openssl dependency.

## Module Layout

New file `src/c2pa.rs` (flat module, matching `learned.rs` style):

```rust
pub fn init_cert(org: Option<&str>, out_dir: &Path) -> anyhow::Result<(PathBuf, PathBuf)>
// rcgen: ECDSA P256 key, self-signed cert, CN = org.unwrap_or("Sigil User"),
// validity 2 years, outputs cert.pem + private.key (0600 perms on key)

pub struct WatermarkClaim { pub mode: String, pub recipient_id: Option<String>, pub keyed: bool }

pub fn sign_image(input: &Path, output: &Path, cert: &Path, key: &Path, claim: &WatermarkClaim) -> anyhow::Result<()>
// Manifest::new(format!("sigil/{}", env!("CARGO_PKG_VERSION"))),
// add_assertion("com.sigil.watermark", serde_json::to_vec(claim)?),
// create_signer::from_files(cert, key, SigningAlg::Es256, None),
// manifest.embed(input, output, signer.as_ref())

pub fn verify_image(input: &Path) -> anyhow::Result<C2paReport>
// ManifestStore::from_file -> active manifest -> format + signature status,
// deserialize com.sigil.watermark assertion back into WatermarkClaim if present

#[derive(Serialize)]
pub struct C2paReport {
    pub present: bool,
    pub claim_generator: Option<String>,
    pub signature_status: String,   // "valid" | "invalid" | "not signed"
    pub watermark_claim: Option<WatermarkClaim>,
}
```

CLI wiring in `cli.rs`: `Commands::C2pa(C2paArgs)` with sub-subcommand enum
`C2paCommand::{Init, Sign, Verify}`. Embed/verify args gain `--c2pa` plus the
cert/key options. `embed.rs` / `verify.rs` gain a small post-step that calls
into `c2pa.rs` when the flag is set (cfg-gated).

## Dual-Layer Linkage

- `embed --c2pa`: run the pixel watermark embed first, then sign the _output_
  image with a manifest whose `com.sigil.watermark` assertion mirrors the
  embed parameters (mode, recipient-id, keyed flag). Never sign the input.
- `verify --c2pa`: run watermark verification as usual, then call
  `verify_image`; the final JSON report contains both the watermark section
  and the `c2pa` section. Exit code remains watermark-based; C2PA status is
  informational unless `--c2pa-strict` is added later (out of scope now).

## Trust Model

Self-signed certs mean the C2PA layer proves "signed by the holder of this
key", not "signed by a known entity". The report includes cert subject +
validity window so users can pin a known cert. This mirrors c2patool's
personal-cert workflow and is documented in README.

## CI Changes

1. `ci.yml`: build/clippy/test matrix switches from `--features learned` to
   `--features learned,c2pa`.
2. `release.yml`: 5-platform build command gains `,c2pa`; nfpm config and AUR
   PKGBUILD (`sigil-wm`) `cargo build --features learned,c2pa`.
3. AUR `_vmver` unchanged; no new system deps thanks to rust_native_crypto.

## Testing

- Unit: WatermarkClaim serde round-trip; rcgen cert parses in
  create_signer::from_files.
- Integration (tests/c2pa_tests.rs, gated on feature):
  - init → sign → verify: signature valid, claim round-trips
  - verify on unsigned image: present=false
  - embed --c2pa → verify --c2pa: watermark + manifest claim consistent
  - sign wrong key (regenerated cert) → verification fails
  - JPEG + PNG both round-trip (JUMBF APP11 / iTXt paths)
- No network access in tests (c2pa http features disabled by construction).

## Release Notes / Docs

- README: "Content Credentials" section with 3-command quickstart and the
  self-signed trust caveat.
- roadmap.md / product-roadmap.md: check the C2PA box.
