//! Clap CLI definition (testable separately from `main`).

use crate::version::{self, HELP_TEMPLATE};
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "rusty-jack",
    about = "Keep macOS system audio on your chosen HDMI (or dock) output",
    version = version::VERSION,
    long_version = version::VERSION,
    help_template = HELP_TEMPLATE,
)]
pub struct Cli {
    /// Path to config JSON (overrides `RUSTY_JACK_CONFIG` and default file)
    #[arg(long, global = true)]
    pub config: Option<std::path::PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

/// Subcommands (enum order = display order in `--help`; keep alphabetical).
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Apply config policy once
    Apply(ApplyArgs),
    /// Run the background supervisor loop (used by launchd)
    Daemon,
    /// Uninstall the launchd LaunchAgent (stop, disable, remove plist)
    Disable(DisableArgs),
    /// Install and start the launchd LaunchAgent for this user
    Install(InstallArgs),
    /// List audio output devices
    List(ListArgs),
    /// Pause the daemon (stop auto-routing; keeps LaunchAgent installed)
    Pause(PauseArgs),
    /// Interactively pick an output device and switch to it
    Picker(PickerArgs),
    /// Resume a paused daemon
    Resume(ResumeArgs),
    /// Show current default output and policy status
    Status(StatusArgs),
    /// Uninstall the launchd LaunchAgent (alias for disable)
    Uninstall(UninstallArgs),
    /// Refresh the LaunchAgent to the current binary and restart it
    Upgrade(UpgradeArgs),
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
pub struct PickerArgs {
    /// Emit JSON instead of human-readable text
    #[arg(long)]
    pub json: bool,

    /// Device list index (same as `list` IDX column); skips interactive menu
    #[arg(long)]
    pub index: Option<usize>,
}

#[derive(Parser, Debug)]
pub struct DisableArgs {
    /// Emit JSON instead of human-readable text
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct InstallArgs {
    /// Emit JSON instead of human-readable text
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct PauseArgs {
    /// Emit JSON instead of human-readable text
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct ResumeArgs {
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

#[derive(Parser, Debug)]
pub struct UninstallArgs {
    /// Emit JSON instead of human-readable text
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct UpgradeArgs {
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
    fn test_subcommands_alphabetical_in_help() {
        use clap::CommandFactory;

        let help = Cli::command().render_help().to_string();
        let names = help
            .lines()
            .filter_map(|line| line.split_whitespace().next())
            .skip_while(|word| *word != "Commands:")
            .skip(1)
            .take_while(|line| *line != "Options:")
            .filter(|word| *word != "help")
            .collect::<Vec<_>>();

        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "subcommands in --help:\n{help}");
    }

    #[test]
    fn test_help_shows_version_and_commit() {
        use clap::CommandFactory;

        let help = Cli::command().render_help().to_string();
        assert!(
            help.contains(crate::version::VERSION),
            "help should include version+commit: {help}"
        );
        assert!(help.starts_with("rusty-jack"));
    }

    #[test]
    fn test_help_shows_copyright() {
        use clap::CommandFactory;

        let help = Cli::command().render_help().to_string();
        assert!(help.contains(crate::version::COPYRIGHT));
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
    fn test_disable_json_flag() {
        let cli = Cli::try_parse_from(["rusty-jack", "disable", "--json"]).unwrap();
        match cli.command {
            Commands::Disable(args) => assert!(args.json),
            _ => panic!("expected disable"),
        }
    }

    #[test]
    fn test_pause_json_flag() {
        let cli = Cli::try_parse_from(["rusty-jack", "pause", "--json"]).unwrap();
        match cli.command {
            Commands::Pause(args) => assert!(args.json),
            _ => panic!("expected pause"),
        }
    }

    #[test]
    fn test_install_json_flag() {
        let cli = Cli::try_parse_from(["rusty-jack", "install", "--json"]).unwrap();
        match cli.command {
            Commands::Install(args) => assert!(args.json),
            _ => panic!("expected install"),
        }
    }

    #[test]
    fn test_resume_json_flag() {
        let cli = Cli::try_parse_from(["rusty-jack", "resume", "--json"]).unwrap();
        match cli.command {
            Commands::Resume(args) => assert!(args.json),
            _ => panic!("expected resume"),
        }
    }

    #[test]
    fn test_picker_index_flag() {
        let cli = Cli::try_parse_from(["rusty-jack", "picker", "--index", "2"]).unwrap();
        match cli.command {
            Commands::Picker(args) => assert_eq!(args.index, Some(2)),
            _ => panic!("expected picker"),
        }
    }

    #[test]
    fn test_uninstall_json_flag() {
        let cli = Cli::try_parse_from(["rusty-jack", "uninstall", "--json"]).unwrap();
        match cli.command {
            Commands::Uninstall(args) => assert!(args.json),
            _ => panic!("expected uninstall"),
        }
    }

    #[test]
    fn test_upgrade_json_flag() {
        let cli = Cli::try_parse_from(["rusty-jack", "upgrade", "--json"]).unwrap();
        match cli.command {
            Commands::Upgrade(args) => assert!(args.json),
            _ => panic!("expected upgrade"),
        }
    }

    #[test]
    fn test_global_config_flag() {
        let cli = Cli::try_parse_from(["rusty-jack", "--config", "/tmp/rusty-jack.json", "status"])
            .unwrap();
        assert_eq!(
            cli.config.as_deref(),
            Some(std::path::Path::new("/tmp/rusty-jack.json"))
        );
    }
}
