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

    #[error("speaker wake error: {0}")]
    Speaker(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
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
    fn test_coreaudio_error_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<RustyJackError>();
    }
}
