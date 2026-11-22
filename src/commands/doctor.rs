//! `rusty-jack doctor` — verify macOS privacy permissions and explain what is missing.

use crate::config::{load_config_optional_readonly, resolve_config_path};
use crate::doctor::build_privacy_report;
use anyhow::Result;
use std::path::Path;

/// Run the diagnostics; exits non-zero when a required permission is missing.
pub fn run(json: bool, config_path: Option<&Path>) -> Result<()> {
    let resolved = resolve_config_path(config_path);
    let config = match resolved.as_deref() {
        Some(path) => load_config_optional_readonly(path, config_path.is_some())?,
        None => None,
    };
    let report = build_privacy_report(config.as_ref(), true)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        match (&config, resolved.as_deref()) {
            (Some(_), Some(path)) => println!("Config: {}", path.display()),
            _ => println!(
                "Config: none loaded (permissions depend on config; run `rusty-jack config init`)"
            ),
        }
        println!();
        println!("{}", report.format("Privacy permissions"));
        println!();
        println!("{}", report.conclusion(config.is_some()));
    }

    if report.has_failures() {
        anyhow::bail!("required privacy permissions are missing");
    }
    Ok(())
}
