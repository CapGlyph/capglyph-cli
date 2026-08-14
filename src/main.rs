use clap::Parser;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

use sigil::cli::{Cli, Commands};
use sigil::{batch, embed, extract, info, strip, verify};

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
    }

    Ok(())
}
