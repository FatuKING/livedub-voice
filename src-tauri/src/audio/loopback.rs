//! Capture of other apps' audio through WASAPI process loopback.
//!
//! [`start_loopback_capture`] activates the `VAD\Process_Loopback` virtual device with
//! `AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK` (Windows 10 2004+): either one app's process
//! tree (`PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE`) or everything except a process
//! tree (`…_EXCLUDE_TARGET_PROCESS_TREE`, used with LiveDub Voice's own id so the translation it
//! plays is never captured back). No driver, and nothing is injected into the captured app, so it
//! is safe with anti-cheat systems.
//!
//! The stream works like the microphone capture ([`capture`](super::capture)): a dedicated
//! thread with COM initialized, event driven, samples converted to interleaved `f32` and pushed
//! into an [`AudioConsumer`] queue that drops the oldest audio when the consumer falls behind.
//! The virtual device has no mix format of its own: the audio engine converts to the format
//! requested in [`LoopbackConfig`] (48 kHz stereo by default, a typical headphone format, so the
//! original audio can later be mixed under the translation without another conversion).
//! [`ToTranslator`](super::ToTranslator) then turns it into the translator's 24 kHz mono PCM16.
//!
//! When a single app is captured and its root process exits, the stream stops with a failure
//! ("the captured app was closed").

use super::capture::CaptureStream;
use super::format::{SampleFormat, StreamFormat};
use super::level::SharedLevel;
use super::ring::AudioConsumer;
use crate::error::{AppError, AppResult};

/// Which audio process loopback captures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopbackTarget {
    /// The audio of this process and all its descendants.
    ProcessTree(u32),
    /// Every app's audio except this process and its descendants.
    ExcludeProcessTree(u32),
}

impl LoopbackTarget {
    /// The whole system except LiveDub Voice's own process tree.
    pub fn everything_except_self() -> Self {
        Self::ExcludeProcessTree(std::process::id())
    }

    /// The process the loopback client is activated with.
    pub fn pid(self) -> u32 {
        match self {
            Self::ProcessTree(pid) | Self::ExcludeProcessTree(pid) => pid,
        }
    }

    /// `true` for `PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE`.
    pub fn includes_tree(self) -> bool {
        matches!(self, Self::ProcessTree(_))
    }
}

/// Loopback capture options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopbackConfig {
    /// Most audio the queue keeps for a slow consumer before dropping the oldest, in ms.
    pub max_backlog_ms: u32,
    /// Sample rate the audio engine converts the captured audio to.
    pub sample_rate: u32,
    /// Channel count the audio engine converts the captured audio to.
    pub channels: u16,
}

impl Default for LoopbackConfig {
    fn default() -> Self {
        Self {
            max_backlog_ms: 200,
            sample_rate: 48_000,
            channels: 2,
        }
    }
}

impl LoopbackConfig {
    /// Format of the captured audio: `f32` interleaved at the requested rate and channels.
    pub fn format(&self) -> StreamFormat {
        StreamFormat {
            sample_rate: self.sample_rate,
            channels: self.channels,
            sample_format: SampleFormat::F32,
        }
    }

    fn validate(&self) -> AppResult<()> {
        if !(8_000..=192_000).contains(&self.sample_rate) || !(1..=8).contains(&self.channels) {
            return Err(AppError::Internal(format!(
                "unsupported loopback format: {}",
                self.format()
            )));
        }
        Ok(())
    }
}

/// Starts capturing `target` (see [`LoopbackTarget`]).
///
/// Blocking (activates the loopback device): call it from a blocking thread. Returns the running
/// stream and the queue its samples arrive in (`f32` interleaved, see
/// [`CaptureStream::format`]).
pub fn start_loopback_capture(
    target: LoopbackTarget,
    config: LoopbackConfig,
) -> AppResult<(CaptureStream, AudioConsumer)> {
    config.validate()?;
    let level = SharedLevel::default();
    let (handle, consumer) = imp::spawn(target, config, level.clone())?;
    Ok((
        CaptureStream::from_parts(handle, config.format(), level),
        consumer,
    ))
}

/// Largest packet expected from the audio engine; buffers start this big so the capture thread
/// does not allocate while running (they grow, once, if a bigger packet ever arrives).
const MAX_PACKET_MS: u32 = 500;

#[cfg(windows)]
mod imp {
    use wasapi::{AudioClient, Direction, SampleType, StreamMode, WaveFormat};
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };

    use super::{LoopbackConfig, LoopbackTarget, MAX_PACKET_MS};
    use crate::audio::format::bytes_to_f32;
    use crate::audio::level::{LEVEL_WINDOW_MS, LevelMeter, SharedLevel};
    use crate::audio::ring::{AudioConsumer, audio_ring};
    use crate::audio::stream::win::{ComGuard, map_error};
    use crate::audio::stream::{EVENT_WAIT_MS, StreamControl, StreamHandle, spawn_stream};
    use crate::error::{AppError, AppResult};

    const OPEN: &str = "could not capture the app audio";
    const RUN: &str = "the app audio capture stopped";

    /// Buffer the audio engine keeps for us, in 100 ns units (20 ms; the engine still delivers
    /// a packet every period, ~10 ms).
    const BUFFER_HNS: i64 = 200_000;

    pub(super) fn spawn(
        target: LoopbackTarget,
        config: LoopbackConfig,
        level: SharedLevel,
    ) -> AppResult<(StreamHandle, AudioConsumer)> {
        spawn_stream("app audio capture", move |control, started| {
            let _com = match ComGuard::new() {
                Ok(com) => com,
                Err(error) => {
                    let _ = started.send(Err(error));
                    return;
                }
            };
            // Every COM object lives inside `run`, so it is released before `_com` drops.
            run(target, config, level, &control, |result| {
                let _ = started.send(result);
            });
        })
    }

    /// Tells when a process has exited, without keeping it alive longer than a handle does.
    struct ProcessWatch(HANDLE);

    impl ProcessWatch {
        fn open(pid: u32) -> Option<Self> {
            // SAFETY: plain Win32 call; the handle is closed on drop.
            unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }
                .ok()
                .map(Self)
        }

        fn has_exited(&self) -> bool {
            // SAFETY: the handle is valid until drop; a zero timeout never blocks.
            unsafe { WaitForSingleObject(self.0, 0) == WAIT_OBJECT_0 }
        }
    }

    impl Drop for ProcessWatch {
        fn drop(&mut self) {
            // SAFETY: opened by us, closed once.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    fn run(
        target: LoopbackTarget,
        config: LoopbackConfig,
        level: SharedLevel,
        control: &StreamControl,
        started: impl FnOnce(AppResult<AudioConsumer>),
    ) {
        let format = config.format();
        let open = || -> AppResult<_> {
            let mut client =
                AudioClient::new_application_loopback_client(target.pid(), target.includes_tree())
                    .map_err(|e| map_error(OPEN, e))?;
            let wave = WaveFormat::new(
                32,
                32,
                SampleType::Float,
                format.sample_rate as usize,
                usize::from(format.channels),
                None,
            );
            // The loopback device has no mix format: the engine converts to ours.
            let mode = StreamMode::EventsShared {
                autoconvert: true,
                buffer_duration_hns: BUFFER_HNS,
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
            client.start_stream().map_err(|e| map_error(OPEN, e))?;
            Ok((client, event, capture))
        };
        let (client, event, capture) = match open() {
            Ok(opened) => opened,
            Err(error) => {
                started(Err(error));
                return;
            }
        };
        let watch = match target {
            LoopbackTarget::ProcessTree(pid) => ProcessWatch::open(pid),
            LoopbackTarget::ExcludeProcessTree(_) => None,
        };

        let channels = usize::from(format.channels);
        let (mut producer, consumer) =
            audio_ring(format.channels, format.frames_for_ms(config.max_backlog_ms));
        let mut meter = LevelMeter::new(level, format.frames_for_ms(LEVEL_WINDOW_MS) * channels);
        let max_frames = format.frames_for_ms(MAX_PACKET_MS);
        let mut bytes = vec![0_u8; max_frames * format.frame_bytes()];
        let mut samples = vec![0.0_f32; max_frames * channels];
        tracing::info!(?target, %format, "app audio capture started");
        started(Ok(consumer));

        let result = (|| -> Result<(), AppError> {
            while !control.should_stop() {
                // A timeout is not an error: fall through so a dead stream surfaces below.
                let _ = event.wait_for_event(EVENT_WAIT_MS);
                loop {
                    let frames = capture
                        .get_next_packet_size()
                        .map_err(|e| map_error(RUN, e))?
                        .unwrap_or(0) as usize;
                    if frames == 0 {
                        break;
                    }
                    if frames * format.frame_bytes() > bytes.len() {
                        tracing::warn!(frames, "loopback packet larger than expected");
                        bytes.resize(frames * format.frame_bytes(), 0);
                        samples.resize(frames * channels, 0.0);
                    }
                    // Packets flagged silent come back as zeros.
                    let (read, _info) = capture
                        .read_from_device(&mut bytes)
                        .map_err(|e| map_error(RUN, e))?;
                    let len = read as usize * format.frame_bytes();
                    let count = bytes_to_f32(format.sample_format, &bytes[..len], &mut samples);
                    meter.process(&samples[..count]);
                    if !producer.is_abandoned() {
                        producer.push(&samples[..count]);
                    }
                }
                if watch.as_ref().is_some_and(ProcessWatch::has_exited) {
                    tracing::info!(pid = target.pid(), "the captured app exited");
                    return Err(AppError::Audio(format!(
                        "{RUN}: the captured app was closed"
                    )));
                }
            }
            Ok(())
        })();

        meter.reset();
        if let Err(error) = result {
            control.fail(&error);
        }
        if let Err(error) = client.stop_stream() {
            tracing::debug!(%error, "stopping the loopback stream failed");
        }
        tracing::info!("app audio capture stopped");
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{LoopbackConfig, LoopbackTarget};
    use crate::audio::level::SharedLevel;
    use crate::audio::ring::AudioConsumer;
    use crate::audio::stream::StreamHandle;
    use crate::error::{AppError, AppResult};

    pub(super) fn spawn(
        _: LoopbackTarget,
        _: LoopbackConfig,
        _: SharedLevel,
    ) -> AppResult<(StreamHandle, AudioConsumer)> {
        Err(AppError::Audio(
            "app audio capture is only supported on Windows".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::ToTranslator;

    #[test]
    fn targets_map_to_the_loopback_modes() {
        let include = LoopbackTarget::ProcessTree(42);
        let exclude = LoopbackTarget::everything_except_self();

        assert_eq!((include.pid(), include.includes_tree()), (42, true));
        assert_eq!(
            (exclude.pid(), exclude.includes_tree()),
            (std::process::id(), false)
        );
    }

    #[test]
    fn default_format_is_48k_stereo_float() {
        let format = LoopbackConfig::default().format();

        assert_eq!(
            format,
            StreamFormat {
                sample_rate: 48_000,
                channels: 2,
                sample_format: SampleFormat::F32,
            }
        );
        assert_eq!(format.frame_bytes(), 8);
    }

    #[test]
    fn rejects_unusable_formats_before_opening_anything() {
        for config in [
            LoopbackConfig {
                sample_rate: 0,
                ..LoopbackConfig::default()
            },
            LoopbackConfig {
                channels: 0,
                ..LoopbackConfig::default()
            },
        ] {
            let error = start_loopback_capture(LoopbackTarget::ProcessTree(1), config)
                .err()
                .unwrap();
            assert_eq!(error.code(), "internal");
        }
    }

    /// The captured format converts to what the translator expects: 24 kHz mono PCM16.
    #[test]
    fn captured_audio_converts_to_translator_input() {
        let format = LoopbackConfig::default().format();
        let mut convert = ToTranslator::new(format.sample_rate, format.channels).unwrap();
        // 1 s of a 440 Hz tone, left and right identical.
        let frames = format.sample_rate as usize;
        let interleaved: Vec<f32> = (0..frames)
            .flat_map(|i| {
                let value = 0.5
                    * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / format.sample_rate as f32)
                        .sin();
                [value, value]
            })
            .collect();

        let mut pcm = Vec::new();
        for chunk in interleaved.chunks(format.frames_for_ms(10) * 2) {
            convert.process(chunk, &mut pcm);
        }
        convert.flush(&mut pcm);

        // ~24000 mono samples (the resampler's delay may hold a few back), at about half scale.
        assert!((23_000..=24_500).contains(&pcm.len()), "{}", pcm.len());
        let peak = pcm.iter().map(|s| s.unsigned_abs()).max().unwrap();
        assert!((15_000..=17_500).contains(&peak), "peak {peak}");
    }
}

#[cfg(all(test, windows))]
mod hardware_tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::audio::ToTranslator;
    use crate::audio::apps::resolve_source;
    use crate::settings::SourceApp;

    /// Captures 5 s of one app (env `LIVEDUB_LOOPBACK_APP`, an executable name such as
    /// `chrome.exe`) or of the whole system minus this process (variable unset), converts it to
    /// the translator's 24 kHz mono PCM16 and writes it to `LIVEDUB_LOOPBACK_WAV` (default:
    /// `loopback.wav` in the temp dir). Play a YouTube video first, then listen to the file.
    ///
    /// ```text
    /// $env:LIVEDUB_LOOPBACK_APP = "chrome.exe"
    /// cargo test --manifest-path src-tauri/Cargo.toml capture_app_audio_to_wav -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "needs an app playing audio"]
    fn capture_app_audio_to_wav() {
        let source = match std::env::var("LIVEDUB_LOOPBACK_APP") {
            Ok(name) if !name.trim().is_empty() => SourceApp::Process { name },
            _ => SourceApp::AllExceptSelf,
        };
        let path = std::env::var_os("LIVEDUB_LOOPBACK_WAV")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("loopback.wav"));
        let target = resolve_source(&source).unwrap();
        println!("capturing {source:?} as {target:?}");

        let (mut stream, mut consumer) =
            start_loopback_capture(target, LoopbackConfig::default()).unwrap();
        let format = stream.format();
        let mut convert = ToTranslator::new(format.sample_rate, format.channels).unwrap();
        let mut buffer = vec![0.0_f32; format.frames_for_ms(100) * usize::from(format.channels)];
        let mut pcm = Vec::new();
        let mut max_peak = 0.0_f32;
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) && stream.is_running() {
            std::thread::sleep(Duration::from_millis(20));
            let count = consumer.pop(&mut buffer);
            convert.process(&buffer[..count], &mut pcm);
            max_peak = max_peak.max(stream.level().peak);
        }
        convert.flush(&mut pcm);
        stream.stop();

        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 24_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for sample in &pcm {
            writer.write_sample(*sample).unwrap();
        }
        writer.finalize().unwrap();

        let seconds = pcm.len() as f64 / 24_000.0;
        println!(
            "format {format}; wrote {seconds:.2} s of 24 kHz mono to {}; max peak {max_peak:.4}; dropped {} frames; failure {:?}",
            path.display(),
            consumer.dropped_frames(),
            stream.failure()
        );
        assert!(stream.failure().is_none(), "{:?}", stream.failure());
    }
}
