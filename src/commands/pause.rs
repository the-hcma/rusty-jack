//! `rusty-jack pause` — stop the daemon without uninstalling.

use crate::launchd::{pause_daemon, print_pause_result};
use anyhow::Result;

/// Stop auto-routing; keeps the LaunchAgent plist for `resume`.
pub fn run(json: bool) -> Result<()> {
    let result = pause_daemon().map_err(anyhow::Error::new)?;

    if json {
        let value = serde_json::to_string_pretty(&result)?;
        println!("{value}");
    } else {
        print_pause_result(&result);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::launchd::PauseResult;

    #[test]
    fn test_run_json_when_not_installed() {
        let json = serde_json::to_string(&PauseResult::NotInstalled {
            plist_path: "/tmp/com.example.rusty-jack.plist".into(),
        })
        .unwrap();
        assert!(json.contains("\"status\":\"not_installed\""));
    }
}
