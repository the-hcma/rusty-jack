//! Persist last-known volumes for non-preferred outputs.

use crate::coreaudio::AudioHal;
use crate::output_device::OutputDevice;
use crate::RustyJackError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

const ENV_STATE_DIR: &str = "RUSTY_JACK_STATE_DIR";

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct VolumeMemory {
    #[serde(default)]
    devices: BTreeMap<String, u8>,
}

#[must_use]
fn memory_path() -> Option<PathBuf> {
    Some(state_dir()?.join("device-volumes.json"))
}

fn state_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var(ENV_STATE_DIR) {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir));
        }
    }
    if cfg!(test) {
        return None;
    }
    crate::config::default_config_path().and_then(|path| path.parent().map(PathBuf::from))
}

fn load_memory() -> VolumeMemory {
    let Some(path) = memory_path() else {
        return VolumeMemory::default();
    };
    let Ok(raw) = std::fs::read_to_string(path) else {
        return VolumeMemory::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn save_memory(memory: &VolumeMemory) -> Result<(), RustyJackError> {
    let Some(path) = memory_path() else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(RustyJackError::Io)?;
    }
    let raw = serde_json::to_string_pretty(memory)
        .map_err(|err| RustyJackError::Config(format!("volume memory JSON: {err}")))?;
    std::fs::write(path, format!("{raw}\n")).map_err(RustyJackError::Io)
}

/// Read a remembered volume for a device UID.
#[must_use]
pub fn remembered_volume(uid: &str) -> Option<u8> {
    load_memory().devices.get(uid).copied()
}

/// Remember the current active non-preferred output volume before switching away.
pub fn remember_active_non_preferred(
    hal: &dyn AudioHal,
    devices: &[OutputDevice],
    preferred_uid: Option<&str>,
    target_uid: &str,
) -> Result<(), RustyJackError> {
    let Some(active) = devices.iter().find(|device| device.is_active) else {
        return Ok(());
    };
    if active.uid == target_uid || preferred_uid == Some(active.uid.as_str()) {
        return Ok(());
    }
    let Some(volume) = hal.output_volume_percent(&active.uid) else {
        return Ok(());
    };

    let mut memory = load_memory();
    memory.devices.insert(active.uid.clone(), volume.min(100));
    save_memory(&memory)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coreaudio::mock::MockHal;
    use crate::output_device::OutputDevice;
    use crate::transport::TransportKind;
    use std::sync::{Mutex, MutexGuard};

    fn with_state_dir<T>(f: impl FnOnce() -> T) -> T {
        static LOCK: Mutex<()> = Mutex::new(());
        let _guard: MutexGuard<'_, ()> = LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var(ENV_STATE_DIR, dir.path());
        let result = f();
        std::env::remove_var(ENV_STATE_DIR);
        result
    }

    #[test]
    fn test_memory_round_trip_in_struct() {
        let mut memory = VolumeMemory::default();
        memory.devices.insert("builtin".into(), 33);
        let raw = serde_json::to_string(&memory).unwrap();
        let parsed: VolumeMemory = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed.devices.get("builtin"), Some(&33));
    }

    #[test]
    fn test_remembered_volume_persists_to_state_dir() {
        with_state_dir(|| {
            let devices = vec![OutputDevice {
                id: 1,
                uid: "active-uid".into(),
                name: "Active".into(),
                transport: TransportKind::BuiltIn,
                is_alive: true,
                is_default: true,
                is_active: true,
            }];
            let hal = MockHal::new(devices).with_output_volume(42);
            remember_active_non_preferred(
                &hal,
                &hal.list_outputs().unwrap().devices,
                Some("preferred"),
                "target",
            )
            .unwrap();
            assert_eq!(remembered_volume("active-uid"), Some(42));
        });
    }
}
