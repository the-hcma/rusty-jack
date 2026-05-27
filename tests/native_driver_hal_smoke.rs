//! Live HAL driver smoke test (macOS only, mutates system audio).
//!
//! ```bash
//! cd .worktrees/native-driver-hal-fix-wt   # or repo root on the HAL-fix branch
//! make driver-bundle
//! RUSTY_JACK_DRIVER_BUNDLE="$PWD/target/share/rusty-jack/RustyJack.driver" \
//!   RUSTY_JACK_HAL_DRIVER_SMOKE=1 \
//!   cargo test --test native_driver_hal_smoke -- --ignored --nocapture
//! ```
//!
//! Requires sudo for moving eqMac's HAL driver and installing Rusty Jack under `/Library`.
//! Teardown restores eqMac when the test finishes (even on failure).

#![cfg(target_os = "macos")]

use rusty_jack::coreaudio::platform_hal;
use rusty_jack::hdmi_displayport_volume_control::RUSTY_JACK_VIRTUAL_OUTPUT_UID;
use rusty_jack::native_driver_hal_smoke::{
    hal_driver_smoke_enabled, run_hal_driver_smoke, HalSmokeGuard, HAL_DRIVER_SMOKE_ENV,
};
use rusty_jack::passthrough::{select_effective_routing_target, PassthroughEngine};
use std::thread;
use std::time::Duration;

#[test]
#[ignore = "mutates HAL plugins and system audio; see module docs"]
fn native_driver_hal_smoke_install_virtual_output_and_passthrough_ring() {
    if !hal_driver_smoke_enabled() {
        eprintln!(
            "skipped: set {HAL_DRIVER_SMOKE_ENV}=1 to run this test (still requires --ignored)"
        );
        return;
    }

    let _guard = HalSmokeGuard::new();
    let hal = platform_hal().expect("CoreAudio HAL");
    let snapshot = run_hal_driver_smoke(hal.as_ref()).expect("HAL driver smoke cycle");

    eprintln!("HAL smoke snapshot: {snapshot:#?}");
    assert!(snapshot.system_rusty_jack_installed);
    assert!(snapshot.virtual_output_listed);
    assert!(snapshot.passthrough_ring_open_ok);
    assert!(snapshot.passthrough_plan_ready);

    let list = hal.list_outputs().expect("list outputs");
    let device = list
        .devices
        .iter()
        .find(|d| d.uid == RUSTY_JACK_VIRTUAL_OUTPUT_UID)
        .expect("virtual output device row");
    eprintln!(
        "virtual device: name={} uid={} transport={:?}",
        device.name, device.uid, device.transport
    );

    let config = rusty_jack::native_driver_hal_smoke::smoke_config_for_hdmi(&list.devices)
        .expect("smoke config");
    let effective =
        select_effective_routing_target(&config, &list.devices).expect("effective routing target");
    assert_eq!(
        effective.uid, RUSTY_JACK_VIRTUAL_OUTPUT_UID,
        "passthrough routing should target the virtual HAL device, not physical HDMI"
    );
}

#[test]
#[ignore = "mutates HAL plugins and system audio; see module docs"]
fn native_driver_hal_smoke_passthrough_engine_starts_on_physical_hdmi() {
    if !hal_driver_smoke_enabled() {
        eprintln!(
            "skipped: set {HAL_DRIVER_SMOKE_ENV}=1 to run this test (still requires --ignored)"
        );
        return;
    }

    let _guard = HalSmokeGuard::new();
    let hal = platform_hal().expect("CoreAudio HAL");
    run_hal_driver_smoke(hal.as_ref()).expect("HAL driver smoke cycle");

    let list = hal.list_outputs().expect("list outputs");
    let config = rusty_jack::native_driver_hal_smoke::smoke_config_for_hdmi(&list.devices)
        .expect("smoke config");
    let plan = rusty_jack::passthrough::plan_passthrough(&config, &list.devices)
        .expect("passthrough plan");
    eprintln!(
        "starting passthrough engine: {} -> {}",
        plan.virtual_output_uid, plan.physical_name
    );

    let engine = PassthroughEngine::start(&plan).expect("passthrough engine start");
    eprintln!("passthrough engine started; rendering silence+gained audio to HDMI");
    drop(engine);
    thread::sleep(Duration::from_secs(1));
}
