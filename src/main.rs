//! Rusty Jack binary entry point.

use anyhow::Result;
use clap::Parser;
use rusty_jack::cli::Cli;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rusty_jack=info".into()),
        )
        .init();

    let cli = Cli::parse();
    rusty_jack::run_cli(cli)
}
