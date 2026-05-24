//! Resolve monitor product names via `system_profiler SPDisplaysDataType -json`.

use serde_json::Value;
use std::process::Command;
use std::sync::OnceLock;

#[derive(Debug, Clone)]
struct MonitorEntry {
    name: String,
    vendor_id: String,
    product_id: String,
    serial_number: Option<String>,
}

static MONITORS: OnceLock<Vec<MonitorEntry>> = OnceLock::new();

fn normalize_hex(s: &str) -> String {
    s.trim().to_ascii_lowercase()
}

/// Parse `{VVVV-PPPP-SSSSSSSS}` suffix on Apple HDA engine UIDs.
pub(crate) fn parse_apple_engine_uid(uid: &str) -> Option<(String, String, String)> {
    let tail = uid.rsplit(':').next()?;
    let inner = tail.strip_prefix('{')?.strip_suffix('}')?;
    let mut parts = inner.split('-');
    let vendor = normalize_hex(parts.next()?);
    let product = normalize_hex(parts.next()?);
    let serial = normalize_hex(parts.next()?);
    if vendor.is_empty() || product.is_empty() || serial.is_empty() {
        return None;
    }
    Some((vendor, product, serial))
}

/// Apple UID uses big-endian byte pairs (e.g. `AC10`); system_profiler uses `10ac`.
pub(crate) fn uid_vendor_to_profiler_format(vendor_uid: &str) -> String {
    let v = normalize_hex(vendor_uid);
    if v.len() == 4 {
        format!("{}{}", &v[2..4], &v[0..2])
    } else {
        v
    }
}

fn load_monitors() -> &'static [MonitorEntry] {
    MONITORS.get_or_init(|| {
        let output = Command::new("system_profiler")
            .args(["SPDisplaysDataType", "-json"])
            .output();

        match output {
            Ok(out) if out.status.success() => parse_system_profiler_json(&out.stdout),
            _ => Vec::new(),
        }
    })
}

fn parse_system_profiler_json(bytes: &[u8]) -> Vec<MonitorEntry> {
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return Vec::new();
    };

    let Some(arr) = value.get("SPDisplaysDataType").and_then(Value::as_array) else {
        return Vec::new();
    };

    let mut monitors = Vec::new();

    for gpu in arr {
        let Some(ndrvs) = gpu.get("spdisplays_ndrvs").and_then(Value::as_array) else {
            continue;
        };
        for ndrv in ndrvs {
            let Some(name) = ndrv.get("_name").and_then(Value::as_str) else {
                continue;
            };
            let Some(vendor) = ndrv
                .get("_spdisplays_display-vendor-id")
                .and_then(Value::as_str)
            else {
                continue;
            };
            let Some(product) = ndrv
                .get("_spdisplays_display-product-id")
                .and_then(Value::as_str)
            else {
                continue;
            };
            let serial = ndrv
                .get("_spdisplays_display-serial-number")
                .and_then(Value::as_str)
                .map(normalize_hex);

            monitors.push(MonitorEntry {
                name: name.to_string(),
                vendor_id: normalize_hex(vendor),
                product_id: normalize_hex(product),
                serial_number: serial,
            });
        }
    }

    monitors
}

/// Match an HDMI/DP audio device UID to an online monitor name, if known.
#[must_use]
pub fn monitor_name_for_audio_uid(uid: &str) -> Option<String> {
    let (vendor_uid, product_uid, serial_uid) = parse_apple_engine_uid(uid)?;
    let vendor_sp = uid_vendor_to_profiler_format(&vendor_uid);

    for monitor in load_monitors() {
        if monitor.vendor_id != vendor_sp || monitor.product_id != product_uid {
            continue;
        }
        if let Some(ref serial) = monitor.serial_number {
            if *serial == serial_uid {
                return Some(monitor.name.clone());
            }
        } else {
            return Some(monitor.name.clone());
        }
    }

    // Fallback: vendor + product only (single monitor on that port class)
    load_monitors()
        .iter()
        .find(|m| m.vendor_id == vendor_sp && m.product_id == product_uid)
        .map(|m| m.name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_system_profiler_sample() {
        let sample = include_str!("../../tests/fixtures/spdisplays_sample.json");
        let monitors = parse_system_profiler_json(sample.as_bytes());
        assert_eq!(monitors.len(), 2);
        assert_eq!(monitors[0].name, "DELL U3219Q");
        assert_eq!(monitors[0].vendor_id, "10ac");
        assert_eq!(monitors[0].product_id, "a120");
    }

    #[test]
    fn test_match_monitor_from_sample() {
        let uid = "AppleHDAEngineOutputDP:0,1,0,1,0:0:{AC10-A120-30594A4C}";
        let (v, p, s) = parse_apple_engine_uid(uid).unwrap();
        assert_eq!(uid_vendor_to_profiler_format(&v), "10ac");
        assert_eq!(p, "a120");
        assert_eq!(s, "30594a4c");
    }
}
