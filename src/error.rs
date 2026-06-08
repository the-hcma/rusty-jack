//! Error types for Rusty Jack.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum RustyJackError {
    #[error("configuration error: {0}")]
    Config(String),

    #[error("coreaudio error: {0}")]
    CoreAudio(String),

    #[error("launchd error: {0}")]
    Launchd(String),

    #[error("app launch error: {0}")]
    AppLaunch(String),

    #[error("speaker wake error: {0}")]
    Speaker(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl RustyJackError {
    /// Inner message without the `thiserror` category prefix (for structured logs).
    #[must_use]
    pub fn detail_message(&self) -> String {
        match self {
            Self::Config(msg)
            | Self::CoreAudio(msg)
            | Self::Launchd(msg)
            | Self::AppLaunch(msg)
            | Self::Speaker(msg) => msg.clone(),
            Self::Io(err) => err.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_error_display() {
        let err = RustyJackError::Config("missing preferred_device_uid".into());
        assert!(err.to_string().contains("configuration error"));
        assert!(err.to_string().contains("preferred_device_uid"));
    }

    #[test]
    fn test_io_error_from_std_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "plist");
        let err: RustyJackError = io_err.into();
        assert!(matches!(err, RustyJackError::Io(_)));
    }

    #[test]
    fn test_detail_message_strips_speaker_prefix() {
        let err = RustyJackError::Speaker(
            "url=http://192.168.1.1:54480/sony/system: connect failed".into(),
        );
        assert_eq!(
            err.detail_message(),
            "url=http://192.168.1.1:54480/sony/system: connect failed"
        );
        assert!(!err.detail_message().contains("speaker wake error"));
    }

    #[test]
    fn test_coreaudio_error_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<RustyJackError>();
    }
}
