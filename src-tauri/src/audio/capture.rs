//! Microphone (or any input device) capture on WASAPI.
//!
//! [`start_capture`] opens the device in event-driven shared mode with its own mix format, on a
//! dedicated thread with COM initialized. Each buffer event the thread converts the samples to
//! interleaved `f32` (device rate and channel count, see [`CaptureStream::format`]), feeds the
//! level meter and pushes them into an [`AudioConsumer`] queue. The thread never blocks on locks
//! or the network and does not allocate after start; if the consumer falls behind, the oldest
//! audio is dropped (see [`ring`](super::ring)).

use super::format::StreamFormat;
use super::level::{InputLevel, SharedLevel};
use super::ring::AudioConsumer;
use super::stream::{StreamHandle, StreamStatus};
use crate::error::AppResult;

/// Capture options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureConfig {
    /// Most audio the queue keeps for a slow consumer before dropping the oldest, in ms.
    pub max_backlog_ms: u32,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            max_backlog_ms: 200,
        }
    }
}

/// A running capture stream. Stops on [`stop`](Self::stop) or drop.
pub struct CaptureStream {
    handle: StreamHandle,
    format: StreamFormat,
    level: SharedLevel,
}

impl CaptureStream {
    /// Format of the captured audio (the device's shared-mode mix format). Samples in the queue
    /// are `f32` interleaved at this rate and channel count.
    pub fn format(&self) -> StreamFormat {
        self.format
    }

    /// Latest input level (~20 readings per second).
    pub fn level(&self) -> InputLevel {
        self.level.get()
    }

    /// Whether the stream is still capturing (it ends on `stop()` or when the device fails).
    pub fn is_running(&self) -> bool {
        self.handle.is_running()
    }

    /// Why the stream stopped on its own, if it did.
    pub fn failure(&self) -> Option<&str> {
        self.handle.failure()
    }

    /// Thread-safe view of [`failure`](Self::failure).
    pub fn status(&self) -> StreamStatus {
        self.handle.status()
    }

    /// Lock-free handle to the input level, for readers on other threads.
    pub fn level_handle(&self) -> SharedLevel {
        self.level.clone()
    }

    /// Stops capturing and waits for the capture thread to end.
    pub fn stop(&mut self) {
        self.handle.stop();
    }
}

/// Starts capturing from the input device `device_id` (`None` = system default).
///
/// Blocking (opens the device): call it from a blocking thread. Returns the running stream and
/// the queue its samples arrive in.
pub fn start_capture(
    device_id: Option<&str>,
    config: CaptureConfig,
) -> AppResult<(CaptureStream, AudioConsumer)> {
    let level = SharedLevel::default();
    let (handle, (format, consumer)) =
        imp::spawn(device_id.map(str::to_owned), config, level.clone())?;
    Ok((
        CaptureStream {
            handle,
            format,
            level,
        },
        consumer,
    ))
}

/// Shared-mode mix format of the input device `device_id` (`None` = system default), without
/// starting a stream. Blocking.
pub fn input_device_format(device_id: Option<&str>) -> AppResult<StreamFormat> {
    imp::device_format(device_id)
}

#[cfg(windows)]
mod imp {
    use wasapi::{Direction, StreamMode};

    use super::CaptureConfig;
    use crate::audio::format::{StreamFormat, bytes_to_f32};
    use crate::audio::level::{LEVEL_WINDOW_MS, LevelMeter, SharedLevel};
    use crate::audio::ring::{AudioConsumer, audio_ring};
    use crate::audio::stream::win::{
        ComGuard, device_mix_format, map_error, mix_format, open_device,
    };
    use crate::audio::stream::{EVENT_WAIT_MS, StreamControl, StreamHandle, spawn_stream};
    use crate::error::AppResult;

    const OPEN: &str = "could not open the microphone";
    const RUN: &str = "the microphone stopped";

    pub(super) fn device_format(device_id: Option<&str>) -> AppResult<StreamFormat> {
        device_mix_format(device_id, Direction::Capture, OPEN)
    }

    pub(super) fn spawn(
        device_id: Option<String>,
        config: CaptureConfig,
        level: SharedLevel,
    ) -> AppResult<(StreamHandle, (StreamFormat, AudioConsumer))> {
        spawn_stream("microphone capture", move |control, started| {
            let _com = match ComGuard::new() {
                Ok(com) => com,
                Err(error) => {
                    let _ = started.send(Err(error));
                    return;
                }
            };
            // Every COM object lives inside `run`, so it is released before `_com` drops.
            run(device_id.as_deref(), config, level, &control, |result| {
                let _ = started.send(result);
            });
        })
    }

    fn run(
        device_id: Option<&str>,
        config: CaptureConfig,
        level: SharedLevel,
        control: &StreamControl,
        started: impl FnOnce(AppResult<(StreamFormat, AudioConsumer)>),
    ) {
        let open = || -> AppResult<_> {
            let device = open_device(device_id, Direction::Capture, OPEN)?;
            let mut client = device.get_iaudioclient().map_err(|e| map_error(OPEN, e))?;
            let (wave, format) = mix_format(&client, OPEN)?;
            let (default_period, _) = client.get_device_period().map_err(|e| map_error(OPEN, e))?;
            let mode = StreamMode::EventsShared {
                autoconvert: false,
                buffer_duration_hns: default_period,
            };
            client
                .initialize_client(&wave, Direction::Capture, mode, None)
                .map_err(|e| map_error(OPEN, e))?;
            let event = client
                .set_get_eventhandle()
                .map_err(|e| map_error(OPEN, e))?;
            let capture = client
                .get_audiocaptureclient()
                .map_err(|e| map_error(OPEN, e))?;
            let buffer_frames = client.get_buffer_size().map_err(|e| map_error(OPEN, e))? as usize;
            client.start_stream().map_err(|e| map_error(OPEN, e))?;
            Ok((client, event, capture, format, buffer_frames))
        };
        let (client, event, capture, format, buffer_frames) = match open() {
            Ok(opened) => opened,
            Err(error) => {
                started(Err(error));
                return;
            }
        };

        let channels = usize::from(format.channels);
        let (mut producer, consumer) =
            audio_ring(format.channels, format.frames_for_ms(config.max_backlog_ms));
        let mut meter = LevelMeter::new(level, format.frames_for_ms(LEVEL_WINDOW_MS) * channels);
        // A packet is never larger than the device buffer, so these never grow.
        let mut bytes = vec![0_u8; buffer_frames * format.frame_bytes()];
        let mut samples = vec![0.0_f32; buffer_frames * channels];
        tracing::info!(%format, buffer_frames, "microphone capture started");
        started(Ok((format, consumer)));

        let result = (|| -> Result<(), wasapi::WasapiError> {
            while !control.should_stop() {
                // A timeout is not an error: fall through so a dead device surfaces below.
                let _ = event.wait_for_event(EVENT_WAIT_MS);
                while capture.get_next_packet_size()?.unwrap_or(0) > 0 {
                    let (frames, _info) = capture.read_from_device(&mut bytes)?;
                    let len = frames as usize * format.frame_bytes();
                    let count = bytes_to_f32(format.sample_format, &bytes[..len], &mut samples);
                    meter.process(&samples[..count]);
                    if !producer.is_abandoned() {
                        producer.push(&samples[..count]);
                    }
                }
            }
            Ok(())
        })();

        meter.reset();
        if let Err(error) = result {
            control.fail(&map_error(RUN, error));
        }
        if let Err(error) = client.stop_stream() {
            tracing::debug!(%error, "stopping the capture stream failed");
        }
        tracing::info!("microphone capture stopped");
    }
}

#[cfg(not(windows))]
mod imp {
    use super::CaptureConfig;
    use crate::audio::format::StreamFormat;
    use crate::audio::level::SharedLevel;
    use crate::audio::ring::AudioConsumer;
    use crate::audio::stream::StreamHandle;
    use crate::error::{AppError, AppResult};

    fn unsupported() -> AppError {
        AppError::Audio("audio capture is only supported on Windows".into())
    }

    pub(super) fn device_format(_: Option<&str>) -> AppResult<StreamFormat> {
        Err(unsupported())
    }

    pub(super) fn spawn(
        _: Option<String>,
        _: CaptureConfig,
        _: SharedLevel,
    ) -> AppResult<(StreamHandle, (StreamFormat, AudioConsumer))> {
        Err(unsupported())
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Captures 2 s from the default microphone. Speak or tap the mic to see the level move.
    /// Run with `cargo test --manifest-path src-tauri/Cargo.toml capture_default_microphone -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs a real microphone"]
    fn capture_default_microphone() {
        let (mut stream, mut consumer) = start_capture(None, CaptureConfig::default()).unwrap();
        let format = stream.format();
        println!("microphone format: {format}");

        let mut buffer = vec![0.0_f32; format.frames_for_ms(100) * usize::from(format.channels)];
        let mut samples = 0;
        let mut max_peak = 0.0_f32;
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(50));
            samples += consumer.pop(&mut buffer);
            let level = stream.level();
            max_peak = max_peak.max(level.peak);
            println!("rms {:.4}  peak {:.4}", level.rms, level.peak);
        }
        stream.stop();

        let frames = samples / usize::from(format.channels);
        let seconds = frames as f64 / f64::from(format.sample_rate);
        println!(
            "captured {frames} frames ({seconds:.2} s), max peak {max_peak:.4}, dropped {}",
            consumer.dropped_frames()
        );
        assert!(stream.failure().is_none(), "{:?}", stream.failure());
        assert!(!stream.is_running());
        assert!(consumer.is_abandoned());
        assert!(seconds > 1.5, "expected ~2 s of audio, got {seconds:.2} s");
    }
}
