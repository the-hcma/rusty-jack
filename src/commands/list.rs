//! `rusty-jack list` — enumerate output devices.

use crate::coreaudio::AudioHal;
use crate::list_fmt;
use crate::output_device::filter_hdmi_devices;
use anyhow::Result;

/// List output devices, optionally filtered to HDMI-class transports.
pub fn run(hal: &dyn AudioHal, hdmi_only: bool, json: bool) -> Result<()> {
    let mut list = hal.list_outputs()?;

    if hdmi_only {
        list.devices = filter_hdmi_devices(&list.devices);
    }

    if json {
        list_fmt::print_json(&list)?;
    } else {
        list_fmt::print_table(&list, hdmi_only)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coreaudio::mock::MockHal;
    use crate::output_device::OutputDevice;
    use crate::transport::TransportKind;

    fn fixture_devices() -> Vec<OutputDevice> {
        vec![
            OutputDevice {
                id: 1,
                uid: "builtin".into(),
                name: "Speakers".into(),
                transport: TransportKind::BuiltIn,
                is_alive: true,
                is_default: true,
                is_active: true,
                monitor_name: None,
            },
            OutputDevice {
                id: 2,
                uid: "hdmi-uid".into(),
                name: "Monitor".into(),
                transport: TransportKind::Hdmi,
                is_alive: true,
                is_default: false,
                is_active: false,
                monitor_name: Some("LG TV".into()),
            },
        ]
    }

    #[test]
    fn test_list_hdmi_filter_via_mock() {
        let hal = MockHal::new(fixture_devices());
        let all = hal.list_outputs().unwrap().devices;
        let hdmi = filter_hdmi_devices(&all);
        assert_eq!(hdmi.len(), 1);
        assert_eq!(hdmi[0].uid, "hdmi-uid");
    }

    #[test]
    fn test_run_json_does_not_panic() {
        let hal = MockHal::new(fixture_devices());
        run(&hal, false, true).unwrap();
    }
}
