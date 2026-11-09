//! Audio playback (render) on WASAPI.
//!
//! [`start_playback`] opens an output device in event-driven shared mode with its own mix format,
//! on a dedicated thread with COM initialized, and plays interleaved `f32` samples read from an
//! [`AudioConsumer`] queue. The thread never blocks: when the queue runs dry it writes silence
//! and waits for a small prebuffer to fill again before resuming, and it drops the oldest queued
//! audio beyond `max_queue_ms` so latency cannot build up.
//!
//! The samples must already be at the device's rate and channel count: no conversion happens
//! here. [`output_device_format`] tells the caller what the device wants.

use super::format::StreamFormat;
use super::ring::AudioConsumer;
use super::stream::{StreamHandle, StreamStatus};
use crate::error::{AppError, AppResult};

/// Playback options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackConfig {
    /// Audio to queue before playing (at start and after the queue ran dry), in ms. Absorbs
    /// jitter in the producer; adds the same amount of latency.
    pub prebuffer_ms: u32,
    /// Most audio kept queued; older audio is dropped to keep latency bounded, in ms.
    pub max_queue_ms: u32,
}

impl Default for PlaybackConfig {
    fn default() -> Self {
        Self {
            prebuffer_ms: 60,
            max_queue_ms: 150,
        }
    }
}

/// A running playback stream. Stops on [`stop`](Self::stop) or drop.
pub struct PlaybackStream {
    handle: StreamHandle,
    format: StreamFormat,
}

impl PlaybackStream {
    /// The device's mix format the stream plays in.
    pub fn format(&self) -> StreamFormat {
        self.format
    }

    /// Whether the stream is still playing (it ends on `stop()` or when the device fails).
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

    /// Stops playback and waits for the playback thread to end.
    pub fn stop(&mut self) {
        self.handle.stop();
    }
}

/// Checks that `f32` samples in `source` format can be played on a device in `device` format.
pub fn check_formats(source: &StreamFormat, device: &StreamFormat) -> AppResult<()> {
    if source.matches_samples_of(device) {
        Ok(())
    } else {
        Err(AppError::Audio(format!(
            "formats differ, needs resampling: the audio is {} Hz, {} ch but the output device plays {} Hz, {} ch",
            source.sample_rate, source.channels, device.sample_rate, device.channels
        )))
    }
}

/// Starts playing the samples of `consumer` (in `source` format) on the output device
/// `device_id` (`None` = system default).
///
/// Fails with a "formats differ, needs resampling" [`AppError::Audio`] when `source` does not
/// have the device's rate and channel count. Blocking (opens the device): call it from a
/// blocking thread.
pub fn start_playback(
    device_id: Option<&str>,
    consumer: AudioConsumer,
    source: StreamFormat,
    config: PlaybackConfig,
) -> AppResult<PlaybackStream> {
    let (handle, format) = imp::spawn(device_id.map(str::to_owned), consumer, source, config)?;
    Ok(PlaybackStream { handle, format })
}

/// Shared-mode mix format of the output device `device_id` (`None` = system default), without
/// starting a stream. Blocking.
pub fn output_device_format(device_id: Option<&str>) -> AppResult<StreamFormat> {
    imp::device_format(device_id)
}

/// What the render loop does with one device period.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlayState {
    /// Waiting for `prebuffer` frames before playing; writes silence meanwhile.
    Prebuffering,
    Playing,
}

/// Fills `out` (whole frames) from `consumer`, padding with silence, and updates the state.
/// Returns whether the queue ran dry while playing (an underrun).
fn fill_period(
    consumer: &mut AudioConsumer,
    out: &mut [f32],
    state: &mut PlayState,
    prebuffer_frames: usize,
    max_queue_frames: usize,
) -> bool {
    consumer.drop_oldest_beyond(max_queue_frames);
    if *state == PlayState::Prebuffering && consumer.available_frames() >= prebuffer_frames {
        *state = PlayState::Playing;
    }
    let read = match state {
        PlayState::Playing => consumer.pop(out),
        PlayState::Prebuffering => 0,
    };
    out[read..].fill(0.0);
    let underrun = *state == PlayState::Playing && read < out.len();
    if underrun {
        *state = PlayState::Prebuffering;
    }
    underrun
}

#[cfg(windows)]
mod imp {
    use wasapi::{Direction, StreamMode};

    use super::{PlayState, PlaybackConfig, check_formats, fill_period};
    use crate::audio::format::{StreamFormat, f32_to_bytes};
    use crate::audio::ring::AudioConsumer;
    use crate::audio::stream::win::{
        ComGuard, device_mix_format, map_error, mix_format, open_device,
    };
    use crate::audio::stream::{EVENT_WAIT_MS, StreamControl, StreamHandle, spawn_stream};
    use crate::error::AppResult;

    const OPEN: &str = "could not open the output device";
    const RUN: &str = "the output device stopped";

    pub(super) fn device_format(device_id: Option<&str>) -> AppResult<StreamFormat> {
        device_mix_format(device_id, Direction::Render, OPEN)
    }

    pub(super) fn spawn(
        device_id: Option<String>,
        consumer: AudioConsumer,
        source: StreamFormat,
        config: PlaybackConfig,
    ) -> AppResult<(StreamHandle, StreamFormat)> {
        let (handle, format) = spawn_stream("audio playback", move |control, started| {
            let _com = match ComGuard::new() {
                Ok(com) => com,
                Err(error) => {
                    let _ = started.send(Err(error));
                    return;
                }
            };
            // Every COM object lives inside `run`, so it is released before `_com` drops.
            run(
                device_id.as_deref(),
                consumer,
                source,
                config,
                &control,
                |result| {
                    let _ = started.send(result);
                },
            );
        })?;
        Ok((handle, format))
    }

    fn run(
        device_id: Option<&str>,
        mut consumer: AudioConsumer,
        source: StreamFormat,
        config: PlaybackConfig,
        control: &StreamControl,
        started: impl FnOnce(AppResult<StreamFormat>),
    ) {
        let open = || -> AppResult<_> {
            let device = open_device(device_id, Direction::Render, OPEN)?;
            let mut client = device.get_iaudioclient().map_err(|e| map_error(OPEN, e))?;
            let (wave, format) = mix_format(&client, OPEN)?;
            check_formats(&source, &format)?;
            let (default_period, _) = client.get_device_period().map_err(|e| map_error(OPEN, e))?;
            let mode = StreamMode::EventsShared {
                autoconvert: false,
                buffer_duration_hns: default_period,
            };
            client
                .initialize_client(&wave, Direction::Render, mode, None)
                .map_err(|e| map_error(OPEN, e))?;
            let event = client
                .set_get_eventhandle()
                .map_err(|e| map_error(OPEN, e))?;
            let render = client
                .get_audiorenderclient()
                .map_err(|e| map_error(OPEN, e))?;
            let buffer_frames = client.get_buffer_size().map_err(|e| map_error(OPEN, e))? as usize;
            Ok((client, event, render, format, buffer_frames))
        };
        let (client, event, render, format, buffer_frames) = match open() {
            Ok(opened) => opened,
            Err(error) => {
                started(Err(error));
                return;
            }
        };

        let channels = usize::from(format.channels);
        let prebuffer_frames = format.frames_for_ms(config.prebuffer_ms);
        let max_queue_frames = format
            .frames_for_ms(config.max_queue_ms)
            .max(prebuffer_frames);
        // The device never asks for more than its buffer, so these never grow.
        let mut samples = vec![0.0_f32; buffer_frames * channels];
        let mut bytes = vec![0_u8; buffer_frames * format.frame_bytes()];
        let mut state = PlayState::Prebuffering;
        let mut underruns: u64 = 0;

        let result = (|| -> Result<(), wasapi::WasapiError> {
            // Start with a buffer of silence, as WASAPI recommends, then play.
            render.write_to_device(&bytes, None)?;
            client.start_stream()?;
            Ok(())
        })();
        if let Err(error) = result {
            started(Err(map_error(OPEN, error)));
            return;
        }
        let latency_ms = client
            .get_stream_latency()
            .map(|hns| hns as f64 / 10_000.0)
            .ok();
        tracing::info!(%format, buffer_frames, ?latency_ms, prebuffer_ms = config.prebuffer_ms, "audio playback started");
        started(Ok(format));

        let result = (|| -> Result<(), wasapi::WasapiError> {
            while !control.should_stop() {
                // A timeout is not an error: fall through so a dead device surfaces below.
                let _ = event.wait_for_event(EVENT_WAIT_MS);
                let frames = client.get_available_space_in_frames()? as usize;
                if frames == 0 {
                    continue;
                }
                let out = &mut samples[..frames * channels];
                if fill_period(
                    &mut consumer,
                    out,
                    &mut state,
                    prebuffer_frames,
                    max_queue_frames,
                ) {
                    underruns += 1;
                }
                let len = frames * format.frame_bytes();
                f32_to_bytes(format.sample_format, out, &mut bytes[..len]);
                render.write_to_device(&bytes[..len], None)?;
            }
            Ok(())
        })();

        if let Err(error) = result {
            control.fail(&map_error(RUN, error));
        }
        if let Err(error) = client.stop_stream() {
            tracing::debug!(%error, "stopping the playback stream failed");
        }
        tracing::info!(
            underruns,
            dropped_frames = consumer.dropped_frames(),
            "audio playback stopped"
        );
    }
}

#[cfg(not(windows))]
mod imp {
    use super::PlaybackConfig;
    use crate::audio::format::StreamFormat;
    use crate::audio::ring::AudioConsumer;
    use crate::audio::stream::StreamHandle;
    use crate::error::{AppError, AppResult};

    fn unsupported() -> AppError {
        AppError::Audio("audio playback is only supported on Windows".into())
    }

    pub(super) fn device_format(_: Option<&str>) -> AppResult<StreamFormat> {
        Err(unsupported())
    }

    pub(super) fn spawn(
        _: Option<String>,
        _: AudioConsumer,
        _: StreamFormat,
        _: PlaybackConfig,
    ) -> AppResult<(StreamHandle, StreamFormat)> {
        Err(unsupported())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::format::SampleFormat;
    use crate::audio::ring::audio_ring;

    fn format(sample_rate: u32, channels: u16) -> StreamFormat {
        StreamFormat {
            sample_rate,
            channels,
            sample_format: SampleFormat::F32,
        }
    }

    #[test]
    fn formats_must_share_rate_and_channels() {
        assert!(check_formats(&format(48_000, 2), &format(48_000, 2)).is_ok());

        let error = check_formats(&format(48_000, 1), &format(48_000, 2)).unwrap_err();
        assert_eq!(
            error.to_string(),
            "audio device error: formats differ, needs resampling: the audio is 48000 Hz, 1 ch but the output device plays 48000 Hz, 2 ch"
        );
        assert!(check_formats(&format(44_100, 2), &format(48_000, 2)).is_err());
    }

    #[test]
    fn plays_silence_until_the_prebuffer_is_full() {
        let (mut producer, mut consumer) = audio_ring(1, 100);
        let mut state = PlayState::Prebuffering;
        let mut out = [9.0_f32; 4];

        producer.push(&[0.5; 5]);
        assert!(!fill_period(&mut consumer, &mut out, &mut state, 6, 100));
        assert_eq!(out, [0.0; 4]);
        assert_eq!(state, PlayState::Prebuffering);

        producer.push(&[0.5; 1]);
        assert!(!fill_period(&mut consumer, &mut out, &mut state, 6, 100));
        assert_eq!(out, [0.5; 4]);
        assert_eq!(state, PlayState::Playing);
    }

    #[test]
    fn underrun_pads_with_silence_and_prebuffers_again() {
        let (mut producer, mut consumer) = audio_ring(1, 100);
        let mut state = PlayState::Playing;
        let mut out = [9.0_f32; 4];

        producer.push(&[0.5; 2]);
        assert!(fill_period(&mut consumer, &mut out, &mut state, 2, 100));
        assert_eq!(out, [0.5, 0.5, 0.0, 0.0]);
        assert_eq!(state, PlayState::Prebuffering);
    }

    #[test]
    fn drops_the_oldest_audio_beyond_the_max_queue() {
        let (mut producer, mut consumer) = audio_ring(1, 100);
        let mut state = PlayState::Playing;
        let mut out = [0.0_f32; 2];

        producer.push(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        fill_period(&mut consumer, &mut out, &mut state, 1, 3);
        assert_eq!(out, [4.0, 5.0]);
    }
}

#[cfg(all(test, windows))]
mod device_tests {
    use super::*;
    use crate::audio::ring::audio_ring;
    use std::time::{Duration, Instant};

    /// Plays a quiet 440 Hz tone on the default output for 2 s (wear headphones, low volume).
    /// Run with `cargo test --manifest-path src-tauri/Cargo.toml play_tone_on_default_output -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs a real output device"]
    fn play_tone_on_default_output() {
        let format = output_device_format(None).unwrap();
        println!("output format: {format}");
        let channels = usize::from(format.channels);
        let (mut producer, consumer) = audio_ring(format.channels, format.frames_for_ms(200));
        let mut stream = start_playback(None, consumer, format, PlaybackConfig::default()).unwrap();

        let step = 2.0 * std::f32::consts::PI * 440.0 / format.sample_rate as f32;
        let chunk_frames = format.frames_for_ms(10);
        let mut chunk = vec![0.0_f32; chunk_frames * channels];
        let mut phase = 0.0_f32;
        let start = Instant::now();
        let mut next = start;
        while start.elapsed() < Duration::from_secs(2) {
            for frame in chunk.chunks_exact_mut(channels) {
                frame.fill(0.05 * phase.sin());
                phase = (phase + step) % (2.0 * std::f32::consts::PI);
            }
            producer.push(&chunk);
            next += Duration::from_millis(10);
            std::thread::sleep(next.saturating_duration_since(Instant::now()));
        }
        assert!(stream.is_running());
        stream.stop();
        assert!(stream.failure().is_none(), "{:?}", stream.failure());
        assert!(!stream.is_running());
    }
}
