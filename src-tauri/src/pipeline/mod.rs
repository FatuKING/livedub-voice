//! Translation pipelines.
//!
//! Each direction wires audio capture, the translator and audio output together, with the same
//! engine ([`translation`]):
//! - **outgoing** ([`outgoing`]): microphone -> translator (to the other side's language) ->
//!   virtual cable;
//! - **incoming** ([`incoming`]): another app's audio (or the whole system minus LiveDub Voice,
//!   through process loopback) -> translator (to the user's language) -> headphones, with the
//!   original audio ducked while it runs.
//!
//! A pipeline owns starting and stopping its direction and reports status, input level,
//! captions and usage through a callback (the Tauri commands forward them to the panel as
//! events). Audio devices are reached through [`AudioIo`], so the pipelines can be tested with
//! in-memory sources and sinks instead of real hardware.

pub mod incoming;
pub mod latency;
pub mod outgoing;
pub mod recorder;
pub mod translation;

use std::path::PathBuf;

use serde::Serialize;

use crate::audio::ducking::{DuckScope, Ducker, SharedGain};
use crate::audio::level::SharedLevel;
use crate::audio::{AudioConsumer, AudioProducer, LoopbackTarget, StreamFormat};
use crate::error::{AppError, AppResult};
use crate::settings::SourceApp;

pub use translation::{
    Direction, EventSink, InputSource, PipelineConfig, PipelineEvent, TranslationPipeline,
};

/// What a pipeline is doing, as shown in the panel's status pill.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PipelineState {
    /// Not running.
    #[default]
    Idle,
    /// Opening devices and connecting to the translator.
    Connecting,
    /// Translating.
    Live,
    /// Stop requested: finishing the translation of what was already said (the tail of the last
    /// sentence) before releasing the devices.
    Stopping,
    /// Stopped because of an error (see [`PipelineStatus::error`]).
    Error,
}

/// An error as the panel receives it: the same `{ code, message }` shape as a failed command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PipelineError {
    pub code: String,
    pub message: String,
}

impl From<&AppError> for PipelineError {
    fn from(error: &AppError) -> Self {
        Self {
            code: error.code().to_owned(),
            message: error.to_string(),
        }
    }
}

/// A pipeline's state, plus the error that stopped it (only with [`PipelineState::Error`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PipelineStatus {
    pub state: PipelineState,
    pub error: Option<PipelineError>,
}

impl PipelineStatus {
    pub fn idle() -> Self {
        Self::default()
    }

    pub fn connecting() -> Self {
        Self {
            state: PipelineState::Connecting,
            error: None,
        }
    }

    pub fn live() -> Self {
        Self {
            state: PipelineState::Live,
            error: None,
        }
    }

    pub fn stopping() -> Self {
        Self {
            state: PipelineState::Stopping,
            error: None,
        }
    }

    pub fn error(error: &AppError) -> Self {
        Self {
            state: PipelineState::Error,
            error: Some(error.into()),
        }
    }
}

/// A running audio stream a pipeline owns (a WASAPI stream, or an in-memory one in tests).
pub trait StreamGuard: Send {
    /// Why the stream stopped on its own, if it did (device unplugged, …).
    fn failure(&self) -> Option<String>;
    /// Stops the stream and waits for its thread. Blocking.
    fn stop(&mut self);
}

impl StreamGuard for crate::audio::CaptureStream {
    fn failure(&self) -> Option<String> {
        crate::audio::CaptureStream::failure(self).map(str::to_owned)
    }

    fn stop(&mut self) {
        crate::audio::CaptureStream::stop(self);
    }
}

impl StreamGuard for crate::audio::PlaybackStream {
    fn failure(&self) -> Option<String> {
        crate::audio::PlaybackStream::failure(self).map(str::to_owned)
    }

    fn stop(&mut self) {
        crate::audio::PlaybackStream::stop(self);
    }
}

/// A capturing input: its samples (interleaved `f32` in `format`), level and stream.
pub struct InputSide {
    pub consumer: AudioConsumer,
    pub format: StreamFormat,
    pub level: SharedLevel,
    pub stream: Box<dyn StreamGuard>,
}

/// A playing output: where to write its samples (interleaved `f32` in `format`) and its stream.
pub struct OutputSide {
    pub producer: AudioProducer,
    pub format: StreamFormat,
    pub stream: Box<dyn StreamGuard>,
}

/// The captured apps, ducked while an incoming pipeline runs (see [`crate::audio::ducking`]).
pub trait DuckGuard: Send {
    /// What the captured audio must be multiplied by so the translator hears the apps at their
    /// normal volume.
    fn gain(&self) -> SharedGain;
    /// Changes the original-audio volume (percent) while ducking.
    fn set_original_volume(&self, percent: u8);
    /// Restores the apps' volume. Blocking.
    fn stop(&mut self);
}

impl DuckGuard for Ducker {
    fn gain(&self) -> SharedGain {
        Ducker::gain(self)
    }

    fn set_original_volume(&self, percent: u8) {
        Ducker::set_original_volume(self, percent);
    }

    fn stop(&mut self) {
        Ducker::stop(self);
    }
}

/// Opens audio devices for a pipeline. Every method blocks (they open devices).
pub trait AudioIo: Send + Sync {
    /// Starts capturing `device_id` (`None` = system default).
    fn open_input(&self, device_id: Option<&str>) -> AppResult<InputSide>;
    /// Starts playing on `device_id` (`None` = system default) whatever is written to the
    /// returned producer.
    fn open_output(&self, device_id: Option<&str>) -> AppResult<OutputSide>;
    /// What process loopback captures for `source`: one app's process tree, or everything
    /// except LiveDub Voice. Fails with [`AppError::SourceAppNotRunning`] when the chosen app is
    /// not running.
    ///
    /// Sources that cannot capture apps (test doubles of the outgoing pipeline) keep the
    /// defaults, which fail.
    fn resolve_loopback(&self, source: &SourceApp) -> AppResult<LoopbackTarget> {
        let _ = source;
        Err(AppError::Audio("app audio capture is not available".into()))
    }
    /// Starts capturing `target` through process loopback.
    fn open_loopback(&self, target: LoopbackTarget) -> AppResult<InputSide> {
        let _ = target;
        Err(AppError::Audio("app audio capture is not available".into()))
    }
    /// Ducks the apps `target` captures to `original_volume` percent until the guard stops.
    fn duck(&self, target: LoopbackTarget, original_volume: u8) -> AppResult<Box<dyn DuckGuard>> {
        let _ = (target, original_volume);
        Err(AppError::Audio("ducking is not available".into()))
    }
}

/// Audio queued between the pipeline and the playback thread before the oldest is dropped.
const OUTPUT_QUEUE_MS: u32 = 200;

/// Audio the playback thread waits for before playing (at start and after running dry). The
/// pipeline keeps the queue above this (see `translation::OUTPUT_TARGET_MS`).
const PLAYBACK_PREBUFFER_MS: u32 = 40;

/// Real devices through WASAPI.
#[derive(Debug, Default, Clone)]
pub struct WasapiIo {
    /// Where the ducked volumes are remembered until restored (`ducking.json`); `None` keeps
    /// them in memory only.
    pub ducking_journal: Option<PathBuf>,
}

impl WasapiIo {
    /// Real devices, remembering ducked volumes in `journal`.
    pub fn with_ducking_journal(journal: PathBuf) -> Self {
        Self {
            ducking_journal: Some(journal),
        }
    }
}

impl AudioIo for WasapiIo {
    fn open_input(&self, device_id: Option<&str>) -> AppResult<InputSide> {
        let (stream, consumer) =
            crate::audio::start_capture(device_id, crate::audio::CaptureConfig::default())?;
        Ok(InputSide {
            consumer,
            format: stream.format(),
            level: stream.level_handle(),
            stream: Box::new(stream),
        })
    }

    fn resolve_loopback(&self, source: &SourceApp) -> AppResult<LoopbackTarget> {
        crate::audio::resolve_source(source)
    }

    fn open_loopback(&self, target: LoopbackTarget) -> AppResult<InputSide> {
        let (stream, consumer) =
            crate::audio::start_loopback_capture(target, crate::audio::LoopbackConfig::default())?;
        Ok(InputSide {
            consumer,
            format: stream.format(),
            level: stream.level_handle(),
            stream: Box::new(stream),
        })
    }

    fn duck(&self, target: LoopbackTarget, original_volume: u8) -> AppResult<Box<dyn DuckGuard>> {
        let ducker = Ducker::start(
            DuckScope::from(target),
            original_volume,
            self.ducking_journal.clone(),
        )?;
        Ok(Box::new(ducker))
    }

    fn open_output(&self, device_id: Option<&str>) -> AppResult<OutputSide> {
        let format = crate::audio::playback::output_device_format(device_id)?;
        let (producer, consumer) =
            crate::audio::audio_ring(format.channels, format.frames_for_ms(OUTPUT_QUEUE_MS));
        let stream = crate::audio::start_playback(
            device_id,
            consumer,
            format,
            crate::audio::PlaybackConfig {
                prebuffer_ms: PLAYBACK_PREBUFFER_MS,
                max_queue_ms: OUTPUT_QUEUE_MS,
            },
        )?;
        Ok(OutputSide {
            producer,
            format,
            stream: Box::new(stream),
        })
    }
}
