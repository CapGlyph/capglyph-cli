#[cfg(not(target_arch = "wasm32"))]
use clap::Parser;
#[cfg(not(target_arch = "wasm32"))]
use tracing::Level;
#[cfg(not(target_arch = "wasm32"))]
use tracing_subscriber::FmtSubscriber;

#[cfg(not(target_arch = "wasm32"))]
use sigil::cli::{Cli, Commands};
#[cfg(not(target_arch = "wasm32"))]
use sigil::{batch, embed, extract, info, strip, verify};

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let level = if cli.verbose {
        Level::DEBUG
    } else {
        Level::INFO
    };
    let _ = tracing::subscriber::set_global_default(
        FmtSubscriber::builder().with_max_level(level).finish(),
    );

    match &cli.command {
        Commands::Embed(args) => embed::run(args)?,
        Commands::Verify(args) => {
            let present = verify::run(args)?;
            if !present {
                std::process::exit(verify::EXIT_ABSENT);
            }
        }
        Commands::Strip(args) => strip::run(args)?,
        Commands::Info(args) => info::info(args)?,
        Commands::Batch(args) => batch::batch(args)?,
        Commands::Extract(args) => {
            let id = extract::run(args)?;
            println!("{}", id);
        }
        #[cfg(feature = "learned")]
        Commands::FetchModels(args) => {
            let dir = sigil::learned::model_dir(args.model_dir.as_deref());
            sigil::learned::fetch_models(&dir)?;
            println!("Models downloaded to {:?}", dir);
        }
        #[cfg(feature = "c2pa")]
        Commands::C2pa(args) => {
            let code = sigil::c2pa_cli::run(&args.command)?;
            if code != 0 {
                std::process::exit(code);
            }
        }
    }

    Ok(())
}
