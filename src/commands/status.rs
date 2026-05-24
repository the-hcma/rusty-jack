//! `rusty-jack status` — show active/default output and policy state.

use crate::coreaudio::AudioHal;
use crate::status::{build_status, print_json, print_text};
use anyhow::Result;

/// Show current default/active output and policy status.
pub fn run(hal: &dyn AudioHal, json: bool) -> Result<()> {
    let list = hal.list_outputs()?;
    let snapshot = build_status(list);

    if json {
        print_json(&snapshot)?;
    } else {
        print_text(&snapshot)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coreaudio::mock::MockHal;
    use crate::output_device::OutputDevice;
    use crate::transport::TransportKind;

    #[test]
    fn test_run_json_does_not_panic() {
        let hal = MockHal::new(vec![OutputDevice {
            id: 1,
            uid: "hdmi".into(),
            name: "Monitor".into(),
            transport: TransportKind::Hdmi,
            is_alive: true,
            is_default: true,
            is_active: true,
            monitor_name: Some("LG TV".into()),
        }]);
        run(&hal, true).unwrap();
    }
}
