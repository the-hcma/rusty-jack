//! Rusty Jack — macOS HDMI output router.

pub mod activity;
pub mod apply;
pub mod cli;
pub mod commands;
pub mod config;
pub mod coreaudio;
pub mod daemon;
pub mod device_select;
pub mod display;
pub mod eqmac;
pub mod error;
pub mod hal_plugin;
pub mod launchd;
pub mod list_fmt;
pub mod network;
pub mod output_device;
pub mod picker;
pub mod policy;
pub mod setup;
pub mod sony;
pub mod status;
pub mod system_default;
pub mod transport;
pub mod version;
pub mod volume_memory;
pub mod volume_result;

pub use error::RustyJackError;

/// Run the CLI (entry point for binary and integration tests).
///
/// # Errors
///
/// Returns an error if CoreAudio enumeration fails or a subcommand fails.
pub fn run_cli(cli: cli::Cli) -> anyhow::Result<()> {
    match cli.command {
        cli::Commands::Disable(args) => commands::disable::run(args.json)?,
        cli::Commands::Install(args) => {
            let hal = coreaudio::platform_hal()?;
            commands::install::run(hal.as_ref(), args.json)?;
        }
        cli::Commands::Pause(args) => commands::pause::run(args.json)?,
        cli::Commands::Resume(args) => {
            let hal = coreaudio::platform_hal()?;
            commands::resume::run(hal.as_ref(), args.json, cli.config.as_deref())?;
        }
        cli::Commands::Uninstall(args) => {
            commands::uninstall::run(args.json, args.remove_config, args.keep_config)?
        }
        cli::Commands::Upgrade(args) => commands::upgrade::run(args.json)?,
        cli::Commands::Daemon => {
            let hal = coreaudio::platform_hal()?;
            commands::daemon::run(hal.as_ref(), cli.config.as_deref())?;
        }
        cli::Commands::List(args) => {
            let hal = coreaudio::platform_hal()?;
            commands::list::run(hal.as_ref(), args.hdmi, args.json)?;
        }
        cli::Commands::Status(args) => {
            let hal = coreaudio::platform_hal()?;
            commands::status::run(hal.as_ref(), args.json, cli.config.as_deref())?;
        }
        cli::Commands::Apply(args) => {
            let hal = coreaudio::platform_hal()?;
            commands::apply::run(hal.as_ref(), args.json, cli.config.as_deref())?;
        }
        cli::Commands::Picker(args) => {
            let hal = coreaudio::platform_hal()?;
            commands::picker::run(hal.as_ref(), args.json, args.index, cli.config.as_deref())?;
        }
    }

    Ok(())
}
