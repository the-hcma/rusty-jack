//! Software gain helpers for the native passthrough pipeline.

/// Convert a 0–100 percent level to a linear gain scalar.
#[must_use]
pub fn percent_to_scalar(percent: u8) -> f32 {
    (percent.min(100) as f32) / 100.0
}

/// Apply linear gain to interleaved stereo `f32` samples (L, R, L, R, …).
pub fn apply_stereo_interleaved_gain(samples: &mut [f32], gain: f32) {
    let gain = gain.clamp(0.0, 1.0);
    if gain == 1.0 {
        return;
    }
    for sample in samples {
        *sample *= gain;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_percent_to_scalar() {
        assert!((percent_to_scalar(0) - 0.0).abs() < f32::EPSILON);
        assert!((percent_to_scalar(50) - 0.5).abs() < f32::EPSILON);
        assert!((percent_to_scalar(100) - 1.0).abs() < f32::EPSILON);
        assert!((percent_to_scalar(255) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_apply_stereo_interleaved_gain() {
        let mut samples = [1.0, 0.5, -1.0, 0.25];
        apply_stereo_interleaved_gain(&mut samples, 0.5);
        assert_eq!(samples, [0.5, 0.25, -0.5, 0.125]);
    }

    #[test]
    fn test_apply_stereo_interleaved_gain_unity_is_noop() {
        let mut samples = [0.25, -0.75];
        apply_stereo_interleaved_gain(&mut samples, 1.0);
        assert_eq!(samples, [0.25, -0.75]);
    }
}
