//! Rusty Jack — macOS HDMI output router.

pub mod cli;
pub mod commands;
pub mod coreaudio;
pub mod display;
pub mod error;
pub mod hal_plugin;
pub mod list_fmt;
pub mod output_device;
pub mod status;
pub mod system_default;
pub mod transport;

pub use error::RustyJackError;

/// Run the CLI (entry point for binary and integration tests).
///
/// # Errors
///
/// Returns an error if CoreAudio enumeration fails or an unimplemented subcommand is invoked.
pub fn run_cli(cli: cli::Cli) -> anyhow::Result<()> {
    let hal = coreaudio::platform_hal()?;

    match cli.command {
        cli::Commands::List(args) => {
            commands::list::run(hal.as_ref(), args.hdmi, args.json)?;
        }
        cli::Commands::Status(args) => {
            commands::status::run(hal.as_ref(), args.json)?;
        }
        cli::Commands::Apply => {
            anyhow::bail!("apply is not implemented yet");
        }
        cli::Commands::Daemon => {
            anyhow::bail!("daemon is not implemented yet");
        }
    }

    Ok(())
}
