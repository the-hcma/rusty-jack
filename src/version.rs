//! Build-time version metadata (from `build.rs` + `Cargo.toml`).

/// Version string shown in `--help` and `-V` / `--version` (includes git commit).
pub const VERSION: &str = env!("RUSTY_JACK_VERSION");

/// Short git commit hash at build time (`unknown` when not built from a git checkout).
pub const GIT_COMMIT: &str = env!("RUSTY_JACK_GIT_COMMIT");

/// clap help header: name, version+commit, about, usage, subcommands, options.
pub const HELP_TEMPLATE: &str = "\
{name} {version}
{about-with-newline}\n\
{usage-heading} {usage}\n\n\
{all-args}\
";
