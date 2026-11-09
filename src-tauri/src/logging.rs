//! Logging setup.
//!
//! Logs go to stdout and to a daily-rotating file in the app's log directory, keeping the last
//! [`MAX_LOG_FILES`] files. The level defaults to `info` (`debug` in debug builds) and can be
//! overridden with the `RUST_LOG` environment variable.
//!
//! **Never log secrets.** The OpenAI API key, and anything that contains it (such as request
//! headers), must never reach a log line at any level.

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;

use crate::error::{AppError, AppResult};

/// Number of daily log files kept in the log directory.
pub const MAX_LOG_FILES: usize = 7;

const LOG_FILE_PREFIX: &str = "livedub-voice";
const LOG_FILE_SUFFIX: &str = "log";

/// Keeps the background log writer alive; dropping it flushes and stops file logging.
///
/// Store it in Tauri's managed state so it lives as long as the app.
pub struct LogGuard {
    _guard: WorkerGuard,
}

/// Filter used when `RUST_LOG` is not set.
fn default_directive() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "info"
    }
}

/// Installs the global `tracing` subscriber, writing to stdout and to a file in `log_dir`.
pub fn init(log_dir: &Path) -> AppResult<LogGuard> {
    std::fs::create_dir_all(log_dir)?;

    let file_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(LOG_FILE_PREFIX)
        .filename_suffix(LOG_FILE_SUFFIX)
        .max_log_files(MAX_LOG_FILES)
        .build(log_dir)
        .map_err(|error| AppError::Internal(format!("could not create log file: {error}")))?;
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);

    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_directive()));

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(std::io::stdout))
        .with(fmt::layer().with_writer(file_writer).with_ansi(false))
        .try_init()
        .map_err(|error| AppError::Internal(format!("could not install logger: {error}")))?;

    Ok(LogGuard { _guard: guard })
}
