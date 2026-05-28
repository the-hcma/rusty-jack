//! `rusty-jack config` — config initialization and validation helpers.

use crate::config::{load_config, resolve_config_path};
use crate::coreaudio::AudioHal;
use crate::setup::{ensure_config_at_path, print_config_setup_result, terminal_is_interactive};
use anyhow::{Context, Result};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ValidateResult {
    Valid {
        config_path: String,
        canonicalized: bool,
    },
}

pub fn init(hal: &dyn AudioHal, json: bool, config_path: Option<&Path>) -> Result<()> {
    let interactive = !json && terminal_is_interactive();
    let path = resolve_config_path(config_path).context(
        "no config path — use --config or set RUSTY_JACK_CONFIG (or use the default path)",
    )?;
    let result = ensure_config_at_path(&path, hal, interactive).map_err(anyhow::Error::new)?;

    if json {
        let value = serde_json::to_string_pretty(&serde_json::json!({
            "config": result,
        }))?;
        println!("{value}");
    } else {
        print_config_setup_result(&result);
    }

    Ok(())
}

pub fn validate(json: bool, config_path: Option<&Path>) -> Result<()> {
    let path = resolve_config_path(config_path)
        .context("no config path — use --config or ~/.config/rusty-jack/config.json")?;
    let raw_before = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;
    load_config(&path).map_err(anyhow::Error::new)?;
    let raw_after = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;

    let result = ValidateResult::Valid {
        config_path: path.display().to_string(),
        canonicalized: raw_before != raw_after,
    };

    if json {
        let value = serde_json::to_string_pretty(&serde_json::json!({
            "config": result,
        }))?;
        println!("{value}");
    } else {
        match &result {
            ValidateResult::Valid {
                config_path,
                canonicalized,
            } => {
                println!("Config is valid");
                println!("  path: {config_path}");
                if *canonicalized {
                    println!("  note: rewrote file in canonical key order");
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_validate_rewrites_and_reports_canonicalized() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            r#"{{
  "version": 1,
  "preferred_device": {{ "uid": "hdmi" }},
  "poll_interval_ms": 3000,
  "also_set_system_output": true
}}"#
        )
        .unwrap();

        // Intentionally scramble order.
        std::fs::write(
            file.path(),
            r#"{
  "version": 1,
  "preferred_device": { "uid": "hdmi" },
  "also_set_system_output": true,
  "poll_interval_ms": 3000
}
"#,
        )
        .unwrap();

        let before = std::fs::read_to_string(file.path()).unwrap();
        validate(true, Some(file.path())).unwrap();
        let after = std::fs::read_to_string(file.path()).unwrap();
        assert_ne!(before, after);
    }
}
