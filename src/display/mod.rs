//! Online monitor names (macOS), matched to HDMI/DP audio device UIDs.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::monitor_name_for_audio_uid;

#[cfg(not(target_os = "macos"))]
#[must_use]
pub fn monitor_name_for_audio_uid(_uid: &str) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(target_os = "macos")]
    fn test_parse_apple_hda_uid() {
        use super::macos::{parse_apple_engine_uid, uid_vendor_to_profiler_format};
        let (v, p, s) = parse_apple_engine_uid(
            "AppleHDAEngineOutputDP:0,1,0,1,0:0:{AC10-A120-30594A4C}",
        )
        .unwrap();
        assert_eq!(v, "ac10");
        assert_eq!(p, "a120");
        assert_eq!(s, "30594a4c");
        assert_eq!(uid_vendor_to_profiler_format(&v), "10ac");
    }
}
