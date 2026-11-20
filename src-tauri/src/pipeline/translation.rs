//! A translation pipeline, for either direction:
//!
//! - **outgoing**: the user's microphone -> translator (to the other side's language) -> virtual
//!   cable;
//! - **incoming**: other apps' audio (process loopback: one app, or the whole system minus
//!   LiveDub Voice) -> translator (to the user's language) -> headphones, with the captured apps
//!   ducked (see [`crate::audio::ducking`]).
//!
//! ```text
//! input (device format) -> [ducking compensation] -> ToTranslator (PCM16 mono 24 kHz) -> LiveSession
//! LiveSession outputs: audio -> JitterBuffer, translated text -> TranscriptDelta events
//! JitterBuffer -> FromTranslator (output format) -> playback queue -> cable / headphones
//! ```
//!
//! Two workers run while the pipeline is live:
//! - the **pump** thread wakes every [`TICK`]: it moves the input's samples to the translator,
//!   and tops the playback queue up to [`OUTPUT_TARGET_MS`] from the jitter buffer. Topping up to
//!   a level (instead of on a timer) makes the pipeline pull translated audio at exactly the
//!   output's clock, so no delay builds up between them. It also reports the input level and the
//!   audio sent;
//! - the **supervisor** task reads the translator's outputs into the jitter buffer, forwards the
//!   captions and watches both devices.
//!
//! # Ducking (incoming)
//!
//! While an incoming pipeline is live, the captured apps play at the configured original-audio
//! volume (their Windows session volume). Process loopback captures them after that volume, so
//! the pump multiplies the captured audio by the ducker's gain (`1 / level`) and the translator
//! hears them at full volume; the input level shown in the panel is compensated too. The apps are
//! ducked once the translator is connected and before their capture starts (so the compensation
//! never amplifies audio that was not ducked), and restored once the pipeline has stopped (after
//! the translated tail played), on a fatal error too. A failure to duck is logged and the
//! translation runs without it.
//!
//! # Stopping
//!
//! A stop requested by the user is graceful, so the end of the last sentence is not lost: the
//! state becomes `stopping`, the input is replaced by [`TRAILING_SILENCE`] of silence (the model
//! needs it to finish the sentence), then `session.close` is sent and the outputs are read until
//! `session.closed`, and finally the jitter buffer plays its tail into the output. After the close
//! the server flushes the translated tail and then seconds of digital silence; once
//! [`TAIL_SILENCE_DONE`] of it has arrived the translation is over and the pipeline stops waiting
//! for `session.closed`. The whole sequence is capped at [`STOP_DRAIN_LIMIT`]. A fatal error
//! (device gone, captured app closed, key rejected) tears down at once.
//!
//! A fast stop ([`TranslationPipeline::shutdown`], or [`StopHandle::stop_now`] while a graceful
//! stop runs) skips the trailing silence and the tail: it sends `session.close`, waits at most
//! [`FAST_CLOSE_TIMEOUT`] for the session to end and releases everything. It is used when the
//! app exits.
//!
//! # Reconnecting
//!
//! When the session drops on its own (network loss, the server closing the socket, a read or
//! ping timeout, an expired session) after a retryable error, the pipeline keeps both devices
//! open and the apps ducked, reports `reconnecting` (attempt n of max) and opens a new session
//! with backoff (see [`super::reconnect`]). While it reconnects the captured audio is discarded
//! (never queued: once back, the translation is about what is said now) and the jitter buffer
//! plays out what had already arrived. A fatal error, a device failure, the attempts running out
//! or a stop end it.
//!
//! # Drift and backlog
//!
//! The model streams its output at real time, silence included, but may deliver translated
//! speech faster than that. The jitter buffer keeps all the speech and skips digital silence
//! queued behind it (keeping a short pause, see [`JitterBuffer::push_delta`]); the output
//! converter then plays slightly faster (see [`drift_adjust`]): up to 2 % for a small backlog
//! (clock drift), up to [`MAX_CATCH_UP`] (6 %) when seconds are waiting.
//!
//! # Diagnostics
//!
//! At stop a one-line summary is logged (jitter underruns, drops, max backlog, deltas, audio sent
//! and received, latency per utterance, input health). With `LIVEDUB_RECORD_DIR` set, the session
//! is also recorded to disk (see [`super::recorder`]).
//!
//! Audio contents are never logged; only levels, sizes and timings are.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle as ThreadHandle;
use std::time::{Duration, Instant};

use serde_json::json;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use super::latency::{LatencyEvent, LatencyReport, LatencyTracker, SPEECH_RMS};
use super::reconnect::{ReconnectError, ReconnectPolicy, reconnect};
use super::recorder::{InputHealth, SessionRecorder};
use super::{AudioIo, DuckGuard, InputSide, OutputSide, PipelineStatus, StreamGuard};
use crate::audio::LoopbackTarget;
use crate::audio::ducking::SharedGain;
use crate::audio::level::{InputLevel, LEVEL_WINDOW_MS, SharedLevel};
use crate::audio::resample::{MAX_CATCH_UP, MAX_RATE_ADJUST};
use crate::audio::{AudioConsumer, AudioProducer, FromTranslator, StreamFormat, ToTranslator};
use crate::error::{AppError, AppResult};
use crate::settings::{Language, SourceApp};
use crate::translator::jitter::is_digital_silence;
use crate::translator::pcm::{SAMPLE_RATE, samples_for_ms};
use crate::translator::{
    AudioInput, JitterBuffer, LiveSession, Translator, TranslatorError, TranslatorOutput,
};

/// How often the pump thread runs.
const TICK: Duration = Duration::from_millis(10);

/// Translated audio the pump keeps queued for the output. Must stay above the playback prebuffer
/// (`PLAYBACK_PREBUFFER_MS` in the parent module) so playback starts.
pub(crate) const OUTPUT_TARGET_MS: u32 = 60;

/// Translated audio pulled from the jitter buffer at a time (10 ms).
const OUTPUT_CHUNK_SAMPLES: usize = samples_for_ms(10);

/// Most chunks the pump writes per tick, so a stuck playback queue cannot spin it.
const MAX_FILLS_PER_TICK: usize = 32;

/// How often the audio sent so far is reported (and jitter stats recorded).
const AUDIO_SENT_INTERVAL: Duration = Duration::from_secs(1);

/// How often the supervisor checks that both devices still work.
const HEALTH_INTERVAL: Duration = Duration::from_millis(100);

/// Silence sent after the user stops, so the model finishes the last sentence.
const TRAILING_SILENCE: Duration = Duration::from_millis(1500);

/// Longest a graceful stop may take (trailing silence, close, playing the tail).
const STOP_DRAIN_LIMIT: Duration = Duration::from_secs(10);

/// Digital silence received after `session.close` (audio time, not wall time) after which the
/// translation counts as finished without waiting for `session.closed`. After the close the
/// server flushes the translated tail first and then many seconds of zeros, faster than real
/// time; this is longer than the model's own delay (~4 s), so no speech is cut.
const TAIL_SILENCE_DONE: Duration = Duration::from_secs(5);

/// Time after the jitter buffer is empty for the playback queue to reach the output device.
const OUTPUT_DRAIN_TAIL: Duration = Duration::from_millis(150);

/// Longest wait for the translator to close after a fatal error or a timed-out stop.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(3);

/// Longest wait for the translator to close on a fast stop (the app is exiting).
pub(crate) const FAST_CLOSE_TIMEOUT: Duration = Duration::from_secs(1);

/// Backlog above the prebuffer at which the drift correction reaches [`MAX_RATE_ADJUST`].
const DRIFT_RANGE_MS: f64 = 1000.0;

/// Backlog above the prebuffer at which the catch-up reaches [`MAX_CATCH_UP`].
const CATCH_UP_FULL_MS: f64 = 4000.0;

/// Which way a pipeline translates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Other apps' audio -> the user's headphones.
    Incoming,
    /// The user's microphone -> the virtual cable.
    Outgoing,
}

impl Direction {
    /// `"incoming"` / `"outgoing"`: for logs, thread and recording names.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Incoming => "incoming",
            Self::Outgoing => "outgoing",
        }
    }
}

/// What a pipeline reports while it runs.
#[derive(Debug, Clone, PartialEq)]
pub enum PipelineEvent {
    /// The pipeline's state changed. The first event is `connecting`; the last one is always
    /// `idle` (stopped) or `error` (could not start, or stopped on a fatal error). A stop asked
    /// by the user goes through `stopping` first.
    Status(PipelineStatus),
    /// Input level (the microphone, or the captured apps as the translator hears them, before
    /// ducking), ~20 times per second while live.
    Level(InputLevel),
    /// Translated text (append-only delta), in the target language.
    TranscriptDelta(String),
    /// Seconds of input audio sent to the translator since start, every second and once more
    /// when the pipeline stops.
    AudioSent { seconds: f64 },
}

/// Receives the pipeline's events, from the pipeline's threads.
pub type EventSink = Arc<dyn Fn(PipelineEvent) + Send + Sync>;

/// Where a pipeline's input comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputSource {
    /// A capture device (the microphone); `None` = system default.
    Device(Option<String>),
    /// Other apps' audio through process loopback.
    Apps(SourceApp),
}

/// What a pipeline uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineConfig {
    pub direction: Direction,
    pub input: InputSource,
    /// Where the translation plays; `None` = system default output.
    pub output_device_id: Option<String>,
    /// Language the listener hears.
    pub target_language: Language,
    /// Volume (percent) the captured apps are ducked to while live; `None` = no ducking. Only
    /// used with [`InputSource::Apps`].
    pub original_volume: Option<u8>,
    /// How a dropped session is reconnected.
    pub reconnect: ReconnectPolicy,
}

/// How a pipeline was asked to stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
enum StopMode {
    /// Finish the last sentence and play its tail (the user pressed Stop).
    Graceful = 1,
    /// Close the session and release everything now (the app is exiting).
    Fast = 2,
}

/// A stop request shared by the pipeline's owner and its supervisor. Only ever escalates
/// (running -> graceful -> fast).
#[derive(Default)]
struct StopSignal {
    /// 0 = running, else a [`StopMode`].
    mode: AtomicU8,
    changed: Notify,
}

impl StopSignal {
    fn request(&self, mode: StopMode) {
        self.mode.fetch_max(mode as u8, Ordering::SeqCst);
        self.changed.notify_waiters();
    }

    fn mode(&self) -> Option<StopMode> {
        match self.mode.load(Ordering::SeqCst) {
            0 => None,
            1 => Some(StopMode::Graceful),
            _ => Some(StopMode::Fast),
        }
    }

    /// Waits until the mode is at least `wanted`.
    async fn reached(&self, wanted: StopMode) -> StopMode {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            // Registered before checking, so a request in between is not missed.
            changed.as_mut().enable();
            if let Some(mode) = self.mode()
                && mode >= wanted
            {
                return mode;
            }
            changed.await;
        }
    }
}

/// Asks a pipeline to stop at once (see [`TranslationPipeline::shutdown`]) without owning it,
/// for example while another task waits for its graceful stop. Cheap to clone.
#[derive(Clone)]
pub struct StopHandle(Arc<StopSignal>);

impl StopHandle {
    /// Turns any stop (or none) into a fast stop: `session.close`, then release everything
    /// without playing the tail.
    pub fn stop_now(&self) {
        self.0.request(StopMode::Fast);
    }
}

/// State shared by the pump thread and the supervisor task.
struct Shared {
    direction: Direction,
    started: Instant,
    jitter: Mutex<JitterBuffer>,
    latency: Mutex<LatencyTracker>,
    health: Mutex<InputHealth>,
    recorder: Mutex<Option<SessionRecorder>>,
    /// Ends the pump thread.
    stop: AtomicBool,
    /// Stop requested: send silence instead of the input.
    trailing: AtomicBool,
    /// The trailing silence has been sent; nothing more goes to the translator.
    input_done: AtomicBool,
    /// The jitter buffer was drained (after `finish_input`) and its tail reached the output.
    output_drained: AtomicBool,
    sent_samples: AtomicU64,
    output_deltas: AtomicU64,
    /// The ducked apps (incoming), restored at shutdown.
    ducking: Mutex<Option<Box<dyn DuckGuard>>>,
    /// Where the pump sends the input: the live session's input, `None` while reconnecting
    /// (the audio is then discarded).
    session_input: Mutex<Option<AudioInput>>,
}

impl Shared {
    fn new(direction: Direction, recorder: Option<SessionRecorder>) -> Self {
        Self {
            direction,
            started: Instant::now(),
            jitter: Mutex::new(JitterBuffer::default()),
            latency: Mutex::new(LatencyTracker::default()),
            health: Mutex::new(InputHealth::default()),
            recorder: Mutex::new(recorder),
            stop: AtomicBool::new(false),
            trailing: AtomicBool::new(false),
            input_done: AtomicBool::new(false),
            output_drained: AtomicBool::new(false),
            sent_samples: AtomicU64::new(0),
            output_deltas: AtomicU64::new(0),
            ducking: Mutex::new(None),
            session_input: Mutex::new(None),
        }
    }

    /// Time since the pipeline went live.
    fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Runs `write` on the recorder, if recording.
    fn record(&self, write: impl FnOnce(&mut SessionRecorder)) {
        if let Some(recorder) = lock(&self.recorder).as_mut() {
            write(recorder);
        }
    }

    fn record_event(&self, fields: serde_json::Value) {
        let at = self.elapsed();
        self.record(|recorder| recorder.event(at, fields));
    }

    /// Logs (debug) and records what the latency tracker noticed.
    fn latency_event(&self, event: LatencyEvent) {
        let direction = self.direction.name();
        let fields = match event {
            LatencyEvent::SpeechStart { at } => {
                tracing::debug!(direction, at_ms = at.as_millis() as u64, "speech started");
                json!({ "kind": "speech_start", "at_ms": at.as_millis() as u64 })
            }
            LatencyEvent::SpeechEnd { at } => {
                tracing::debug!(direction, at_ms = at.as_millis() as u64, "speech ended");
                json!({ "kind": "speech_end", "at_ms": at.as_millis() as u64 })
            }
            LatencyEvent::StartToAudio { utterance, latency } => {
                tracing::debug!(
                    direction,
                    utterance,
                    latency_ms = latency.as_millis() as u64,
                    "latency: start of speech -> first translated audio"
                );
                json!({ "kind": "latency_start_to_audio", "utterance": utterance, "ms": latency.as_millis() as u64 })
            }
            LatencyEvent::EndToAudio { utterance, latency } => {
                tracing::debug!(
                    direction,
                    utterance,
                    latency_ms = latency.as_millis() as u64,
                    "latency: end of speech -> first translated audio"
                );
                json!({ "kind": "latency_end_to_audio", "utterance": utterance, "ms": latency.as_millis() as u64 })
            }
        };
        self.record_event(fields);
    }

    /// Restores the ducked apps, if any. Blocking.
    fn stop_ducking(&self) {
        let ducking = lock(&self.ducking).take();
        if let Some(mut ducking) = ducking {
            ducking.stop();
        }
    }
}

/// A running translation, in one direction. Stop it with [`stop`](Self::stop); dropping it also stops it
/// (in the background).
pub struct TranslationPipeline {
    stop: Arc<StopSignal>,
    task: Option<JoinHandle<()>>,
    shared: Arc<Shared>,
}

impl TranslationPipeline {
    /// Opens the output and the input, starts a translation session, ducks the captured apps
    /// (incoming) and starts translating.
    ///
    /// Emits `connecting` first, then `live`; on failure emits `error` and returns the error,
    /// with nothing left running. Must run inside a Tokio runtime.
    pub async fn start(
        config: PipelineConfig,
        translator: Arc<dyn Translator>,
        io: Arc<dyn AudioIo>,
        events: EventSink,
    ) -> AppResult<Self> {
        let direction = config.direction.name();
        events(PipelineEvent::Status(PipelineStatus::connecting()));
        match Self::try_start(config, translator, io, Arc::clone(&events)).await {
            Ok(pipeline) => Ok(pipeline),
            Err(error) => {
                tracing::warn!(direction, code = error.code(), %error, "translation could not start");
                events(PipelineEvent::Status(PipelineStatus::error(&error)));
                Err(error)
            }
        }
    }

    async fn try_start(
        config: PipelineConfig,
        translator: Arc<dyn Translator>,
        io: Arc<dyn AudioIo>,
        events: EventSink,
    ) -> AppResult<Self> {
        let direction = config.direction;
        // The output first: for outgoing it is the device most likely to be missing (the cable).
        let output = {
            let io = Arc::clone(&io);
            let id = config.output_device_id.clone();
            blocking(move || io.open_output(id.as_deref())).await?
        };
        // The microphone opens now; the apps are only found now (fails fast when the app is not
        // running) and captured once ducked, below.
        let pending = {
            let io = Arc::clone(&io);
            let source = config.input.clone();
            blocking(move || match source {
                InputSource::Device(id) => io.open_input(id.as_deref()).map(PendingInput::Opened),
                InputSource::Apps(source) => io.resolve_loopback(&source).map(PendingInput::Apps),
            })
            .await
        };
        let pending = match pending {
            Ok(pending) => pending,
            Err(error) => {
                stop_streams(vec![output.stream]).await;
                return Err(error);
            }
        };

        let session = match translator.start(config.target_language).await {
            Ok(session) => session,
            Err(error) => {
                let mut streams = vec![output.stream];
                if let PendingInput::Opened(input) = pending {
                    streams.push(input.stream);
                }
                stop_streams(streams).await;
                return Err(error.into());
            }
        };

        let (input, ducking) = match pending {
            PendingInput::Opened(input) => (input, None),
            PendingInput::Apps(target) => {
                // Ducked before the capture starts, so every captured sample is ducked and the
                // compensation never amplifies audio that was not.
                let ducking = match config.original_volume {
                    Some(volume) => {
                        let io = Arc::clone(&io);
                        match blocking(move || io.duck(target, volume)).await {
                            Ok(ducking) => Some(ducking),
                            Err(error) => {
                                tracing::warn!(code = error.code(), %error, "could not lower the original audio; translating without ducking");
                                None
                            }
                        }
                    }
                    None => None,
                };
                match blocking(move || io.open_loopback(target)).await {
                    Ok(input) => (input, ducking),
                    Err(error) => {
                        drop(session);
                        stop_streams(vec![output.stream]).await;
                        stop_ducking(ducking).await;
                        return Err(error);
                    }
                }
            }
        };

        let converters = ToTranslator::new(input.format.sample_rate, input.format.channels)
            .and_then(|to| {
                FromTranslator::new(output.format.sample_rate, output.format.channels)
                    .map(|from| (to, from))
            });
        let (to_translator, from_translator) = match converters {
            Ok(converters) => converters,
            Err(error) => {
                drop(session);
                stop_streams(vec![input.stream, output.stream]).await;
                stop_ducking(ducking).await;
                return Err(AppError::Audio(error.to_string()));
            }
        };
        tracing::info!(
            direction = direction.name(),
            provider = translator.name(),
            input = %input.format,
            output = %output.format,
            language = ?config.target_language,
            ducked = ducking.is_some(),
            "translation started"
        );

        let InputSide {
            consumer,
            format: _,
            level,
            stream: input_stream,
        } = input;
        let OutputSide {
            producer,
            format: output_format,
            stream: output_stream,
        } = output;
        let recorder = SessionRecorder::from_env(
            direction.name(),
            output_format.sample_rate,
            output_format.channels,
        );
        let shared = Arc::new(Shared::new(direction, recorder));
        let gain = ducking.as_ref().map(|ducking| ducking.gain());
        if let Some(gain) = &gain {
            shared.record_event(json!({ "kind": "ducking", "gain": gain.get() }));
        }
        *lock(&shared.ducking) = ducking;

        *lock(&shared.session_input) = Some(session.input());

        events(PipelineEvent::Status(PipelineStatus::live()));
        let pump = Pump::spawn(PumpParts {
            input: consumer,
            gain,
            to_translator,
            output: producer,
            output_format,
            from_translator,
            shared: Arc::clone(&shared),
            level,
            events: Arc::clone(&events),
        });
        let pump = match pump {
            Ok(pump) => pump,
            Err(error) => {
                drop(session);
                stop_streams(vec![input_stream, output_stream]).await;
                let ducked = Arc::clone(&shared);
                let _ = tokio::task::spawn_blocking(move || ducked.stop_ducking()).await;
                return Err(error);
            }
        };

        let stop = Arc::new(StopSignal::default());
        let supervisor = Supervisor {
            session: Some(session),
            translator,
            target_language: config.target_language,
            policy: config.reconnect,
            attempts: 0,
            connected_at: tokio::time::Instant::now(),
            input_stream,
            output_stream,
            pump,
            shared: Arc::clone(&shared),
            events,
            last_error: None,
            silence_run_samples: 0,
            close_abandoned: false,
            dropped_chunks: 0,
        };
        let task = tokio::spawn(supervisor.run(Arc::clone(&stop)));
        Ok(Self {
            stop,
            task: Some(task),
            shared,
        })
    }

    /// Which way this pipeline translates.
    pub fn direction(&self) -> Direction {
        self.shared.direction
    }

    /// Changes the volume the captured apps are ducked to, while live (incoming only).
    pub fn set_original_volume(&self, percent: u8) {
        if let Some(ducking) = lock(&self.shared.ducking).as_ref() {
            ducking.set_original_volume(percent);
        }
    }

    /// Restores the ducked apps now, without waiting for the pipeline to stop (the app is
    /// exiting). Blocking.
    pub fn release_ducking(&self) {
        self.shared.stop_ducking();
    }

    /// Whether the pipeline has ended (stopped on its own after a fatal error).
    pub fn is_finished(&self) -> bool {
        self.task.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Stops the pipeline (gracefully: see the module docs) and waits until every device is
    /// released and the translator closed.
    pub async fn stop(self) {
        self.stop.request(StopMode::Graceful);
        self.wait().await;
    }

    /// Stops the pipeline at once (see the module docs: no tail) and waits until every device
    /// is released and the translator closed. For when the app exits.
    pub async fn shutdown(self) {
        self.stop.request(StopMode::Fast);
        self.wait().await;
    }

    /// A handle that can turn a stop into a fast one from elsewhere.
    pub fn stop_handle(&self) -> StopHandle {
        StopHandle(Arc::clone(&self.stop))
    }

    async fn wait(mut self) {
        if let Some(task) = self.task.take()
            && let Err(error) = task.await
        {
            tracing::error!(direction = self.shared.direction.name(), %error, "pipeline task ended abnormally");
        }
    }

    /// Latency per utterance so far (see [`super::latency`]).
    pub fn latency_report(&self) -> LatencyReport {
        lock(&self.shared.latency).report()
    }
}

impl Drop for TranslationPipeline {
    fn drop(&mut self) {
        // The supervisor tears down on its own once it sees the stop.
        self.stop.request(StopMode::Graceful);
    }
}

/// How a dropped session ended up.
enum Reconnected {
    /// A new session is live.
    Live,
    /// A stop was asked for while reconnecting.
    Stopped,
    /// The direction cannot go on.
    Failed(AppError),
}

/// What to do after an output.
enum Next {
    Continue,
    /// The session cannot go on.
    Fatal(AppError),
    /// `Closed`: the session ended.
    Closed,
}

/// Owns everything while the pipeline is live; see the module docs.
struct Supervisor {
    /// The live session; `None` while reconnecting.
    session: Option<LiveSession>,
    translator: Arc<dyn Translator>,
    target_language: Language,
    policy: ReconnectPolicy,
    /// Reconnect attempts used since the last stable session (see [`super::reconnect`]).
    attempts: u32,
    /// When the current session went live.
    connected_at: tokio::time::Instant,
    input_stream: Box<dyn StreamGuard>,
    output_stream: Box<dyn StreamGuard>,
    pump: Pump,
    shared: Arc<Shared>,
    events: EventSink,
    /// The latest non-fatal error, reported if the session then closes.
    last_error: Option<TranslatorError>,
    /// Digital silence received since the last audible delta.
    silence_run_samples: usize,
    /// The stop finished on [`TAIL_SILENCE_DONE`] without waiting for `session.closed`.
    close_abandoned: bool,
    /// Input chunks the sessions that dropped had to discard.
    dropped_chunks: u64,
}

impl Supervisor {
    async fn run(mut self, stop: Arc<StopSignal>) {
        let mut health = tokio::time::interval(HEALTH_INTERVAL);
        // `None` = stopped by the user (or the app).
        let failure: Option<AppError> = 'run: loop {
            let cause = loop {
                tokio::select! {
                    _ = stop.reached(StopMode::Graceful) => break 'run None,
                    _ = health.tick() => {
                        if let Some(failure) = self.device_failure() {
                            break 'run Some(AppError::Audio(failure));
                        }
                    }
                    output = next_output(&mut self.session) => match self.on_output(output) {
                        Next::Continue => {}
                        Next::Fatal(error) => break 'run Some(error),
                        Next::Closed => {
                            break self.last_error.take().unwrap_or(TranslatorError::Closed);
                        }
                    }
                }
            };
            match self.reconnect(&stop, &mut health, cause).await {
                Reconnected::Live => {}
                Reconnected::Stopped => break None,
                Reconnected::Failed(error) => break Some(error),
            }
        };
        let fast = || stop.mode() == Some(StopMode::Fast);
        let drained = match failure {
            None if !fast() && self.session.is_some() => self.drain(&stop).await,
            _ => false,
        };
        // Checked again: a graceful stop may have turned fast while draining.
        self.shutdown(failure, drained, fast()).await;
    }

    /// The session dropped after `cause`: keeps the devices open and opens a new session with
    /// backoff (see the module docs).
    async fn reconnect(
        &mut self,
        stop: &StopSignal,
        health: &mut tokio::time::Interval,
        cause: TranslatorError,
    ) -> Reconnected {
        let direction = self.shared.direction.name();
        // The audio said while disconnected is discarded, not queued.
        *lock(&self.shared.session_input) = None;
        if let Some(session) = self.session.take() {
            self.dropped_chunks += session.dropped_chunks();
        }
        if self.connected_at.elapsed() >= self.policy.stable_after {
            self.attempts = 0;
        }
        tracing::warn!(direction, code = cause.code(), %cause, "translation session dropped");
        self.shared
            .record_event(json!({ "kind": "session_dropped", "code": cause.code() }));

        let events = Arc::clone(&self.events);
        let max_attempts = self.policy.max_attempts;
        let attempt = reconnect(
            Arc::clone(&self.translator),
            self.target_language,
            self.policy,
            self.attempts + 1,
            cause,
            move |attempt| {
                events(PipelineEvent::Status(PipelineStatus::reconnecting(
                    attempt,
                    max_attempts,
                )));
            },
        );
        tokio::pin!(attempt);
        loop {
            tokio::select! {
                _ = stop.reached(StopMode::Graceful) => return Reconnected::Stopped,
                _ = health.tick() => {
                    if let Some(failure) = self.device_failure() {
                        return Reconnected::Failed(AppError::Audio(failure));
                    }
                }
                result = &mut attempt => return match result {
                    Ok((session, attempt)) => {
                        self.attempts = attempt;
                        self.connected_at = tokio::time::Instant::now();
                        self.last_error = None;
                        self.silence_run_samples = 0;
                        *lock(&self.shared.session_input) = Some(session.input());
                        self.session = Some(session);
                        self.shared
                            .record_event(json!({ "kind": "reconnected", "attempt": attempt }));
                        (self.events)(PipelineEvent::Status(PipelineStatus::live()));
                        Reconnected::Live
                    }
                    Err(ReconnectError::Fatal(error)) => Reconnected::Failed(error.into()),
                    Err(ReconnectError::Exhausted { attempts, last }) => {
                        Reconnected::Failed(AppError::ReconnectFailed { attempts, last })
                    }
                },
            }
        }
    }

    fn device_failure(&self) -> Option<String> {
        self.input_stream
            .failure()
            .or_else(|| self.output_stream.failure())
    }

    fn on_output(&mut self, output: Option<TranslatorOutput>) -> Next {
        let shared = &self.shared;
        match output {
            Some(TranslatorOutput::Audio(delta)) => {
                let at = shared.elapsed();
                let silent = is_digital_silence(&delta.samples);
                let (kept, buffered_ms) = {
                    let mut jitter = lock(&shared.jitter);
                    let kept = jitter.push_delta(&delta.samples);
                    (kept, jitter.buffered().as_millis() as u64)
                };
                shared.output_deltas.fetch_add(1, Ordering::Relaxed);
                if silent {
                    self.silence_run_samples += delta.samples.len();
                } else {
                    self.silence_run_samples = 0;
                }
                // The model streams silence too: only audible deltas say "translation arrived".
                let latency_events = if silent {
                    Vec::new()
                } else {
                    lock(&shared.latency).on_output(at)
                };
                shared.record(|recorder| recorder.received(&delta, at, kept, buffered_ms));
                for event in latency_events {
                    shared.latency_event(event);
                }
                Next::Continue
            }
            Some(TranslatorOutput::OutputTranscriptDelta(text)) => {
                shared.record_event(json!({ "kind": "output_transcript", "text": text }));
                (self.events)(PipelineEvent::TranscriptDelta(text));
                Next::Continue
            }
            Some(TranslatorOutput::InputTranscriptDelta(text)) => {
                shared.record_event(json!({ "kind": "input_transcript", "text": text }));
                Next::Continue
            }
            Some(TranslatorOutput::Error { code, message }) => {
                let error = TranslatorError::from_api_error(code, &message);
                shared.record_event(json!({ "kind": "error", "code": error.code() }));
                if !error.is_retryable() {
                    return Next::Fatal(error.into());
                }
                tracing::warn!(code = error.code(), %error, "translator reported an error");
                self.last_error = Some(error);
                Next::Continue
            }
            Some(TranslatorOutput::Closed) | None => Next::Closed,
        }
    }

    /// The graceful part of a stop asked by the user. Returns whether everything was played
    /// within [`STOP_DRAIN_LIMIT`].
    async fn drain(&mut self, stop: &StopSignal) -> bool {
        (self.events)(PipelineEvent::Status(PipelineStatus::stopping()));
        self.shared
            .record_event(json!({ "kind": "stop_requested" }));
        tracing::info!(
            direction = self.shared.direction.name(),
            "translation stopping: finishing the last sentence"
        );
        let deadline = tokio::time::Instant::now() + STOP_DRAIN_LIMIT;
        self.shared.trailing.store(true, Ordering::Relaxed);
        let mut poll = tokio::time::interval(Duration::from_millis(20));

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum Phase {
            /// Sending the trailing silence.
            Trailing,
            /// `session.close` sent, reading outputs until `session.closed`.
            Closing,
            /// Playing the jitter buffer's tail into the output.
            Playing,
        }
        let mut phase = Phase::Trailing;
        let tail_silence_done = samples_for_ms(TAIL_SILENCE_DONE.as_millis() as u32);
        loop {
            if phase == Phase::Trailing
                && (self.shared.input_done.load(Ordering::Relaxed)
                    || self.input_stream.failure().is_some())
            {
                if let Some(session) = &self.session {
                    session.stop();
                }
                self.shared
                    .record_event(json!({ "kind": "session_close_sent" }));
                // Only silence the server sends after the close counts as the end.
                self.silence_run_samples = 0;
                phase = Phase::Closing;
            }
            if phase == Phase::Playing && self.shared.output_drained.load(Ordering::Relaxed) {
                self.shared
                    .record_event(json!({ "kind": "output_drained" }));
                return true;
            }
            if self.output_stream.failure().is_some() {
                return false;
            }
            tokio::select! {
                _ = stop.reached(StopMode::Fast) => {
                    tracing::info!(?phase, "stopping at once: not playing the rest of the tail");
                    self.shared.record_event(json!({ "kind": "stop_now", "phase": format!("{phase:?}") }));
                    return false;
                }
                () = tokio::time::sleep_until(deadline) => {
                    tracing::warn!(
                        ?phase,
                        limit_s = STOP_DRAIN_LIMIT.as_secs(),
                        "the translation did not finish in time; stopping anyway"
                    );
                    self.shared.record_event(json!({ "kind": "stop_timeout", "phase": format!("{phase:?}") }));
                    return false;
                }
                _ = poll.tick() => {}
                output = next_output(&mut self.session), if phase != Phase::Playing => {
                    match self.on_output(output) {
                        Next::Continue => {
                            if phase == Phase::Closing
                                && self.silence_run_samples >= tail_silence_done
                            {
                                // The server is only flushing silence now: the translation is
                                // over, no need to wait for `session.closed`.
                                tracing::info!(
                                    silence_ms = self.silence_run_samples as u64 * 1000
                                        / u64::from(SAMPLE_RATE),
                                    "translation tail is silent; not waiting for session.closed"
                                );
                                self.shared.record_event(json!({ "kind": "tail_silent" }));
                                self.close_abandoned = true;
                                lock(&self.shared.jitter).finish_input();
                                phase = Phase::Playing;
                            }
                        }
                        Next::Fatal(error) => {
                            tracing::warn!(code = error.code(), %error, "translator error while stopping");
                            lock(&self.shared.jitter).finish_input();
                            phase = Phase::Playing;
                        }
                        Next::Closed => {
                            self.shared.record_event(json!({ "kind": "session_closed" }));
                            // Closed before the trailing silence ended: nothing more to send.
                            self.shared.input_done.store(true, Ordering::Relaxed);
                            lock(&self.shared.jitter).finish_input();
                            phase = Phase::Playing;
                        }
                    }
                }
            }
        }
    }

    /// Tears down in order, logs the session summary and emits the final status.
    async fn shutdown(self, failure: Option<AppError>, drained: bool, fast: bool) {
        let Self {
            session,
            input_stream,
            output_stream,
            mut pump,
            shared,
            events,
            close_abandoned,
            dropped_chunks,
            ..
        } = self;

        // No more input audio.
        *lock(&shared.session_input) = None;
        stop_streams(vec![input_stream]).await;
        let mut dropped_chunks = dropped_chunks;
        match session {
            None => {}
            Some(session) if close_abandoned => {
                // Only silence was left to read: dropping the session ends it.
                dropped_chunks += session.dropped_chunks();
                drop(session);
            }
            Some(session) => {
                dropped_chunks += session.dropped_chunks();
                // After a drained stop the session is already closed and this returns at once.
                // `finish` sends `session.close` first.
                let close_wait = if drained || fast {
                    FAST_CLOSE_TIMEOUT
                } else {
                    CLOSE_TIMEOUT
                };
                if tokio::time::timeout(close_wait, session.finish())
                    .await
                    .is_err()
                {
                    tracing::warn!("the translator did not close in time");
                }
            }
        }
        // The pump reports the final audio sent, then the output stops and, once the translated
        // tail has played, the ducked apps get their volume back.
        let teardown = Arc::clone(&shared);
        let _ = tokio::task::spawn_blocking(move || {
            pump.stop();
            let mut output_stream = output_stream;
            output_stream.stop();
            teardown.stop_ducking();
        })
        .await;

        log_summary(&shared, dropped_chunks);
        let direction = shared.direction.name();

        events(PipelineEvent::Level(InputLevel::default()));
        match failure {
            None => {
                tracing::info!(direction, "translation stopped");
                events(PipelineEvent::Status(PipelineStatus::idle()));
            }
            Some(error) => {
                tracing::warn!(direction, code = error.code(), %error, "translation stopped on an error");
                events(PipelineEvent::Status(PipelineStatus::error(&error)));
            }
        }
    }
}

/// Logs the one-line session summary (and records it), then finishes the recording.
fn log_summary(shared: &Shared, dropped_chunks: u64) {
    let direction = shared.direction.name();
    let jitter = lock(&shared.jitter).stats();
    let sent = shared.sent_samples.load(Ordering::Relaxed);
    let deltas = shared.output_deltas.load(Ordering::Relaxed);
    let seconds = |samples: u64| (samples as f64 / f64::from(SAMPLE_RATE) * 10.0).round() / 10.0;
    tracing::info!(
        jitter_underruns = jitter.underruns,
        jitter_dropped_samples = jitter.dropped_samples,
        jitter_max_buffered_ms = jitter.max_buffered_ms(),
        utterances_played = jitter.utterances,
        output_deltas = deltas,
        sent_s = seconds(sent),
        sent_bytes = sent * 2,
        received_s = seconds(jitter.pushed_samples),
        received_bytes = jitter.pushed_samples * 2,
        skipped_silence_s = seconds(jitter.skipped_silence_samples),
        input_dropped_chunks = dropped_chunks,
        direction,
        "session summary"
    );
    let latency = lock(&shared.latency).report();
    tracing::info!(direction, "latency: {latency}");
    let input = lock(&shared.health).summary();
    tracing::info!(
        direction,
        peak_dbfs = input.peak_dbfs,
        rms_dbfs = input.rms_dbfs,
        speech_rms_dbfs = input.speech_rms_dbfs,
        clipped_samples = input.clipped_samples,
        channel_rms_dbfs = ?input.channel_rms_dbfs,
        "input summary"
    );
    // The warnings are about microphone settings; an app's audio is what it is.
    if shared.direction == Direction::Outgoing {
        for warning in &input.warnings {
            tracing::warn!("microphone: {warning}");
        }
    }

    let spread = |spread: Option<super::latency::Spread>| {
        spread.map(|s| {
            json!({
                "count": s.count,
                "min_ms": s.min.as_millis() as u64,
                "median_ms": s.median.as_millis() as u64,
                "max_ms": s.max.as_millis() as u64,
            })
        })
    };
    shared.record_event(json!({
        "kind": "summary",
        "direction": direction,
        "jitter_underruns": jitter.underruns,
        "jitter_dropped_samples": jitter.dropped_samples,
        "jitter_max_buffered_ms": jitter.max_buffered_ms(),
        "utterances_played": jitter.utterances,
        "output_deltas": deltas,
        "sent_s": seconds(sent),
        "sent_bytes": sent * 2,
        "received_s": seconds(jitter.pushed_samples),
        "received_bytes": jitter.pushed_samples * 2,
        "skipped_silence_s": seconds(jitter.skipped_silence_samples),
        "kept_s": seconds(jitter.pushed_samples - jitter.skipped_silence_samples),
        "input_dropped_chunks": dropped_chunks,
        "latency_start_to_audio": spread(latency.start_to_audio),
        "latency_end_to_audio": spread(latency.end_to_audio),
        "input_peak_dbfs": input.peak_dbfs,
        "input_rms_dbfs": input.rms_dbfs,
        "input_speech_rms_dbfs": input.speech_rms_dbfs,
        "input_clipped_samples": input.clipped_samples,
        "input_channel_rms_dbfs": input.channel_rms_dbfs,
        "input_warnings": input.warnings,
    }));
    if let Some(recorder) = lock(&shared.recorder).take() {
        recorder.finish();
    }
}

/// The next output of `session`; never ready while there is none (reconnecting).
async fn next_output(session: &mut Option<LiveSession>) -> Option<TranslatorOutput> {
    match session {
        Some(session) => session.next_output().await,
        None => std::future::pending().await,
    }
}

/// What the pump thread owns.
struct PumpParts {
    input: AudioConsumer,
    /// Ducking compensation for the captured audio (incoming), see the module docs.
    gain: Option<SharedGain>,
    to_translator: ToTranslator,
    output: AudioProducer,
    output_format: StreamFormat,
    from_translator: FromTranslator,
    shared: Arc<Shared>,
    level: SharedLevel,
    events: EventSink,
}

struct Pump {
    shared: Arc<Shared>,
    thread: Option<ThreadHandle<()>>,
}

impl Pump {
    fn spawn(parts: PumpParts) -> AppResult<Self> {
        let shared = Arc::clone(&parts.shared);
        let thread = std::thread::Builder::new()
            .name(format!("livedub-{}", parts.shared.direction.name()))
            .spawn(move || run_pump(parts))
            .map_err(|error| {
                AppError::Internal(format!("could not start the pipeline thread: {error}"))
            })?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }

    fn stop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!(
                direction = self.shared.direction.name(),
                "pipeline pump thread panicked"
            );
        }
    }
}

impl Drop for Pump {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run_pump(mut parts: PumpParts) {
    let shared = Arc::clone(&parts.shared);
    let channels = parts.input.channels();
    let mut captured = vec![0.0_f32; 1024 * channels];
    let mut pcm: Vec<i16> = Vec::with_capacity(samples_for_ms(100));
    let mut translated = vec![0_i16; OUTPUT_CHUNK_SAMPLES];
    let mut device: Vec<f32> = Vec::with_capacity(
        parts.output_format.frames_for_ms(20) * usize::from(parts.output_format.channels),
    );
    let target_frames = parts.output_format.frames_for_ms(OUTPUT_TARGET_MS);
    let level_interval = Duration::from_millis(u64::from(LEVEL_WINDOW_MS));
    let trailing_samples = samples_for_ms(TRAILING_SILENCE.as_millis() as u32) as u64;
    let mut trailing_sent: u64 = 0;
    let mut sent_samples: u64 = 0;
    let mut rate = 1.0;
    let mut drained_since: Option<Instant> = None;
    let mut next_level = Instant::now();
    let mut next_sent = Instant::now() + AUDIO_SENT_INTERVAL;
    let seconds = |samples: u64| samples as f64 / f64::from(SAMPLE_RATE);

    while !shared.stop.load(Ordering::Relaxed) {
        // Input -> translator.
        let trailing = shared.trailing.load(Ordering::Relaxed);
        let gain = parts.gain.as_ref().map_or(1.0, SharedGain::get);
        loop {
            let read = parts.input.pop(&mut captured);
            if read == 0 {
                break;
            }
            apply_gain(&mut captured[..read], gain);
            if !trailing {
                lock(&shared.health).captured(&captured[..read], channels);
            }
            parts.to_translator.process(&captured[..read], &mut pcm);
        }
        if !pcm.is_empty() && !shared.input_done.load(Ordering::Relaxed) {
            if trailing {
                // The user stopped: silence, paced by the input's clock, so the model
                // finishes the last sentence.
                let take = (trailing_samples - trailing_sent).min(pcm.len() as u64) as usize;
                pcm.truncate(take);
                pcm.fill(0);
                trailing_sent += take as u64;
            }
            let block_rms = rms(&pcm);
            let at = shared.elapsed();
            if !trailing {
                lock(&shared.health).sent(&pcm, block_rms >= SPEECH_RMS);
            }
            // While reconnecting there is no session: the audio is discarded.
            let pushed = lock(&shared.session_input)
                .as_ref()
                .is_some_and(|input| input.push(&pcm));
            if pushed {
                let latency_event = lock(&shared.latency).on_input(block_rms, at);
                if let Some(event) = latency_event {
                    shared.latency_event(event);
                }
                sent_samples += pcm.len() as u64;
                shared.sent_samples.store(sent_samples, Ordering::Relaxed);
                shared.record(|recorder| recorder.sent(&pcm, at));
            }
            if trailing && trailing_sent >= trailing_samples {
                shared.input_done.store(true, Ordering::Relaxed);
            }
        }
        pcm.clear();

        // Translator -> output, at the output's pace.
        let mut drained = false;
        for _ in 0..MAX_FILLS_PER_TICK {
            if parts.output.queued_frames() >= target_frames {
                break;
            }
            let wanted = {
                let mut jitter = lock(&shared.jitter);
                jitter.pull(&mut translated);
                drained = jitter.is_drained();
                drift_adjust(&jitter)
            };
            if (wanted - rate).abs() > 0.000_5 {
                rate = parts.from_translator.set_rate_adjust(wanted);
            }
            parts.from_translator.process(&translated, &mut device);
            parts.output.push(&device);
            shared.record(|recorder| recorder.output(&device));
            device.clear();
        }

        let now = Instant::now();
        if drained {
            // Wait for the playback queue to reach the output device too.
            let since = *drained_since.get_or_insert(now);
            if now.duration_since(since) >= OUTPUT_DRAIN_TAIL {
                shared.output_drained.store(true, Ordering::Relaxed);
            }
        }
        if now >= next_level {
            (parts.events)(PipelineEvent::Level(compensated_level(
                parts.level.get(),
                gain,
            )));
            next_level = now + level_interval;
        }
        if now >= next_sent {
            (parts.events)(PipelineEvent::AudioSent {
                seconds: seconds(sent_samples),
            });
            let (stats, buffered_ms) = {
                let jitter = lock(&shared.jitter);
                (jitter.stats(), jitter.buffered().as_millis() as u64)
            };
            let (queued_chunks, dropped_chunks) = lock(&shared.session_input)
                .as_ref()
                .map_or((0, 0), |input| {
                    (input.queued_chunks(), input.dropped_chunks())
                });
            shared.record_event(json!({
                "kind": "jitter",
                "buffered_ms": buffered_ms,
                "underruns": stats.underruns,
                "dropped_samples": stats.dropped_samples,
                "utterances": stats.utterances,
                "rate": (rate * 10_000.0).round() / 10_000.0,
                "input_queued_chunks": queued_chunks,
                "input_dropped_chunks": dropped_chunks,
            }));
            next_sent = now + AUDIO_SENT_INTERVAL;
        }
        std::thread::sleep(TICK);
    }
    (parts.events)(PipelineEvent::AudioSent {
        seconds: seconds(sent_samples),
    });
}

/// Output rate factor for the jitter buffer's backlog (audio queued above the prebuffer
/// target): 1.0 at or below the target; then down to `1 - MAX_RATE_ADJUST` at
/// [`DRIFT_RANGE_MS`] of backlog (clock drift, inaudible); then down to `1 - MAX_CATCH_UP` at
/// [`CATCH_UP_FULL_MS`] (a burst of translated speech waiting: play it slightly faster rather
/// than fall seconds behind). Below 1.0 each translated sample becomes fewer output frames.
fn drift_adjust(jitter: &JitterBuffer) -> f64 {
    if !jitter.is_playing() {
        return 1.0;
    }
    let excess_samples = jitter
        .buffered_samples()
        .saturating_sub(jitter.target_samples());
    let excess_ms = excess_samples as f64 * 1000.0 / f64::from(SAMPLE_RATE);
    let speed_up = if excess_ms <= DRIFT_RANGE_MS {
        MAX_RATE_ADJUST * excess_ms / DRIFT_RANGE_MS
    } else {
        let catch_up =
            ((excess_ms - DRIFT_RANGE_MS) / (CATCH_UP_FULL_MS - DRIFT_RANGE_MS)).min(1.0);
        MAX_RATE_ADJUST + (MAX_CATCH_UP - MAX_RATE_ADJUST) * catch_up
    };
    1.0 - speed_up
}

/// Multiplies captured samples by the ducking compensation `gain` (1 = untouched). Samples are
/// `f32`, so no precision is lost; anything above full scale is clipped when converted to PCM16.
fn apply_gain(samples: &mut [f32], gain: f32) {
    if gain != 1.0 {
        for sample in samples {
            *sample *= gain;
        }
    }
}

/// The capture's level as the translator hears it (after the ducking compensation `gain`),
/// within full scale.
fn compensated_level(level: InputLevel, gain: f32) -> InputLevel {
    InputLevel {
        rms: (level.rms * gain).min(1.0),
        peak: (level.peak * gain).min(1.0),
    }
}

/// RMS of PCM16 samples, linear 0–1.
fn rms(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples
        .iter()
        .map(|&sample| {
            let value = f64::from(sample) / 32_768.0;
            value * value
        })
        .sum();
    (sum / samples.len() as f64).sqrt() as f32
}

/// Runs blocking device work off the async runtime.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| AppError::Internal(format!("audio task failed: {error}")))?
}

/// What a pipeline captures, between finding it and starting the capture.
enum PendingInput {
    /// The microphone, already capturing.
    Opened(InputSide),
    /// The apps to capture, once ducked.
    Apps(LoopbackTarget),
}

/// Restores ducked apps off the async runtime.
async fn stop_ducking(ducking: Option<Box<dyn DuckGuard>>) {
    if let Some(mut ducking) = ducking {
        let _ = tokio::task::spawn_blocking(move || ducking.stop()).await;
    }
}

/// Stops streams (joining their threads) off the async runtime.
async fn stop_streams(mut streams: Vec<Box<dyn StreamGuard>>) {
    let result = tokio::task::spawn_blocking(move || {
        for stream in &mut streams {
            stream.stop();
        }
    })
    .await;
    if let Err(error) = result {
        tracing::error!(%error, "stopping an audio stream failed");
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic while holding a lock leaves at worst a stale buffer behind: keep going.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc;

    use super::*;
    use crate::pipeline::PipelineState;
    use crate::pipeline::testing::{
        MemoryIo, Plan, ScriptedTranslator, TestTranslator, discord, incoming_config_from,
        outgoing_config, quick_reconnect, recorder, states, statuses,
    };
    use crate::translator::FakeTranslator;
    use crate::translator::live::input_channel;

    async fn wait_until_finished(pipeline: &TranslationPipeline) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !pipeline.is_finished() {
            assert!(Instant::now() < deadline, "the pipeline did not stop");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    #[tokio::test]
    async fn translates_the_microphone_into_the_output_and_stops_cleanly() {
        let io = Arc::new(MemoryIo::new());
        let (events, recorded) = recorder();
        let translator = Arc::new(FakeTranslator::echo(Duration::from_millis(100)));

        let pipeline =
            TranslationPipeline::start(outgoing_config(), translator, io.clone(), events)
                .await
                .unwrap();
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let latency = pipeline.latency_report();
        pipeline.stop().await;

        // The echoed tone reached the cable: at least 0.3 s of it, in the cable's format.
        let loud = io.loud_played_samples();
        assert!(loud > 44_100 * 2 * 3 / 10, "{loud} loud samples played");
        assert!(io.input_stopped.load(Ordering::Relaxed));
        assert!(io.output_stopped.load(Ordering::Relaxed));

        let events = recorded.lock().unwrap();
        assert_eq!(
            states(&events),
            [
                PipelineState::Connecting,
                PipelineState::Live,
                PipelineState::Stopping,
                PipelineState::Idle
            ]
        );
        assert_eq!(
            events.first(),
            Some(&PipelineEvent::Status(PipelineStatus::connecting()))
        );
        assert_eq!(
            events.last(),
            Some(&PipelineEvent::Status(PipelineStatus::idle()))
        );
        let live_at = events
            .iter()
            .position(|event| event == &PipelineEvent::Status(PipelineStatus::live()))
            .unwrap();
        let loud_levels = events[live_at..]
            .iter()
            .filter(|event| matches!(event, PipelineEvent::Level(level) if level.rms > 0.1))
            .count();
        assert!(loud_levels >= 10, "{loud_levels} loud level events");
        let sent = events
            .iter()
            .rev()
            .find_map(|event| match event {
                PipelineEvent::AudioSent { seconds } => Some(*seconds),
                _ => None,
            })
            .unwrap();
        // 1.5 s live plus 1.5 s of trailing silence.
        assert!((2.5..4.0).contains(&sent), "{sent} s sent");

        // The tone is one utterance: chunking (200 ms) plus the echo delay (100 ms), give or
        // take a few ticks.
        println!("latency: {latency}");
        assert_eq!(latency.utterances.len(), 1, "{latency:?}");
        let start_to_audio = latency.utterances[0].start_to_audio.unwrap();
        assert!(
            (Duration::from_millis(250)..Duration::from_millis(600)).contains(&start_to_audio),
            "{latency:?}"
        );
    }

    #[tokio::test]
    async fn forwards_captions_and_stops_on_a_fatal_translator_error() {
        let io = Arc::new(MemoryIo::new());
        let (events, recorded) = recorder();
        let translator = Arc::new(ScriptedTranslator {
            outputs: vec![
                TranslatorOutput::OutputTranscriptDelta("Hello".into()),
                TranslatorOutput::Error {
                    code: "invalid_api_key".into(),
                    message: "Incorrect API key provided: sk-abc".into(),
                },
            ],
            rejects_key: false,
        });

        let pipeline =
            TranslationPipeline::start(outgoing_config(), translator, io.clone(), events)
                .await
                .unwrap();
        wait_until_finished(&pipeline).await;
        pipeline.stop().await;

        assert!(io.input_stopped.load(Ordering::Relaxed));
        assert!(io.output_stopped.load(Ordering::Relaxed));
        let events = recorded.lock().unwrap();
        assert_eq!(
            states(&events),
            [
                PipelineState::Connecting,
                PipelineState::Live,
                PipelineState::Error
            ]
        );
        let caption_at = events
            .iter()
            .position(|event| event == &PipelineEvent::TranscriptDelta("Hello".into()))
            .unwrap();
        let live_at = events
            .iter()
            .position(|event| event == &PipelineEvent::Status(PipelineStatus::live()))
            .unwrap();
        assert!(live_at < caption_at);
        let PipelineEvent::Status(last) = events.last().unwrap() else {
            panic!("the last event is not a status: {:?}", events.last());
        };
        let error = last.error.as_ref().unwrap();
        assert_eq!(error.code, "openai_auth");
        assert!(!error.message.contains("sk-"));
    }

    #[tokio::test]
    async fn stops_when_the_microphone_fails() {
        let io = Arc::new(MemoryIo::new());
        let (events, recorded) = recorder();
        let translator = Arc::new(FakeTranslator::echo(Duration::from_millis(100)));

        let pipeline =
            TranslationPipeline::start(outgoing_config(), translator, io.clone(), events)
                .await
                .unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        io.input_failure
            .set("the microphone stopped: the device was unplugged or disabled".into())
            .unwrap();
        wait_until_finished(&pipeline).await;

        assert!(io.input_stopped.load(Ordering::Relaxed));
        assert!(io.output_stopped.load(Ordering::Relaxed));
        let events = recorded.lock().unwrap();
        let last = statuses(&events).pop().unwrap();
        assert_eq!(last.state, PipelineState::Error);
        assert_eq!(last.error.unwrap().code, "audio");
    }

    #[tokio::test]
    async fn start_fails_cleanly_when_the_translator_rejects_the_key() {
        let io = Arc::new(MemoryIo::new());
        let (events, recorded) = recorder();
        let translator = Arc::new(ScriptedTranslator {
            outputs: Vec::new(),
            rejects_key: true,
        });

        let error = TranslationPipeline::start(outgoing_config(), translator, io.clone(), events)
            .await
            .err()
            .unwrap();

        assert_eq!(error.code(), "openai_auth");
        assert!(io.input_stopped.load(Ordering::Relaxed));
        assert!(io.output_stopped.load(Ordering::Relaxed));
        let events = recorded.lock().unwrap();
        assert_eq!(
            states(&events),
            [PipelineState::Connecting, PipelineState::Error]
        );
    }

    #[tokio::test]
    async fn start_fails_without_opening_the_microphone_when_the_output_cannot_open() {
        let io = Arc::new(MemoryIo {
            output_opens: false,
            ..MemoryIo::new()
        });
        let (events, recorded) = recorder();
        let translator = Arc::new(FakeTranslator::echo(Duration::from_millis(100)));

        let error = TranslationPipeline::start(outgoing_config(), translator, io.clone(), events)
            .await
            .err()
            .unwrap();

        assert_eq!(error.code(), "audio");
        assert!(!io.input_opened.load(Ordering::Relaxed));
        let last = statuses(&recorded.lock().unwrap()).pop().unwrap();
        assert_eq!(last.state, PipelineState::Error);
    }

    fn network() -> TranslatorError {
        TranslatorError::Network("connection reset".into())
    }

    /// Waits until a status matching `wanted` was emitted.
    async fn wait_for_status(
        recorded: &Mutex<Vec<PipelineEvent>>,
        wanted: impl Fn(&PipelineStatus) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if statuses(&recorded.lock().unwrap()).iter().any(&wanted) {
                return;
            }
            assert!(Instant::now() < deadline, "the status never came");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    #[tokio::test]
    async fn reconnects_after_a_drop_keeping_the_devices_open() {
        let io = Arc::new(MemoryIo::new());
        let (events, recorded) = recorder();
        let translator = TestTranslator::new(vec![
            Plan::DropAfter(Duration::from_millis(500), network()),
            Plan::Fail(network()),
            Plan::Echo,
        ]);

        let pipeline =
            TranslationPipeline::start(outgoing_config(), translator.clone(), io.clone(), events)
                .await
                .unwrap();
        wait_for_status(&recorded, |status| {
            status
                .reconnect
                .is_some_and(|progress| progress.attempt == 2)
        })
        .await;
        let played_before = io.loud_played_samples();
        // Back live: attempt 2 (50 + 100 ms) opened the third session.
        tokio::time::sleep(Duration::from_millis(800)).await;
        let played_after = io.loud_played_samples();
        pipeline.stop().await;

        // The microphone and the cable were opened once and stayed open.
        assert_eq!(io.streams_opened(), 2);
        assert_eq!(io.streams_alive(), 0);
        assert_eq!(translator.starts(), 3);
        assert_eq!(translator.live_sessions(), 0);
        // The translation resumed after the reconnect.
        assert!(
            played_after > played_before + 44_100 * 2 / 5,
            "{played_before} -> {played_after} loud samples"
        );
        let statuses = statuses(&recorded.lock().unwrap());
        assert_eq!(
            statuses,
            [
                PipelineStatus::connecting(),
                PipelineStatus::live(),
                PipelineStatus::reconnecting(1, 3),
                PipelineStatus::reconnecting(2, 3),
                PipelineStatus::live(),
                PipelineStatus::stopping(),
                PipelineStatus::idle(),
            ]
        );
    }

    #[tokio::test]
    async fn stops_with_reconnect_failed_when_the_attempts_run_out() {
        let io = Arc::new(MemoryIo::new());
        let (events, recorded) = recorder();
        let translator = TestTranslator::new(vec![
            Plan::DropAfter(Duration::from_millis(300), network()),
            Plan::Fail(network()),
            Plan::Fail(TranslatorError::Server("HTTP 503".into())),
            Plan::Fail(network()),
        ]);

        let pipeline = TranslationPipeline::start(
            incoming_config_from(discord()),
            translator.clone(),
            io.clone(),
            events,
        )
        .await
        .unwrap();
        wait_until_finished(&pipeline).await;
        pipeline.stop().await;

        assert_eq!(translator.starts(), 4);
        assert_eq!(io.streams_alive(), 0);
        // Ducked once for the whole time, restored at the end.
        assert_eq!(io.ducking.lock().unwrap().started.len(), 1);
        assert_eq!(io.app_volume(), 1.0);
        let statuses = statuses(&recorded.lock().unwrap());
        let last = statuses.last().unwrap();
        assert_eq!(last.state, PipelineState::Error);
        let error = last.error.as_ref().unwrap();
        assert_eq!(error.code, "reconnect_failed");
        assert!(
            error.message.contains("after 3 attempts"),
            "{}",
            error.message
        );
        assert_eq!(
            statuses
                .iter()
                .filter_map(|status| status.reconnect)
                .map(|progress| progress.attempt)
                .collect::<Vec<_>>(),
            [1, 2, 3]
        );
    }

    #[tokio::test]
    async fn a_fatal_error_while_reconnecting_ends_the_direction() {
        let io = Arc::new(MemoryIo::new());
        let (events, recorded) = recorder();
        let translator = TestTranslator::new(vec![
            Plan::DropAfter(Duration::from_millis(300), network()),
            Plan::Fail(TranslatorError::QuotaExceeded),
        ]);

        let pipeline =
            TranslationPipeline::start(outgoing_config(), translator.clone(), io.clone(), events)
                .await
                .unwrap();
        wait_until_finished(&pipeline).await;
        pipeline.stop().await;

        assert_eq!(translator.starts(), 2);
        assert_eq!(io.streams_alive(), 0);
        let last = statuses(&recorded.lock().unwrap()).pop().unwrap();
        assert_eq!(last.error.unwrap().code, "openai_quota");
    }

    #[tokio::test]
    async fn a_fatal_error_mid_session_is_not_retried() {
        let io = Arc::new(MemoryIo::new());
        let (events, recorded) = recorder();
        let translator = TestTranslator::new(vec![Plan::DropAfter(
            Duration::from_millis(300),
            TranslatorError::Auth,
        )]);

        let pipeline =
            TranslationPipeline::start(outgoing_config(), translator.clone(), io.clone(), events)
                .await
                .unwrap();
        wait_until_finished(&pipeline).await;
        pipeline.stop().await;

        assert_eq!(translator.starts(), 1);
        assert_eq!(
            states(&recorded.lock().unwrap()),
            [
                PipelineState::Connecting,
                PipelineState::Live,
                PipelineState::Error
            ]
        );
    }

    #[tokio::test]
    async fn a_rate_limit_error_mid_session_keeps_translating() {
        let io = Arc::new(MemoryIo::new());
        let (events, recorded) = recorder();
        let translator = Arc::new(ScriptedTranslator {
            outputs: vec![TranslatorOutput::Error {
                code: "rate_limit_exceeded".into(),
                message: "Rate limit reached. Please try again in 2s.".into(),
            }],
            rejects_key: false,
        });

        let pipeline =
            TranslationPipeline::start(outgoing_config(), translator, io.clone(), events)
                .await
                .unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(!pipeline.is_finished());
        pipeline.shutdown().await;

        assert_eq!(
            states(&recorded.lock().unwrap()),
            [
                PipelineState::Connecting,
                PipelineState::Live,
                PipelineState::Idle
            ]
        );
    }

    #[tokio::test]
    async fn a_stop_while_reconnecting_is_immediate() {
        let io = Arc::new(MemoryIo::new());
        let (events, recorded) = recorder();
        let translator = TestTranslator::new(vec![
            Plan::DropAfter(Duration::from_millis(200), network()),
            Plan::Fail(network()),
        ]);
        let config = PipelineConfig {
            reconnect: ReconnectPolicy {
                base_delay: Duration::from_secs(5),
                ..quick_reconnect(3)
            },
            ..outgoing_config()
        };

        let pipeline = TranslationPipeline::start(config, translator.clone(), io.clone(), events)
            .await
            .unwrap();
        wait_for_status(&recorded, |status| status.reconnect.is_some()).await;
        let stopping = Instant::now();
        pipeline.stop().await;

        // No session to finish: no trailing silence, no 5 s wait.
        assert!(stopping.elapsed() < Duration::from_millis(500));
        assert_eq!(translator.starts(), 1);
        assert_eq!(translator.live_sessions(), 0);
        assert_eq!(io.streams_alive(), 0);
        assert_eq!(
            states(&recorded.lock().unwrap()),
            [
                PipelineState::Connecting,
                PipelineState::Live,
                PipelineState::Reconnecting,
                PipelineState::Idle
            ]
        );
    }

    #[tokio::test]
    async fn the_attempt_count_carries_over_a_session_that_did_not_last() {
        let io = Arc::new(MemoryIo::new());
        let (events, recorded) = recorder();
        // Every session drops quickly: the count never starts over, so it runs out.
        let translator = TestTranslator::new(vec![
            Plan::DropAfter(Duration::from_millis(200), network()),
            Plan::DropAfter(Duration::from_millis(200), network()),
            Plan::DropAfter(Duration::from_millis(200), network()),
            Plan::DropAfter(Duration::from_millis(200), network()),
        ]);

        let pipeline =
            TranslationPipeline::start(outgoing_config(), translator.clone(), io.clone(), events)
                .await
                .unwrap();
        wait_until_finished(&pipeline).await;
        pipeline.stop().await;

        // The first session and three reconnects (attempts 1, 2, 3), then no attempt left.
        assert_eq!(translator.starts(), 4);
        let statuses = statuses(&recorded.lock().unwrap());
        assert_eq!(
            statuses
                .iter()
                .filter_map(|status| status.reconnect)
                .map(|progress| progress.attempt)
                .collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert_eq!(
            statuses.last().unwrap().error.as_ref().unwrap().code,
            "reconnect_failed"
        );
    }

    #[tokio::test]
    async fn shutdown_skips_the_tail() {
        let io = Arc::new(MemoryIo::new());
        let (events, recorded) = recorder();
        let translator = Arc::new(TailTranslator {
            tail_delay: Duration::from_millis(800),
        });

        let pipeline =
            TranslationPipeline::start(outgoing_config(), translator, io.clone(), events)
                .await
                .unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        let stopping = Instant::now();
        pipeline.shutdown().await;
        let took = stopping.elapsed();

        // `session.close` and at most FAST_CLOSE_TIMEOUT for the session to end.
        assert!(
            took < FAST_CLOSE_TIMEOUT + Duration::from_millis(400),
            "{took:?}"
        );
        assert_eq!(io.streams_alive(), 0);
        assert_eq!(
            states(&recorded.lock().unwrap()),
            [
                PipelineState::Connecting,
                PipelineState::Live,
                PipelineState::Idle
            ]
        );
    }

    #[tokio::test]
    async fn stop_now_cuts_a_graceful_stop_short() {
        let io = Arc::new(MemoryIo::new());
        let (events, recorded) = recorder();
        let translator = Arc::new(FakeTranslator::echo(Duration::from_millis(100)));

        let pipeline =
            TranslationPipeline::start(outgoing_config(), translator, io.clone(), events)
                .await
                .unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        let handle = pipeline.stop_handle();
        let stopping = Instant::now();
        let stop = tokio::spawn(pipeline.stop());
        tokio::time::sleep(Duration::from_millis(200)).await;
        handle.stop_now();
        stop.await.unwrap();

        // Well before the 1.5 s of trailing silence would have been sent.
        assert!(stopping.elapsed() < Duration::from_millis(1200));
        assert_eq!(io.streams_alive(), 0);
        assert_eq!(
            states(&recorded.lock().unwrap()),
            [
                PipelineState::Connecting,
                PipelineState::Live,
                PipelineState::Stopping,
                PipelineState::Idle
            ]
        );
    }

    #[test]
    fn drift_adjust_drains_a_growing_backlog() {
        let ms = |value: usize| value * SAMPLE_RATE as usize / 1000;
        let mut jitter = JitterBuffer::default();
        assert_eq!(drift_adjust(&jitter), 1.0);
        let target = jitter.target_samples();
        jitter.push(&vec![0; target]);
        assert_eq!(drift_adjust(&jitter), 1.0);
        // Half a second of backlog: gentle drift correction.
        jitter.push(&vec![0; ms(500)]);
        assert!((drift_adjust(&jitter) - (1.0 - MAX_RATE_ADJUST / 2.0)).abs() < 1e-6);
        // One second: the drift correction's limit.
        jitter.push(&vec![0; ms(500)]);
        assert!((drift_adjust(&jitter) - (1.0 - MAX_RATE_ADJUST)).abs() < 1e-6);
        // Seconds waiting: catching up, up to MAX_CATCH_UP.
        jitter.push(&vec![0; ms(1500)]);
        let halfway = 1.0 - (MAX_RATE_ADJUST + MAX_CATCH_UP) / 2.0;
        assert!((drift_adjust(&jitter) - halfway).abs() < 1e-6);
        jitter.push(&vec![0; ms(10_000)]);
        assert!((drift_adjust(&jitter) - (1.0 - MAX_CATCH_UP)).abs() < 1e-6);
    }

    /// A translator that only answers when the input closes: after `tail_delay` it sends one
    /// second of loud audio (the translation of the last sentence), then `Closed`.
    struct TailTranslator {
        tail_delay: Duration,
    }

    impl Translator for TailTranslator {
        fn name(&self) -> &'static str {
            "tail"
        }

        fn start(
            &self,
            _target_language: Language,
        ) -> futures_util::future::BoxFuture<'_, Result<LiveSession, TranslatorError>> {
            Box::pin(async move {
                let (input, reader) = input_channel(5);
                let (outputs_tx, outputs_rx) = mpsc::channel(16);
                let delay = self.tail_delay;
                let task = tokio::spawn(async move {
                    while reader.next_chunk().await.is_some() {}
                    tokio::time::sleep(delay).await;
                    let _ = outputs_tx
                        .send(TranslatorOutput::Audio(
                            vec![20_000; SAMPLE_RATE as usize].into(),
                        ))
                        .await;
                    let _ = outputs_tx.send(TranslatorOutput::Closed).await;
                });
                Ok(LiveSession::new(input, outputs_rx, task))
            })
        }
    }

    #[tokio::test]
    async fn graceful_stop_plays_the_tail_of_the_last_sentence() {
        let io = Arc::new(MemoryIo::new());
        let (events, recorded) = recorder();
        let translator = Arc::new(TailTranslator {
            tail_delay: Duration::from_millis(800),
        });

        let pipeline =
            TranslationPipeline::start(outgoing_config(), translator, io.clone(), events)
                .await
                .unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        let stopping = Instant::now();
        pipeline.stop().await;
        let took = stopping.elapsed();

        // 1.5 s of trailing silence + 0.8 s for the tail + 1 s of tail played, under the cap.
        assert!(took >= Duration::from_millis(2_800), "{took:?}");
        assert!(took < STOP_DRAIN_LIMIT, "{took:?}");
        // The whole second of tail reached the cable (44.1 kHz stereo), minus resampler edges.
        let tail = io
            .played
            .lock()
            .unwrap()
            .iter()
            .filter(|sample| sample.abs() > 0.5)
            .count();
        assert!(tail > 44_100 * 2 * 95 / 100, "{tail} tail samples played");
        assert_eq!(
            states(&recorded.lock().unwrap()),
            [
                PipelineState::Connecting,
                PipelineState::Live,
                PipelineState::Stopping,
                PipelineState::Idle
            ]
        );
    }

    /// Behaves like the real `gpt-realtime-translate` server: while the input is open it sends a
    /// 400 ms delta every 400 ms, digital silence included (deltas 2-4 are speech); after the
    /// close it flushes the tail (one speech delta) and 62 silent deltas (24.8 s) faster than
    /// real time, and sends `session.closed` only 10 s later.
    struct ServerLikeTranslator;

    const SERVER_DELTA: usize = 9600;

    impl Translator for ServerLikeTranslator {
        fn name(&self) -> &'static str {
            "server-like"
        }

        fn start(
            &self,
            _target_language: Language,
        ) -> futures_util::future::BoxFuture<'_, Result<LiveSession, TranslatorError>> {
            Box::pin(async move {
                let (input, reader) = input_channel(5);
                let (outputs_tx, outputs_rx) = mpsc::channel(16);
                let task = tokio::spawn(async move {
                    let delta =
                        |value: i16| TranslatorOutput::Audio(vec![value; SERVER_DELTA].into());
                    let mut ticker = tokio::time::interval(Duration::from_millis(400));
                    let mut sent = 0;
                    loop {
                        tokio::select! {
                            chunk = reader.next_chunk() => if chunk.is_none() { break },
                            _ = ticker.tick() => {
                                let value = if (2..5).contains(&sent) { 20_000 } else { 0 };
                                if outputs_tx.send(delta(value)).await.is_err() {
                                    return;
                                }
                                sent += 1;
                            }
                        }
                    }
                    if outputs_tx.send(delta(20_000)).await.is_err() {
                        return;
                    }
                    for _ in 0..62 {
                        if outputs_tx.send(delta(0)).await.is_err() {
                            return;
                        }
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    let _ = outputs_tx.send(TranslatorOutput::Closed).await;
                });
                Ok(LiveSession::new(input, outputs_rx, task))
            })
        }
    }

    #[tokio::test]
    async fn server_silence_neither_builds_a_backlog_nor_delays_the_stop() {
        let io = Arc::new(MemoryIo::new());
        let (events, recorded) = recorder();
        let pipeline = TranslationPipeline::start(
            outgoing_config(),
            Arc::new(ServerLikeTranslator),
            io.clone(),
            events,
        )
        .await
        .unwrap();
        let shared = Arc::clone(&pipeline.shared);
        tokio::time::sleep(Duration::from_millis(2_000)).await;
        let stopping = Instant::now();
        pipeline.stop().await;
        let took = stopping.elapsed();

        // 1.5 s of trailing silence, then the tail and a short pause, not the 10 s wait for
        // `session.closed` (nor the STOP_DRAIN_LIMIT cap).
        assert!(took < Duration::from_secs(4), "{took:?}");
        let stats = lock(&shared.jitter).stats();
        assert!(
            stats.max_buffered_ms() < 2_000,
            "max backlog {} ms",
            stats.max_buffered_ms()
        );
        assert!(stats.skipped_silence_samples > 0);
        assert_eq!(stats.dropped_samples, 0);
        // The three live speech deltas and the tail all reached the cable (44.1 kHz stereo).
        let speech = io
            .played
            .lock()
            .unwrap()
            .iter()
            .filter(|sample| sample.abs() > 0.5)
            .count();
        let expected = 4 * 44_100 * 2 * 400 / 1000;
        assert!(
            speech > expected * 95 / 100,
            "{speech} speech samples played"
        );
        assert_eq!(
            states(&recorded.lock().unwrap()),
            [
                PipelineState::Connecting,
                PipelineState::Live,
                PipelineState::Stopping,
                PipelineState::Idle
            ]
        );
    }

    #[test]
    fn ducking_compensation_scales_samples_and_level() {
        let mut samples = [0.06, -0.03, 0.0];
        apply_gain(&mut samples, 5.0);
        for (sample, expected) in samples.iter().zip([0.3, -0.15, 0.0]) {
            assert!((sample - expected).abs() < 1e-6, "{samples:?}");
        }

        let mut untouched = [0.5, -0.5];
        apply_gain(&mut untouched, 1.0);
        assert_eq!(untouched, [0.5, -0.5]);

        let level = compensated_level(
            InputLevel {
                rms: 0.04,
                peak: 0.3,
            },
            5.0,
        );
        assert!((level.rms - 0.2).abs() < 1e-6);
        // A session that was not ducked yet would go past full scale: shown at full scale.
        assert_eq!(level.peak, 1.0);
    }

    #[tokio::test]
    async fn incoming_translates_an_app_into_the_headphones_with_the_original_ducked() {
        let io = Arc::new(MemoryIo::new());
        let (events, recorded) = recorder();
        let translator = Arc::new(FakeTranslator::echo(Duration::from_millis(100)));

        let pipeline = TranslationPipeline::start(
            incoming_config_from(discord()),
            translator,
            io.clone(),
            events,
        )
        .await
        .unwrap();
        assert_eq!(pipeline.direction(), Direction::Incoming);
        tokio::time::sleep(Duration::from_millis(1000)).await;
        // The app plays at 20 % while live.
        assert!((io.app_volume() - 0.2).abs() < 1e-6);
        pipeline.set_original_volume(50);
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!((io.app_volume() - 0.5).abs() < 1e-6);
        pipeline.stop().await;

        // The app captured at 20 % then 50 % was compensated: the echoed tone reached the
        // headphones at full level (a 0.06 tone would never pass 0.1).
        let loud = io.loud_played_samples();
        assert!(loud > 44_100 * 2 * 3 / 10, "{loud} loud samples played");
        // Restored, after the translated tail played and the headphones stopped.
        assert_eq!(io.app_volume(), 1.0);
        let ducking = io.ducking.lock().unwrap();
        assert_eq!(ducking.started, [(LoopbackTarget::ProcessTree(4242), 20)]);
        assert_eq!(ducking.changed, [50]);
        assert_eq!(ducking.restored_after_output, Some(true));
        assert!(io.input_stopped.load(Ordering::Relaxed));

        let events = recorded.lock().unwrap();
        assert_eq!(
            states(&events),
            [
                PipelineState::Connecting,
                PipelineState::Live,
                PipelineState::Stopping,
                PipelineState::Idle
            ]
        );
        // The level is the app as the translator hears it, not ducked.
        let loud_levels = events
            .iter()
            .filter(|event| matches!(event, PipelineEvent::Level(level) if level.rms > 0.15))
            .count();
        assert!(loud_levels >= 10, "{loud_levels} loud level events");
    }

    #[tokio::test]
    async fn incoming_restores_the_original_when_the_captured_app_closes() {
        let io = Arc::new(MemoryIo::new());
        let (events, recorded) = recorder();
        let translator = Arc::new(FakeTranslator::echo(Duration::from_millis(100)));

        let pipeline = TranslationPipeline::start(
            incoming_config_from(SourceApp::AllExceptSelf),
            translator,
            io.clone(),
            events,
        )
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        io.input_failure
            .set("the app audio capture stopped: the captured app was closed".into())
            .unwrap();
        wait_until_finished(&pipeline).await;

        assert_eq!(io.app_volume(), 1.0);
        let ducking = io.ducking.lock().unwrap();
        assert_eq!(
            ducking.started,
            [(LoopbackTarget::everything_except_self(), 20)]
        );
        assert_eq!(ducking.restored_after_output, Some(true));
        let last = statuses(&recorded.lock().unwrap()).pop().unwrap();
        assert_eq!(last.state, PipelineState::Error);
        assert_eq!(last.error.unwrap().code, "audio");
    }

    #[tokio::test]
    async fn incoming_fails_cleanly_when_the_app_is_not_running() {
        let io = Arc::new(MemoryIo {
            app_running: false,
            ..MemoryIo::new()
        });
        let (events, recorded) = recorder();
        let translator = Arc::new(FakeTranslator::echo(Duration::from_millis(100)));

        let error = TranslationPipeline::start(
            incoming_config_from(discord()),
            translator,
            io.clone(),
            events,
        )
        .await
        .err()
        .unwrap();

        assert_eq!(error.code(), "source_app_not_running");
        assert!(io.output_stopped.load(Ordering::Relaxed));
        assert!(io.ducking.lock().unwrap().started.is_empty());
        assert_eq!(
            states(&recorded.lock().unwrap()),
            [PipelineState::Connecting, PipelineState::Error]
        );
    }

    #[tokio::test]
    async fn incoming_neither_ducks_nor_captures_when_the_translator_rejects_the_key() {
        let io = Arc::new(MemoryIo::new());
        let (events, _recorded) = recorder();
        let translator = Arc::new(ScriptedTranslator {
            outputs: Vec::new(),
            rejects_key: true,
        });

        let error = TranslationPipeline::start(
            incoming_config_from(discord()),
            translator,
            io.clone(),
            events,
        )
        .await
        .err()
        .unwrap();

        assert_eq!(error.code(), "openai_auth");
        assert!(io.ducking.lock().unwrap().started.is_empty());
        assert_eq!(io.app_volume(), 1.0);
        // The apps are only captured once the translator is connected (and they are ducked).
        assert!(!io.input_opened.load(Ordering::Relaxed));
        assert!(io.output_stopped.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn incoming_translates_without_ducking_when_ducking_fails() {
        let io = Arc::new(MemoryIo {
            duck_fails: true,
            ..MemoryIo::new()
        });
        let (events, recorded) = recorder();
        let translator = Arc::new(FakeTranslator::echo(Duration::from_millis(100)));

        let pipeline = TranslationPipeline::start(
            incoming_config_from(discord()),
            translator,
            io.clone(),
            events,
        )
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(1000)).await;
        pipeline.stop().await;

        assert_eq!(io.app_volume(), 1.0);
        assert!(io.loud_played_samples() > 0);
        assert_eq!(
            states(&recorded.lock().unwrap()),
            [
                PipelineState::Connecting,
                PipelineState::Live,
                PipelineState::Stopping,
                PipelineState::Idle
            ]
        );
    }

    /// Default microphone -> fake echo translator (300 ms) -> VB-CABLE "CABLE Input" for 3 s,
    /// checking that audio comes out of "CABLE Output". Speak during the test to get a latency
    /// sample. Run with
    /// `cargo test --manifest-path src-tauri/Cargo.toml outgoing_to_vb_cable -- --ignored --nocapture`.
    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "needs a microphone and VB-CABLE"]
    async fn outgoing_to_vb_cable() {
        use crate::audio::{CaptureConfig, DeviceKind, start_capture};
        use crate::pipeline::WasapiIo;

        let _ = tracing_subscriber::fmt()
            .with_env_filter("livedub_voice_lib=debug")
            .with_test_writer()
            .try_init();
        let devices = crate::audio::list_devices().unwrap();
        let find = |kind: DeviceKind, name: &str| {
            devices
                .iter()
                .find(|device| device.kind == kind && device.name.to_lowercase().contains(name))
                .map(|device| device.id.clone())
        };
        let (Some(cable_input), Some(cable_output)) = (
            find(DeviceKind::Output, "cable input"),
            find(DeviceKind::Input, "cable output"),
        ) else {
            println!("VB-CABLE not found; skipping");
            return;
        };

        // Listen to the cable's recording side, like Discord would.
        let (mut listener, _queue) =
            start_capture(Some(&cable_output), CaptureConfig::default()).unwrap();
        // What other apps already play into the cable.
        let mut baseline_peak = 0.0_f32;
        for _ in 0..20 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            baseline_peak = baseline_peak.max(listener.level().peak);
        }
        println!("CABLE Output peak before starting: {baseline_peak:.3}");
        let (events, recorded) = recorder();
        let started = Instant::now();
        let pipeline = TranslationPipeline::start(
            PipelineConfig {
                direction: Direction::Outgoing,
                input: InputSource::Device(None),
                output_device_id: Some(cable_input),
                target_language: Language::English,
                original_volume: None,
                reconnect: ReconnectPolicy::default(),
            },
            Arc::new(FakeTranslator::echo(Duration::from_millis(300))),
            Arc::new(WasapiIo::default()),
            events,
        )
        .await
        .unwrap();
        println!("started in {:?}", started.elapsed());

        let mut cable_peak = 0.0_f32;
        for _ in 0..60 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            cable_peak = cable_peak.max(listener.level().peak);
        }
        let latency = pipeline.latency_report();
        let stopping = Instant::now();
        pipeline.stop().await;
        println!("stopped in {:?}", stopping.elapsed());
        listener.stop();

        let events = recorded.lock().unwrap();
        let mic_peak = events
            .iter()
            .filter_map(|event| match event {
                PipelineEvent::Level(level) => Some(level.peak),
                _ => None,
            })
            .fold(0.0_f32, f32::max);
        println!("mic peak {mic_peak:.3}, CABLE Output peak {cable_peak:.3}");
        println!("latency: {latency}");
        assert_eq!(
            states(&events),
            [
                PipelineState::Connecting,
                PipelineState::Live,
                PipelineState::Stopping,
                PipelineState::Idle
            ]
        );
    }

    /// A PowerShell child plays a quiet tone; the incoming pipeline captures it (process
    /// loopback), ducks it to 20 %, echoes it through the fake translator to the default output
    /// for 3 s, then stops. Checks that the translator heard the tone at full level (ducking
    /// compensated), that the app was ducked while live and restored after. Run with
    /// `cargo test --manifest-path src-tauri/Cargo.toml incoming_from_a_tone_player -- --ignored --nocapture`.
    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "plays a tone on the default output"]
    async fn incoming_from_a_tone_player() {
        use crate::pipeline::WasapiIo;

        let _ = tracing_subscriber::fmt()
            .with_env_filter("livedub_voice_lib=info")
            .with_test_writer()
            .try_init();
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("tone.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&wav, spec).unwrap();
        for i in 0..48_000 * 10 {
            let t = i as f32 / 48_000.0;
            let value = 0.25 * (std::f32::consts::TAU * 440.0 * t).sin();
            writer.write_sample((value * 32_767.0) as i16).unwrap();
        }
        writer.finalize().unwrap();
        // Started through `cmd /c start` so it is not in this process's tree, which LiveDub
        // Voice never captures (its parent, cmd, exits at once).
        std::process::Command::new("cmd")
            .args([
                "/c",
                "start",
                "",
                "/min",
                "powershell.exe",
                "-NoProfile",
                "-Command",
                &format!(
                    "(New-Object Media.SoundPlayer '{}').PlaySync()",
                    wav.display()
                ),
            ])
            .status()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        let player = loop {
            let playing = crate::audio::list_audio_apps()
                .unwrap()
                .into_iter()
                .find(|app| app.process_name.eq_ignore_ascii_case("powershell.exe") && app.active);
            if let Some(app) = playing {
                break app.pid;
            }
            assert!(Instant::now() < deadline, "the tone player never played");
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        println!("tone player pid {player}");

        let journal = dir.path().join("ducking.json");
        let (events, recorded) = recorder();
        let pipeline = TranslationPipeline::start(
            PipelineConfig {
                direction: Direction::Incoming,
                input: InputSource::Apps(SourceApp::Process {
                    name: "powershell.exe".into(),
                }),
                output_device_id: None,
                target_language: Language::Spanish,
                original_volume: Some(20),
                reconnect: ReconnectPolicy::default(),
            },
            Arc::new(FakeTranslator::echo(Duration::from_millis(300))),
            Arc::new(WasapiIo::with_ducking_journal(journal.clone())),
            events,
        )
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(1000)).await;
        let journaled = crate::audio::ducking::load_journal(&journal);
        println!("journal while live: {journaled:?}");
        tokio::time::sleep(Duration::from_millis(2000)).await;
        pipeline.stop().await;
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &player.to_string(), "/F"])
            .status();

        let events = recorded.lock().unwrap();
        let peak = events
            .iter()
            .filter_map(|event| match event {
                PipelineEvent::Level(level) => Some(level.peak),
                _ => None,
            })
            .fold(0.0_f32, f32::max);
        println!("compensated capture peak {peak:.4} (the tone is 0.25)");
        assert_eq!(
            states(&events),
            [
                PipelineState::Connecting,
                PipelineState::Live,
                PipelineState::Stopping,
                PipelineState::Idle
            ]
        );
        assert_eq!(journaled.sessions.len(), 1, "{journaled:?}");
        assert!((journaled.sessions[0].ducked - 0.2).abs() < 0.01);
        assert!((0.2..0.3).contains(&peak), "peak {peak}");
        assert!(!journal.exists(), "restored on stop");
    }
}
