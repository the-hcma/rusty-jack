//! Structured logging (my-tracks-style pipe format).

use crate::config::LoggingConfig as FileLoggingConfig;
use crate::RustyJackError;
use file_rotate::compression::Compression;
use file_rotate::suffix::AppendCount;
use file_rotate::{ContentLimit, FileRotate};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Once, OnceLock};
use tracing::Level;
use tracing_subscriber::fmt::format::{FormatEvent, FormatFields, Writer};
use tracing_subscriber::layer::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Registry};

const ENV_LOG_FILE: &str = "RUSTY_JACK_LOG_FILE";
const ENV_LOG_LEVEL: &str = "RUSTY_JACK_LOG_LEVEL";
const DEFAULT_LOG_FILE: &str = "~/Library/Logs/rusty-jack.log";
const DEFAULT_LOG_LEVEL: &str = "info";
const LOG_ROTATE_BYTES: usize = 10 * 1024 * 1024;
const LOG_ROTATE_KEEP: usize = 5;

static INIT: Once = Once::new();
static LOG_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

/// Effective logging settings for the daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonLoggingOptions {
    pub level: String,
    pub file: PathBuf,
    /// Mirror structured logs to stderr (always on for interactive daemon runs).
    pub console: bool,
}

impl Default for DaemonLoggingOptions {
    fn default() -> Self {
        Self {
            level: DEFAULT_LOG_LEVEL.into(),
            file: PathBuf::from(DEFAULT_LOG_FILE),
            console: true,
        }
    }
}

impl From<&FileLoggingConfig> for DaemonLoggingOptions {
    fn from(value: &FileLoggingConfig) -> Self {
        Self {
            level: value.level.clone(),
            file: PathBuf::from(value.file.clone()),
            console: true,
        }
    }
}

/// Initialize stderr-only logging for foreground CLI commands.
pub fn init_cli() {
    INIT.call_once(|| {
        let filter = build_filter(DEFAULT_LOG_LEVEL);
        Registry::default()
            .with(
                tracing_subscriber::fmt::layer()
                    .event_format(MyTracksFormat)
                    .with_ansi(false)
                    .with_writer(io::stderr)
                    .with_filter(filter),
            )
            .init();
    });
}

/// Initialize rotated file logging for the background daemon.
///
/// # Errors
///
/// Returns an error when the log path cannot be created or the subscriber fails to init.
pub fn init_daemon(options: &DaemonLoggingOptions) -> Result<(), RustyJackError> {
    let mut init_error = None;
    INIT.call_once(|| {
        if let Err(err) = init_daemon_inner(options) {
            init_error = Some(err);
        }
    });
    init_error.map_or(Ok(()), Err)
}

fn init_daemon_inner(options: &DaemonLoggingOptions) -> Result<(), RustyJackError> {
    let level = resolve_level(&options.level);
    let file_path = resolve_log_file_path(&options.file)?;
    ensure_log_parent(&file_path)?;

    let rotate = FileRotate::new(
        &file_path,
        AppendCount::new(LOG_ROTATE_KEEP),
        ContentLimit::Bytes(LOG_ROTATE_BYTES),
        Compression::None,
        None,
    );
    let (file_writer, guard) = tracing_appender::non_blocking(rotate);
    let _ = LOG_GUARD.set(guard);

    let filter = build_filter(&level);
    let file_layer = tracing_subscriber::fmt::layer()
        .event_format(MyTracksFormat)
        .with_ansi(false)
        .with_writer(file_writer)
        .with_filter(filter.clone());

    let registry = Registry::default().with(file_layer);

    if options.console {
        registry
            .with(
                tracing_subscriber::fmt::layer()
                    .event_format(MyTracksFormat)
                    .with_ansi(false)
                    .with_writer(io::stderr)
                    .with_filter(filter),
            )
            .init();
    } else {
        registry.init();
    }

    let timestamp_mode = if use_utc_timestamps() {
        "UTC"
    } else {
        "local time"
    };
    tracing::debug!(
        target: "logging",
        "Daemon logging initialized (level={level}, timestamps={timestamp_mode}, file={})",
        file_path.display()
    );
    Ok(())
}

fn build_filter(default_level: &str) -> EnvFilter {
    EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("rusty_jack={default_level}")))
}

struct MyTracksFormat;

impl<S, N> FormatEvent<S, N> for MyTracksFormat
where
    S: tracing::Subscriber + for<'lookup> tracing_subscriber::registry::LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn format_event(
        &self,
        ctx: &tracing_subscriber::fmt::FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> fmt::Result {
        let meta = event.metadata();
        write!(
            writer,
            "{} | {} | {} | ",
            chrono_timestamp(),
            pad_level(meta.level()),
            pad_module(meta.target())
        )?;
        ctx.format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

fn chrono_timestamp() -> String {
    if use_utc_timestamps() {
        chrono::Utc::now().format("%Y%m%d-%H:%M:%S%.3f").to_string()
    } else {
        chrono::Local::now()
            .format("%Y%m%d-%H:%M:%S%.3f")
            .to_string()
    }
}

fn pad_level(level: &Level) -> String {
    format!("{:<8}", level.as_str().to_uppercase())
}

fn pad_module(target: &str) -> String {
    let name = target.rsplit("::").next().unwrap_or(target);
    if name.len() <= 12 {
        format!("{name:<12}")
    } else {
        name.chars().take(12).collect()
    }
}

fn resolve_level(configured: &str) -> String {
    std::env::var(ENV_LOG_LEVEL)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| configured.trim().to_lowercase())
}

pub fn resolve_log_file_path(path: &Path) -> Result<PathBuf, RustyJackError> {
    if let Ok(from_env) = std::env::var(ENV_LOG_FILE) {
        if !from_env.trim().is_empty() {
            return Ok(expand_tilde(&from_env));
        }
    }
    Ok(expand_tilde(path.to_string_lossy().as_ref()))
}

pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

fn ensure_log_parent(path: &Path) -> Result<(), RustyJackError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(RustyJackError::Io)?;
    }
    Ok(())
}

fn use_utc_timestamps() -> bool {
    matches!(
        std::env::var("LOG_UTC").ok().as_deref(),
        Some("1") | Some("true") | Some("yes")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_tilde() {
        std::env::set_var("HOME", "/Users/example");
        assert_eq!(
            expand_tilde("~/Library/Logs/rusty-jack.log"),
            PathBuf::from("/Users/example/Library/Logs/rusty-jack.log")
        );
    }

    #[test]
    fn test_pad_module_truncates_long_targets() {
        assert_eq!(pad_module("rusty_jack::daemon"), "daemon      ");
        assert_eq!(pad_module("verylongmodulename"), "verylongmodu");
    }

    #[test]
    fn test_pad_level() {
        assert_eq!(pad_level(&Level::INFO), "INFO    ");
        assert_eq!(pad_level(&Level::WARN), "WARN    ");
    }
}
