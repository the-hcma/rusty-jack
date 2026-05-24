//! Clap CLI definition (testable separately from `main`).

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "rusty-jack",
    about = "Keep macOS system audio on your chosen HDMI (or dock) output",
    version
)]
pub struct Cli {
    /// Path to config JSON (overrides `RUSTY_JACK_CONFIG` and default file)
    #[arg(long, global = true)]
    pub config: Option<std::path::PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// List audio output devices
    List(ListArgs),
    /// Show current default output and policy status
    Status(StatusArgs),
    /// Apply config policy once
    Apply(ApplyArgs),
    /// Run the background supervisor loop (used by launchd)
    Daemon,
}

#[derive(Parser, Debug)]
pub struct ListArgs {
    /// Only show HDMI, DisplayPort, Thunderbolt, and USB dock outputs
    #[arg(long)]
    pub hdmi: bool,

    /// Emit JSON instead of a table
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct ApplyArgs {
    /// Emit JSON instead of human-readable text
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct StatusArgs {
    /// Emit JSON instead of human-readable text
    #[arg(long)]
    pub json: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn test_cli_debug() {
        Cli::command().debug_assert();
    }

    #[test]
    fn test_list_hdmi_flag() {
        let cli = Cli::try_parse_from(["rusty-jack", "list", "--hdmi"]).unwrap();
        match cli.command {
            Commands::List(args) => assert!(args.hdmi),
            _ => panic!("expected list"),
        }
    }

    #[test]
    fn test_list_json_flag() {
        let cli = Cli::try_parse_from(["rusty-jack", "list", "--json"]).unwrap();
        match cli.command {
            Commands::List(args) => assert!(args.json),
            _ => panic!("expected list"),
        }
    }

    #[test]
    fn test_status_json_flag() {
        let cli = Cli::try_parse_from(["rusty-jack", "status", "--json"]).unwrap();
        match cli.command {
            Commands::Status(args) => assert!(args.json),
            _ => panic!("expected status"),
        }
    }

    #[test]
    fn test_apply_json_flag() {
        let cli = Cli::try_parse_from(["rusty-jack", "apply", "--json"]).unwrap();
        match cli.command {
            Commands::Apply(args) => assert!(args.json),
            _ => panic!("expected apply"),
        }
    }

    #[test]
    fn test_global_config_flag() {
        let cli = Cli::try_parse_from([
            "rusty-jack",
            "--config",
            "/tmp/rusty-jack.json",
            "status",
        ])
        .unwrap();
        assert_eq!(
            cli.config.as_deref(),
            Some(std::path::Path::new("/tmp/rusty-jack.json"))
        );
    }
}
