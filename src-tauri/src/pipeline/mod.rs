//! Translation pipelines.
//!
//! Each direction wires audio capture, the translator and audio output together:
//! - **outgoing** ([`outgoing`]): microphone -> translator (to the other side's language) ->
//!   virtual cable;
//! - **incoming** (not yet): another app's audio -> translator (to the user's language) ->
//!   headphones.
//!
//! A pipeline owns starting and stopping its direction and reports status, input level,
//! captions and usage through a callback (the Tauri commands forward them to the panel as
//! events). Audio devices are reached through [`AudioIo`], so the pipelines can be tested with
//! in-memory sources and sinks instead of real hardware.

pub mod outgoing;

use serde::Serialize;

use crate::audio::level::SharedLevel;
use crate::audio::{AudioConsumer, AudioProducer, StreamFormat};
use crate::error::{AppError, AppResult};

pub use outgoing::{OutgoingConfig, OutgoingEvent, OutgoingPipeline};

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

/// Opens audio devices for a pipeline. Both methods block (they open devices).
pub trait AudioIo: Send + Sync {
    /// Starts capturing `device_id` (`None` = system default).
    fn open_input(&self, device_id: Option<&str>) -> AppResult<InputSide>;
    /// Starts playing on `device_id` (`None` = system default) whatever is written to the
    /// returned producer.
    fn open_output(&self, device_id: Option<&str>) -> AppResult<OutputSide>;
}

/// Audio queued between the pipeline and the playback thread before the oldest is dropped.
const OUTPUT_QUEUE_MS: u32 = 200;

/// Audio the playback thread waits for before playing (at start and after running dry). The
/// pipeline keeps the queue above this (see `outgoing::OUTPUT_TARGET_MS`).
const PLAYBACK_PREBUFFER_MS: u32 = 40;

/// Real devices through WASAPI.
#[derive(Debug, Default, Clone, Copy)]
pub struct WasapiIo;

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
