//! `rusty-jack apply` — apply config policy once.

use crate::apply::{apply_policy, print_text};
use crate::config::{load_config, resolve_config_path};
use crate::coreaudio::AudioHal;
use anyhow::{Context, Result};
use std::path::Path;

/// Load config and set the default output to the policy target.
pub fn run(hal: &dyn AudioHal, json: bool, config_path: Option<&Path>) -> Result<()> {
    let path = resolve_config_path(config_path)
        .context("no config path — use --config or ~/.config/rusty-jack/config.json")?;
    let config = load_config(&path).map_err(anyhow::Error::new)?;
    let result = apply_policy(hal, &config).map_err(anyhow::Error::new)?;

    if json {
        let value = serde_json::to_string_pretty(&result)?;
        println!("{value}");
    } else {
        print_text(&result);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coreaudio::mock::MockHal;
    use crate::output_device::OutputDevice;
    use crate::transport::TransportKind;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_run_apply_with_config_file() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            r#"{{
  "version": 1,
  "preferred_device": {{ "uid": "hdmi" }},
  "also_set_system_output": true
}}"#
        )
        .unwrap();

        let hal = MockHal::new(vec![OutputDevice {
            id: 1,
            uid: "hdmi".into(),
            name: "Monitor".into(),
            transport: TransportKind::Hdmi,
            is_alive: true,
            is_default: false,
            is_active: false,
            monitor_name: Some("TV".into()),
        }]);

        run(&hal, true, Some(file.path())).unwrap();
    }
}
