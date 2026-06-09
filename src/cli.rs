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
    /// Manage configuration (init, validate)
    Config(ConfigArgs),
    /// Run the background supervisor loop (used by launchd)
    Daemon,
    /// Uninstall the launchd LaunchAgent (stop, disable, remove plist)
    Disable(DisableArgs),
    /// Test native driver by swapping eqMac's HAL driver out and back in
    Driver(DriverArgs),
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
    /// ScalarWebAPI-compatible speaker helpers
    #[command(name = "scalar-webapi-device")]
    ScalarWebapiDevice(ScalarWebapiDeviceArgs),
    /// Show current default output and policy status
    Status(StatusArgs),
    /// Uninstall the launchd LaunchAgent (alias for disable)
    Uninstall(UninstallArgs),
    /// Refresh the LaunchAgent to the current binary when needed
    Upgrade(UpgradeArgs),
}

#[derive(Parser, Debug)]
pub struct DriverArgs {
    #[command(subcommand)]
    pub command: DriverCommand,
}

#[derive(Subcommand, Debug)]
pub enum DriverCommand {
    /// Back up eqMac's HAL driver and install Rusty Jack's driver
    SwapIn(DriverSwapArgs),
    /// Remove Rusty Jack's driver and restore the backed-up eqMac driver
    SwapOut(DriverSwapArgs),
    /// Reinstall eqMac's HAL driver (backup, embedded copy, or refresh) and restart CoreAudio
    RestoreEqMac(DriverSwapArgs),
}

#[derive(Parser, Debug)]
pub struct DriverSwapArgs {
    /// Emit JSON instead of human-readable text
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct ListArgs {
    /// Only show HDMI, DisplayPort, Thunderbolt, and USB dock outputs
    #[arg(long)]
    pub hdmi: bool,

    /// Emit JSON instead of a table
    #[arg(long)]
    pub json: bool,

    /// Discover ScalarWebAPI speakers on the LAN and refresh cache for configured host
    #[arg(long)]
    pub discover: bool,
}

#[derive(Parser, Debug)]
pub struct ApplyArgs {
    /// Emit JSON instead of human-readable text
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Subcommand, Debug)]
pub enum ConfigCommand {
    /// Create the config file when missing (prompts in interactive mode)
    Init(ConfigInitArgs),
    /// Validate and canonicalize a config file
    Validate(ConfigValidateArgs),
}

#[derive(Parser, Debug)]
pub struct ConfigInitArgs {
    /// Emit JSON instead of human-readable text
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct ConfigValidateArgs {
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
pub struct ScalarWebapiDeviceArgs {
    #[command(subcommand)]
    pub command: ScalarWebapiDeviceCommand,
}

#[derive(Subcommand, Debug)]
pub enum ScalarWebapiDeviceCommand {
    /// Scan the LAN for ScalarWebAPI-compatible speakers
    Discover(ScalarWebapiDeviceDiscoverArgs),
}

#[derive(Parser, Debug)]
pub struct ScalarWebapiDeviceDiscoverArgs {
    /// Emit JSON instead of human-readable text
    #[arg(long)]
    pub json: bool,

    /// SSDP discovery timeout in milliseconds
    #[arg(long)]
    pub timeout_ms: Option<u64>,

    /// Fetch `mac_address` for the configured device and write it to the config file
    #[arg(long)]
    pub update_config: bool,
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

    /// Remove the default config file without prompting
    #[arg(long, conflicts_with_all = ["keep_config", "only_driver"])]
    pub remove_config: bool,

    /// Keep the default config file without prompting
    #[arg(long, conflicts_with_all = ["remove_config", "only_driver"])]
    pub keep_config: bool,

    /// Only remove the native audio driver; keep LaunchAgent, binary, and config
    #[arg(long)]
    pub only_driver: bool,

    /// Skip restoring the pre-install default output device, if present
    #[arg(long)]
    pub no_restore_audio: bool,

    /// Remove daemon log files (included automatically with `--remove-config`)
    #[arg(long, conflicts_with = "only_driver")]
    pub purge_logs: bool,

    /// Full cleanup: remove config, purge logs, and restore audio (same as `--remove-config --purge-logs`)
    #[arg(long, conflicts_with_all = ["only_driver", "keep_config", "remove_config", "purge_logs"])]
    pub purge: bool,
}

#[derive(Parser, Debug)]
pub struct UpgradeArgs {
    /// Emit JSON instead of human-readable text
    #[arg(long)]
    pub json: bool,

    /// Restart/rewrite even when the LaunchAgent already matches this binary
    #[arg(long)]
    pub force: bool,
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
    fn test_list_discover_flag() {
        let cli = Cli::try_parse_from(["rusty-jack", "list", "--discover"]).unwrap();
        match cli.command {
            Commands::List(args) => assert!(args.discover),
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
    fn test_config_init_json_flag() {
        let cli = Cli::try_parse_from(["rusty-jack", "config", "init", "--json"]).unwrap();
        match cli.command {
            Commands::Config(args) => match args.command {
                ConfigCommand::Init(args) => assert!(args.json),
                _ => panic!("expected init"),
            },
            _ => panic!("expected config"),
        }
    }

    #[test]
    fn test_config_validate_json_flag() {
        let cli = Cli::try_parse_from(["rusty-jack", "config", "validate", "--json"]).unwrap();
        match cli.command {
            Commands::Config(args) => match args.command {
                ConfigCommand::Validate(args) => assert!(args.json),
                _ => panic!("expected validate"),
            },
            _ => panic!("expected config"),
        }
    }

    #[test]
    fn test_driver_swap_flags() {
        let cli = Cli::try_parse_from(["rusty-jack", "driver", "swap-in", "--json"]).unwrap();
        match cli.command {
            Commands::Driver(args) => match args.command {
                DriverCommand::SwapIn(args) => assert!(args.json),
                _ => panic!("expected swap-in"),
            },
            _ => panic!("expected driver"),
        }

        let cli = Cli::try_parse_from(["rusty-jack", "driver", "swap-out", "--json"]).unwrap();
        match cli.command {
            Commands::Driver(args) => match args.command {
                DriverCommand::SwapOut(args) => assert!(args.json),
                _ => panic!("expected swap-out"),
            },
            _ => panic!("expected driver"),
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
    fn test_uninstall_config_flags() {
        let cli = Cli::try_parse_from(["rusty-jack", "uninstall", "--remove-config"]).unwrap();
        match cli.command {
            Commands::Uninstall(args) => {
                assert!(args.remove_config);
                assert!(!args.keep_config);
                assert!(!args.only_driver);
            }
            _ => panic!("expected uninstall"),
        }

        assert!(Cli::try_parse_from([
            "rusty-jack",
            "uninstall",
            "--remove-config",
            "--keep-config",
        ])
        .is_err());
    }

    #[test]
    fn test_uninstall_purge_flag() {
        let cli = Cli::try_parse_from(["rusty-jack", "uninstall", "--purge"]).unwrap();
        match cli.command {
            Commands::Uninstall(args) => {
                assert!(args.purge);
                assert!(!args.only_driver);
            }
            _ => panic!("expected uninstall"),
        }
    }

    #[test]
    fn test_scalar_webapi_device_discover_parses() {
        let cli = Cli::try_parse_from([
            "rusty-jack",
            "scalar-webapi-device",
            "discover",
            "--json",
            "--timeout-ms",
            "5000",
        ])
        .unwrap();
        match cli.command {
            Commands::ScalarWebapiDevice(args) => match args.command {
                ScalarWebapiDeviceCommand::Discover(args) => {
                    assert!(args.json);
                    assert_eq!(args.timeout_ms, Some(5000));
                }
            },
            _ => panic!("expected scalar-webapi-device"),
        }
    }

    #[test]
    fn test_uninstall_only_driver_flag() {
        let cli = Cli::try_parse_from(["rusty-jack", "uninstall", "--only-driver"]).unwrap();
        match cli.command {
            Commands::Uninstall(args) => {
                assert!(args.only_driver);
                assert!(!args.remove_config);
                assert!(!args.keep_config);
                assert!(!args.no_restore_audio);
            }
            _ => panic!("expected uninstall"),
        }

        assert!(Cli::try_parse_from([
            "rusty-jack",
            "uninstall",
            "--only-driver",
            "--remove-config",
        ])
        .is_err());
    }

    #[test]
    fn test_uninstall_no_restore_audio_flag() {
        let cli = Cli::try_parse_from(["rusty-jack", "uninstall", "--no-restore-audio"]).unwrap();
        match cli.command {
            Commands::Uninstall(args) => assert!(args.no_restore_audio),
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
    fn test_upgrade_force_flag() {
        let cli = Cli::try_parse_from(["rusty-jack", "upgrade", "--force"]).unwrap();
        match cli.command {
            Commands::Upgrade(args) => assert!(args.force),
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
