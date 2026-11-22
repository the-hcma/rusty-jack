//! `rusty-jack doctor` — verify macOS privacy permissions and explain what is missing.

use crate::config::{load_config_optional, resolve_config_path};
use crate::doctor::build_privacy_report;
use anyhow::Result;
use std::path::Path;

/// Run the diagnostics; exits non-zero when a required permission is missing.
pub fn run(json: bool, config_path: Option<&Path>) -> Result<()> {
    let resolved = resolve_config_path(config_path);
    let config = match resolved.as_deref() {
        Some(path) => load_config_optional(path, config_path.is_some())?,
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
        if report.has_failures() {
            println!();
            println!("Fix the FAIL items above, then re-run `rusty-jack doctor`.");
        } else if report.has_warnings() {
            println!();
            println!("No required permission is confirmed missing; see warnings above.");
        } else {
            println!();
            println!("All required permissions are in place.");
        }
    }

    if report.has_failures() {
        anyhow::bail!("required privacy permissions are missing");
    }
    Ok(())
}
