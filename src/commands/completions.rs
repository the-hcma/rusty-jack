//! `rusty-jack completions <shell>` — print a shell completion script to stdout.

use crate::cli::Cli;
use clap::CommandFactory;
use clap_complete::Shell;

/// Write the completion script for `shell` to stdout.
pub fn run(shell: Shell) {
    let mut cmd = Cli::command();
    clap_complete::generate(shell, &mut cmd, "rusty-jack", &mut std::io::stdout());
}
