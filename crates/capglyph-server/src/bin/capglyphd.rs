//! capglyphd — credential server binary
//!
//! MVP: SQLite-backed issuing / verify / consume / revoke over HTTP.
//! Keep `image bytes never a cryptographic key` — keys derived via KMS.

#[cfg(not(target_arch = "wasm32"))]
use std::net::SocketAddr;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

#[cfg(not(target_arch = "wasm32"))]
use capglyph_server::{Db, Kms, Service};

#[cfg(target_arch = "wasm32")]
fn main() {
    eprintln!("capglyphd does not run on wasm32");
}

#[cfg(not(target_arch = "wasm32"))]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Minimal CLI: `capglyphd --db ./capglyphd.db --listen 127.0.0.1:3000`
    let args: Vec<String> = std::env::args().collect();
    let mut db_path: Option<PathBuf> = None;
    let mut listen: String = "127.0.0.1:3000".to_string();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--db" => {
                i += 1;
                if i < args.len() {
                    db_path = Some(PathBuf::from(&args[i]));
                }
            }
            "--listen" => {
                i += 1;
                if i < args.len() {
                    listen = args[i].clone();
                }
            }
            "--help" | "-h" => {
                println!("capglyphd — CapGlyph credential server MVP");
                println!("Usage: capglyphd [--db PATH] [--listen ADDR]");
                println!("  --db PATH      SQLite file (default: in-memory)");
                println!("  --listen ADDR  HTTP listen addr (default: 127.0.0.1:3000)");
                println!("Env: CAPGLYPHD_MASTER_KEY (exactly 32 non-zero bytes as hex)");
                return Ok(());
            }
            _ => {}
        }
        i += 1;
    }

    tracing_subscriber::fmt::init();

    let persistent_db = db_path.is_some();
    let db = if let Some(p) = db_path {
        eprintln!("capglyphd: opening db at {:?}", p);
        Db::new(p)?
    } else {
        eprintln!("capglyphd: using in-memory db (ephemeral)");
        Db::new_in_memory()?
    };

    // KMS: persistent deployments must provide a stable, valid master key.
    let mut kms = Kms::new();
    if let Ok(hex_key) = std::env::var("CAPGLYPHD_MASTER_KEY") {
        let bytes = hex::decode(hex_key.trim())
            .map_err(|_| anyhow::anyhow!("CAPGLYPHD_MASTER_KEY must be valid hexadecimal"))?;
        if bytes.len() != 32 {
            anyhow::bail!("CAPGLYPHD_MASTER_KEY must decode to exactly 32 bytes");
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        if arr == [0u8; 32] {
            anyhow::bail!("CAPGLYPHD_MASTER_KEY must not be the all-zero key");
        }
        kms = kms.with_key("default", arr);
        kms = kms.with_key("cred-2026-08", arr);
        eprintln!("capglyphd: loaded master from CAPGLYPHD_MASTER_KEY");
    } else if persistent_db {
        anyhow::bail!("CAPGLYPHD_MASTER_KEY is required with --db");
    } else {
        kms.generate_key_id("default");
        kms.generate_key_id("cred-2026-08");
        eprintln!(
            "capglyphd: generated ephemeral master keys for the in-memory development database"
        );
    }

    let svc = Service::new(db, kms);

    // Ensure demo cover exists so `POST /v1/credentials` works without pre-seeding
    let _ = svc.ensure_demo_cover();

    let app = capglyph_server::router(svc);
    let addr: SocketAddr = listen.parse()?;
    eprintln!("capglyphd: listening on http://{}", addr);
    eprintln!("  GET  /v1/version               — API and wire versions");
    eprintln!("  POST /v1/credentials           — issue");
    eprintln!("  POST /v1/credentials/verify    — verify (read-only)");
    eprintln!("  POST /v1/credentials/consume   — consume (atomic, Idempotency-Key)");
    eprintln!("  GET  /v1/credentials/:id       — status");
    eprintln!("  POST /v1/credentials/:id/revoke — revoke");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
