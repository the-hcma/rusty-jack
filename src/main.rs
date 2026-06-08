//! Rusty Jack binary entry point.

use anyhow::Result;
use clap::Parser;
use rusty_jack::cli::Cli;

fn main() -> Result<()> {
    let cli = Cli::parse();
    rusty_jack::run_cli(cli)
}
