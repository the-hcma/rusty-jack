//! `rusty-jack resume` — restart a paused daemon.

use crate::launchd::{print_resume_result, resume_daemon};
use anyhow::Result;

/// Re-enable and load the LaunchAgent after `pause`.
pub fn run(json: bool) -> Result<()> {
    let result = resume_daemon().map_err(anyhow::Error::new)?;

    if json {
        let value = serde_json::to_string_pretty(&result)?;
        println!("{value}");
    } else {
        print_resume_result(&result);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launchd::ResumeResult;

    #[test]
    fn test_run_json_when_not_installed() {
        if matches!(resume_daemon().unwrap(), ResumeResult::NotInstalled { .. }) {
            run(true).unwrap();
        }
    }
}
