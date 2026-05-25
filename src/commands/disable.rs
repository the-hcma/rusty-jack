//! `rusty-jack disable` — uninstall the launchd LaunchAgent.

use crate::launchd::{print_disable_result, uninstall_daemon};
use anyhow::Result;

/// Stop the daemon, remove it from launchd, and delete the LaunchAgent plist.
pub fn run(json: bool) -> Result<()> {
    let result = uninstall_daemon().map_err(anyhow::Error::new)?;

    if json {
        let value = serde_json::to_string_pretty(&result)?;
        println!("{value}");
    } else {
        print_disable_result(&result);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launchd::DisableResult;

    #[test]
    fn test_run_json_when_not_installed() {
        if matches!(
            uninstall_daemon().unwrap(),
            DisableResult::NotInstalled { .. }
        ) {
            run(true).unwrap();
        }
    }
}
