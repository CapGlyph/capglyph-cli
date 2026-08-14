use clap::Parser;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

mod cli;
mod embed;
mod geometry;
mod signal;
mod strip;
mod verify;

use cli::{Cli, Commands};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Logging
    let level = if cli.verbose { Level::DEBUG } else { Level::INFO };
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
    }

    Ok(())
}
