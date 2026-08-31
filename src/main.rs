#[cfg(not(target_arch = "wasm32"))]
use clap::Parser;
#[cfg(not(target_arch = "wasm32"))]
use tracing::Level;
#[cfg(not(target_arch = "wasm32"))]
use tracing_subscriber::FmtSubscriber;

#[cfg(not(target_arch = "wasm32"))]
use capglyph::cli::{Cli, Commands, ConformanceCommand};
#[cfg(not(target_arch = "wasm32"))]
use capglyph::{batch, conformance, embed, extract, info, pointer, strip, verify};

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
        Commands::Pointer(args) => match &args.command {
            capglyph::cli::PointerCommand::Embed(a) => pointer::run_pointer_embed(a)?,
            capglyph::cli::PointerCommand::Extract(a) => pointer::run_pointer_extract(a)?,
            capglyph::cli::PointerCommand::OfflineEmbed(a) => {
                pointer::run_pointer_offline_embed(a)?
            }
            capglyph::cli::PointerCommand::OfflineExtract(a) => {
                pointer::run_pointer_offline_extract(a)?
            }
        },
        Commands::Message(args) => match &args.command {
            capglyph::cli::MessageCommand::Encrypt(a) => pointer::run_pointer_embed(a)?,
            capglyph::cli::MessageCommand::Decrypt(a) => pointer::run_pointer_extract(a)?,
            capglyph::cli::MessageCommand::Store(a) => pointer::run_message_store(a)?,
            capglyph::cli::MessageCommand::Resolve(a) => pointer::run_message_resolve(a)?,
        },
        Commands::Conformance(args) => match &args.command {
            ConformanceCommand::Test(t) => conformance::run(t)?,
        },
        #[cfg(feature = "learned")]
        Commands::FetchModels(args) => {
            let dir = capglyph::learned::model_dir(args.model_dir.as_deref());
            capglyph::learned::fetch_models(&dir)?;
            println!("Models downloaded to {:?}", dir);
        }
        #[cfg(feature = "c2pa")]
        Commands::C2pa(args) => {
            let code = capglyph::c2pa_cli::run(&args.command)?;
            if code != 0 {
                std::process::exit(code);
            }
        }
    }

    Ok(())
}
