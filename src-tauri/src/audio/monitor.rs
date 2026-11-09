//! Microphone test: capture an input, report its level, and optionally play it back ("listen to
//! myself") on an output device.
//!
//! Playing back needs the input and output to share sample rate and channel count, since there
//! is no resampling yet; otherwise starting fails with a "formats differ, needs resampling"
//! error.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use super::capture::{CaptureConfig, CaptureStream, start_capture};
use super::level::{InputLevel, LEVEL_WINDOW_MS};
use super::playback::{
    PlaybackConfig, PlaybackStream, check_formats, output_device_format, start_playback,
};
use crate::error::{AppError, AppResult};

/// What the monitor reports while it runs.
#[derive(Debug, Clone, PartialEq)]
pub enum MonitorEvent {
    /// The latest input level, ~20 times per second.
    Level(InputLevel),
    /// The monitor stopped on its own (for example the device was unplugged).
    Stopped { message: String },
}

/// A running microphone test. Stops on [`stop`](Self::stop) or drop.
pub struct Monitor {
    // Field order is drop order: the reporter goes first, then playback, then capture.
    reporter: Option<Reporter>,
    playback: Option<PlaybackStream>,
    capture: CaptureStream,
}

impl Monitor {
    /// Starts capturing `input_id` and, when `output_id` is `Some`, plays it back there
    /// (`Some(None)` = default output). `None` / `Some(None)` ids mean the system default.
    ///
    /// `on_event` is called from a background thread with the level (~20 Hz) and if the
    /// monitor stops on its own. Blocking (opens devices): call it from a blocking thread.
    pub fn start(
        input_id: Option<&str>,
        output_id: Option<Option<&str>>,
        on_event: impl Fn(MonitorEvent) + Send + 'static,
    ) -> AppResult<Self> {
        if let Some(output_id) = output_id {
            // Check before opening the microphone so the error comes without a click.
            let input = super::capture::input_device_format(input_id)?;
            let output = output_device_format(output_id)?;
            check_formats(&input, &output)?;
        }

        let (capture, consumer) = start_capture(input_id, CaptureConfig::default())?;
        let playback = match output_id {
            Some(output_id) => Some(start_playback(
                output_id,
                consumer,
                capture.format(),
                PlaybackConfig::default(),
            )?),
            None => None,
        };
        tracing::info!(format = %capture.format(), listening = playback.is_some(), "microphone test started");

        let mut monitor = Self {
            reporter: None,
            playback,
            capture,
        };
        monitor.reporter = Some(Reporter::spawn(&monitor, on_event)?);
        Ok(monitor)
    }

    /// Stops the reporter, playback and capture, waiting for their threads.
    pub fn stop(&mut self) {
        if let Some(mut reporter) = self.reporter.take() {
            reporter.stop();
        }
        if let Some(playback) = self.playback.as_mut() {
            playback.stop();
        }
        self.capture.stop();
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Thread that polls the level and the streams' health and reports them through `on_event`.
struct Reporter {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

/// What the reporter thread reads from the streams: lock-free level and the failure slots.
type HealthCheck = Box<dyn Fn() -> Option<String> + Send>;

impl Reporter {
    fn spawn(
        monitor: &Monitor,
        on_event: impl Fn(MonitorEvent) + Send + 'static,
    ) -> AppResult<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let level = monitor.capture.level_handle();
        let health = monitor.health_check();
        let thread = std::thread::Builder::new()
            .name("livedub-monitor-level".into())
            .spawn(move || {
                let interval = Duration::from_millis(u64::from(LEVEL_WINDOW_MS));
                while !thread_stop.load(Ordering::Relaxed) {
                    if let Some(message) = health() {
                        on_event(MonitorEvent::Level(InputLevel::default()));
                        on_event(MonitorEvent::Stopped { message });
                        return;
                    }
                    on_event(MonitorEvent::Level(level.get()));
                    std::thread::sleep(interval);
                }
                on_event(MonitorEvent::Level(InputLevel::default()));
            })
            .map_err(|error| {
                AppError::Internal(format!("could not start the level thread: {error}"))
            })?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("monitor level thread panicked");
        }
    }
}

impl Monitor {
    /// A closure that says why a stream died, if one did. Reads only atomics and `OnceLock`s.
    fn health_check(&self) -> HealthCheck {
        let capture = self.capture.status();
        let playback = self.playback.as_ref().map(PlaybackStream::status);
        Box::new(move || {
            capture
                .failure()
                .or_else(|| playback.as_ref().and_then(|p| p.failure()))
                .map(str::to_owned)
        })
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::Instant;

    /// Plays the default microphone on the default output for 2 s (wear headphones!).
    /// Run with `cargo test --manifest-path src-tauri/Cargo.toml monitor_default_devices -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs a real microphone and output device"]
    fn monitor_default_devices() {
        // Show the streams' formats, buffer sizes and latency.
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        let started = Instant::now();
        let result = Monitor::start(None, Some(None), move |event| {
            sink.lock().unwrap().push(event);
        });
        let mut monitor = match result {
            Ok(monitor) => monitor,
            Err(error) => {
                println!("monitor did not start: {error}");
                return;
            }
        };
        println!("monitor started in {:?}", started.elapsed());
        std::thread::sleep(Duration::from_secs(2));
        monitor.stop();

        let events = events.lock().unwrap();
        let levels = events
            .iter()
            .filter(|e| matches!(e, MonitorEvent::Level(_)))
            .count();
        println!("{levels} level events in 2 s");
        assert!(levels >= 20, "expected ~40 level events, got {levels}");
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, MonitorEvent::Stopped { .. }))
        );
    }
}
