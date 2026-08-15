//! CLI dispatch for the `sigil c2pa` command group.

use std::path::PathBuf;

use anyhow::{Context, Result};
use tracing::info;

use crate::c2pa::WatermarkClaim;
use crate::cli::{C2paCommand, C2paSignArgs};

const DEFAULT_CERT_DIR: &str = "./sigil-certs/";

/// Entry point for the `sigil c2pa` subcommand group.
///
/// Returns the process exit code: 0 = valid, 1 = invalid, 2 = unsigned.
pub fn run(cmd: &C2paCommand) -> Result<i32> {
    match cmd {
        C2paCommand::Init(args) => {
            let dir = args
                .out
                .clone()
                .unwrap_or_else(|| PathBuf::from(DEFAULT_CERT_DIR));
            let (cert, key) = crate::c2pa::init_cert(args.org.as_deref(), &dir, args.force)?;
            println!("Certificate written: {cert:?}");
            println!("Private key written: {key:?}");
            Ok(0)
        }
        C2paCommand::Sign(args) => {
            let output = resolve_sign_output(args)?;
            let claim = match (&args.recipient_id, &args.mode) {
                (Some(id), Some(mode)) => WatermarkClaim {
                    mode: mode.to_string(),
                    recipient_id: Some(id.clone()),
                    keyed: args.key.is_some(),
                },
                _ => WatermarkClaim {
                    mode: "none".to_string(),
                    recipient_id: None,
                    keyed: false,
                },
            };
            crate::c2pa::sign_image(
                &args.input,
                &output,
                &args.cert,
                &args.pkey,
                &claim,
                args.manifest_json.as_deref(),
                Some(&args.source_type),
            )?;
            info!("C2PA manifest signed: {:?} -> {:?}", args.input, output);
            println!("C2PA manifest signed -> {:?}", output);
            Ok(0)
        }
        C2paCommand::Verify(args) => {
            let report = crate::c2pa::verify_image(&args.input)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&report).context("Failed to serialize report")?
            );
            match report.signature_status.as_str() {
                "valid" => Ok(0),
                "invalid" => Ok(1),
                _ => Ok(2),
            }
        }
    }
}

fn resolve_sign_output(args: &C2paSignArgs) -> Result<PathBuf> {
    if let Some(out) = &args.output {
        return Ok(out.clone());
    }
    anyhow::bail!("-o <output> is required: in-place signing would destroy the source on failure");
}
