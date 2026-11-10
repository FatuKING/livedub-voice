//! Outgoing direction: the user's microphone -> translator -> virtual cable.
//!
//! ```text
//! mic (device format) -> ToTranslator (PCM16 mono 24 kHz) -> LiveSession input
//! LiveSession outputs: audio -> JitterBuffer, translated text -> TranscriptDelta events
//! JitterBuffer -> FromTranslator (cable format) -> playback queue -> virtual cable
//! ```
//!
//! Two workers run while the pipeline is live:
//! - the **pump** thread wakes every [`TICK`]: it moves the microphone's samples to the
//!   translator, and tops the playback queue up to [`OUTPUT_TARGET_MS`] from the jitter buffer.
//!   Topping up to a level (instead of on a timer) makes the pipeline pull translated audio at
//!   exactly the cable's clock, so no delay builds up between them. It also reports the input
//!   level and the audio sent;
//! - the **supervisor** task reads the translator's outputs into the jitter buffer, forwards the
//!   captions and watches both devices.
//!
//! # Stopping
//!
//! A stop requested by the user is graceful, so the end of the last sentence is not lost: the
//! state becomes `stopping`, the microphone is replaced by [`TRAILING_SILENCE`] of silence (the
//! model needs it to finish the sentence), then `session.close` is sent and the outputs are read
//! until `session.closed`, and finally the jitter buffer plays its tail into the cable. The
//! whole sequence is capped at [`STOP_DRAIN_LIMIT`]. A fatal error (device gone, key rejected)
//! tears down at once.
//!
//! # Drift and backlog
//!
//! The model often delivers translated speech faster than real time. The jitter buffer keeps all
//! of it; the output converter then plays slightly faster (see [`drift_adjust`]): up to 2 % for
//! a small backlog (clock drift), up to [`MAX_CATCH_UP`] (6 %) when seconds are waiting.
//!
//! # Diagnostics
//!
//! At stop a one-line summary is logged (jitter underruns, drops, max backlog, deltas, audio sent
//! and received, latency per utterance, microphone health). With `LIVEDUB_RECORD_DIR` set, the
//! session is also recorded to disk (see [`super::recorder`]).
//!
//! Audio contents are never logged; only levels, sizes and timings are.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle as ThreadHandle;
use std::time::{Duration, Instant};

use serde_json::json;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use super::latency::{LatencyEvent, LatencyReport, LatencyTracker, SPEECH_RMS};
use super::recorder::{InputHealth, SessionRecorder};
use super::{AudioIo, InputSide, OutputSide, PipelineStatus, StreamGuard};
use crate::audio::level::{InputLevel, LEVEL_WINDOW_MS, SharedLevel};
use crate::audio::resample::{MAX_CATCH_UP, MAX_RATE_ADJUST};
use crate::audio::{
    AudioConsumer, AudioDevice, AudioProducer, DeviceKind, FromTranslator, StreamFormat,
    ToTranslator,
};
use crate::error::{AppError, AppResult};
use crate::settings::{Language, OutgoingSettings};
use crate::translator::pcm::{SAMPLE_RATE, samples_for_ms};
use crate::translator::{
    AudioInput, JitterBuffer, LiveSession, Translator, TranslatorError, TranslatorOutput,
};

/// How often the pump thread runs.
const TICK: Duration = Duration::from_millis(10);

/// Translated audio the pump keeps queued for the cable. Must stay above the playback prebuffer
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

/// Time after the jitter buffer is empty for the playback queue to reach the cable.
const CABLE_DRAIN_TAIL: Duration = Duration::from_millis(150);

/// Longest wait for the translator to close after a fatal error or a timed-out stop.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(3);

/// Backlog above the prebuffer at which the drift correction reaches [`MAX_RATE_ADJUST`].
const DRIFT_RANGE_MS: f64 = 1000.0;

/// Backlog above the prebuffer at which the catch-up reaches [`MAX_CATCH_UP`].
const CATCH_UP_FULL_MS: f64 = 4000.0;

/// What the outgoing pipeline reports while it runs.
#[derive(Debug, Clone, PartialEq)]
pub enum OutgoingEvent {
    /// The pipeline's state changed. The first event is `connecting`; the last one is always
    /// `idle` (stopped) or `error` (could not start, or stopped on a fatal error). A stop asked
    /// by the user goes through `stopping` first.
    Status(PipelineStatus),
    /// Microphone level, ~20 times per second while live.
    Level(InputLevel),
    /// Translated text (append-only delta), in the other side's language.
    TranscriptDelta(String),
    /// Seconds of microphone audio sent to the translator since start, every second and once
    /// more when the pipeline stops.
    AudioSent { seconds: f64 },
}

/// Receives the pipeline's events, from the pipeline's threads.
pub type EventSink = Arc<dyn Fn(OutgoingEvent) + Send + Sync>;

/// What the outgoing pipeline uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingConfig {
    /// Microphone; `None` = system default.
    pub input_device_id: Option<String>,
    /// Playback side of the virtual cable; `None` = system default output.
    pub output_device_id: Option<String>,
    /// Language the other participants hear.
    pub target_language: Language,
}

impl From<&OutgoingSettings> for OutgoingConfig {
    fn from(settings: &OutgoingSettings) -> Self {
        Self {
            input_device_id: settings.input_device_id.clone(),
            output_device_id: settings.output_device_id.clone(),
            target_language: settings.target_language,
        }
    }
}

/// Checks the outgoing devices against the active ones before starting.
///
/// Fails when no output is chosen ([`AppError::NoOutputDevice`]) or a chosen device is not
/// connected ([`AppError::DeviceMissing`]). Returns whether the output is a known virtual cable:
/// any other output still works (the user may know better), so it only deserves a warning.
pub fn check_devices(settings: &OutgoingSettings, devices: &[AudioDevice]) -> AppResult<bool> {
    let output_id = settings
        .output_device_id
        .as_deref()
        .ok_or(AppError::NoOutputDevice)?;
    let output = devices
        .iter()
        .find(|device| device.kind == DeviceKind::Output && device.id == output_id)
        .ok_or_else(|| {
            AppError::DeviceMissing("the output device (virtual cable) is not connected".into())
        })?;
    if let Some(input_id) = settings.input_device_id.as_deref()
        && !devices
            .iter()
            .any(|device| device.kind == DeviceKind::Input && device.id == input_id)
    {
        return Err(AppError::DeviceMissing(
            "the microphone is not connected".into(),
        ));
    }
    Ok(output.is_virtual_cable)
}

/// State shared by the pump thread and the supervisor task.
struct Shared {
    started: Instant,
    jitter: Mutex<JitterBuffer>,
    latency: Mutex<LatencyTracker>,
    health: Mutex<InputHealth>,
    recorder: Mutex<Option<SessionRecorder>>,
    /// Ends the pump thread.
    stop: AtomicBool,
    /// Stop requested: send silence instead of the microphone.
    trailing: AtomicBool,
    /// The trailing silence has been sent; nothing more goes to the translator.
    input_done: AtomicBool,
    /// The jitter buffer was drained (after `finish_input`) and its tail reached the cable.
    cable_drained: AtomicBool,
    sent_samples: AtomicU64,
    output_deltas: AtomicU64,
}

impl Shared {
    fn new(recorder: Option<SessionRecorder>) -> Self {
        Self {
            started: Instant::now(),
            jitter: Mutex::new(JitterBuffer::default()),
            latency: Mutex::new(LatencyTracker::default()),
            health: Mutex::new(InputHealth::default()),
            recorder: Mutex::new(recorder),
            stop: AtomicBool::new(false),
            trailing: AtomicBool::new(false),
            input_done: AtomicBool::new(false),
            cable_drained: AtomicBool::new(false),
            sent_samples: AtomicU64::new(0),
            output_deltas: AtomicU64::new(0),
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
        let fields = match event {
            LatencyEvent::SpeechStart { at } => {
                tracing::debug!(at_ms = at.as_millis() as u64, "outgoing speech started");
                json!({ "kind": "speech_start", "at_ms": at.as_millis() as u64 })
            }
            LatencyEvent::SpeechEnd { at } => {
                tracing::debug!(at_ms = at.as_millis() as u64, "outgoing speech ended");
                json!({ "kind": "speech_end", "at_ms": at.as_millis() as u64 })
            }
            LatencyEvent::StartToAudio { utterance, latency } => {
                tracing::debug!(
                    utterance,
                    latency_ms = latency.as_millis() as u64,
                    "outgoing latency: start of speech -> first translated audio"
                );
                json!({ "kind": "latency_start_to_audio", "utterance": utterance, "ms": latency.as_millis() as u64 })
            }
            LatencyEvent::EndToAudio { utterance, latency } => {
                tracing::debug!(
                    utterance,
                    latency_ms = latency.as_millis() as u64,
                    "outgoing latency: end of speech -> first translated audio"
                );
                json!({ "kind": "latency_end_to_audio", "utterance": utterance, "ms": latency.as_millis() as u64 })
            }
        };
        self.record_event(fields);
    }
}

/// A running outgoing translation. Stop it with [`stop`](Self::stop); dropping it also stops it
/// (in the background).
pub struct OutgoingPipeline {
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
    shared: Arc<Shared>,
}

impl OutgoingPipeline {
    /// Opens the cable and the microphone, starts a translation session and starts translating.
    ///
    /// Emits `connecting` first, then `live`; on failure emits `error` and returns the error,
    /// with nothing left running. Must run inside a Tokio runtime.
    pub async fn start(
        config: OutgoingConfig,
        translator: Arc<dyn Translator>,
        io: Arc<dyn AudioIo>,
        events: EventSink,
    ) -> AppResult<Self> {
        events(OutgoingEvent::Status(PipelineStatus::connecting()));
        match Self::try_start(config, translator, io, Arc::clone(&events)).await {
            Ok(pipeline) => Ok(pipeline),
            Err(error) => {
                tracing::warn!(code = error.code(), %error, "outgoing translation could not start");
                events(OutgoingEvent::Status(PipelineStatus::error(&error)));
                Err(error)
            }
        }
    }

    async fn try_start(
        config: OutgoingConfig,
        translator: Arc<dyn Translator>,
        io: Arc<dyn AudioIo>,
        events: EventSink,
    ) -> AppResult<Self> {
        // The cable first: it is the device most likely to be missing.
        let output = {
            let io = Arc::clone(&io);
            let id = config.output_device_id.clone();
            blocking(move || io.open_output(id.as_deref())).await?
        };
        let input = {
            let id = config.input_device_id.clone();
            match blocking(move || io.open_input(id.as_deref())).await {
                Ok(input) => input,
                Err(error) => {
                    stop_streams(vec![output.stream]).await;
                    return Err(error);
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
                stop_streams(vec![input.stream, output.stream]).await;
                return Err(AppError::Audio(error.to_string()));
            }
        };

        let session = match translator.start(config.target_language).await {
            Ok(session) => session,
            Err(error) => {
                stop_streams(vec![input.stream, output.stream]).await;
                return Err(error.into());
            }
        };
        tracing::info!(
            provider = translator.name(),
            input = %input.format,
            output = %output.format,
            language = ?config.target_language,
            "outgoing translation started"
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
            "outgoing",
            output_format.sample_rate,
            output_format.channels,
        );
        let shared = Arc::new(Shared::new(recorder));

        events(OutgoingEvent::Status(PipelineStatus::live()));
        let pump = Pump::spawn(PumpParts {
            input: consumer,
            to_translator,
            session_input: session.input(),
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
                return Err(error);
            }
        };

        let (stop_tx, stop_rx) = oneshot::channel();
        let supervisor = Supervisor {
            session,
            input_stream,
            output_stream,
            pump,
            shared: Arc::clone(&shared),
            events,
            last_error: None,
        };
        let task = tokio::spawn(supervisor.run(stop_rx));
        Ok(Self {
            stop: Some(stop_tx),
            task: Some(task),
            shared,
        })
    }

    /// Whether the pipeline has ended (stopped on its own after a fatal error).
    pub fn is_finished(&self) -> bool {
        self.task.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Stops the pipeline (gracefully: see the module docs) and waits until every device is
    /// released and the translator closed.
    pub async fn stop(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take()
            && let Err(error) = task.await
        {
            tracing::error!(%error, "outgoing pipeline task ended abnormally");
        }
    }

    /// Latency per utterance so far (see [`super::latency`]).
    pub fn latency_report(&self) -> LatencyReport {
        lock(&self.shared.latency).report()
    }
}

impl Drop for OutgoingPipeline {
    fn drop(&mut self) {
        // The supervisor tears down on its own once it sees the stop.
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
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
    session: LiveSession,
    input_stream: Box<dyn StreamGuard>,
    output_stream: Box<dyn StreamGuard>,
    pump: Pump,
    shared: Arc<Shared>,
    events: EventSink,
    /// The latest non-fatal error, reported if the session then closes.
    last_error: Option<TranslatorError>,
}

impl Supervisor {
    async fn run(mut self, mut stop: oneshot::Receiver<()>) {
        let mut health = tokio::time::interval(HEALTH_INTERVAL);
        // `None` = stopped by the user.
        let failure: Option<AppError> = loop {
            tokio::select! {
                _ = &mut stop => break None,
                _ = health.tick() => {
                    if let Some(failure) = self.device_failure() {
                        break Some(AppError::Audio(failure));
                    }
                }
                output = self.session.next_output() => match self.on_output(output) {
                    Next::Continue => {}
                    Next::Fatal(error) => break Some(error),
                    Next::Closed => {
                        break Some(self.last_error.take().unwrap_or(TranslatorError::Closed).into());
                    }
                }
            }
        };
        let drained = match failure {
            None => self.drain().await,
            Some(_) => false,
        };
        self.shutdown(failure, drained).await;
    }

    fn device_failure(&self) -> Option<String> {
        self.input_stream
            .failure()
            .or_else(|| self.output_stream.failure())
    }

    fn on_output(&mut self, output: Option<TranslatorOutput>) -> Next {
        let shared = &self.shared;
        match output {
            Some(TranslatorOutput::Audio(samples)) => {
                let at = shared.elapsed();
                let buffered_ms = {
                    let mut jitter = lock(&shared.jitter);
                    jitter.push(&samples);
                    jitter.buffered().as_millis() as u64
                };
                shared.output_deltas.fetch_add(1, Ordering::Relaxed);
                let latency_events = lock(&shared.latency).on_output(at);
                shared.record(|recorder| recorder.received(&samples, at, buffered_ms));
                for event in latency_events {
                    shared.latency_event(event);
                }
                Next::Continue
            }
            Some(TranslatorOutput::OutputTranscriptDelta(text)) => {
                shared.record_event(json!({ "kind": "output_transcript", "text": text }));
                (self.events)(OutgoingEvent::TranscriptDelta(text));
                Next::Continue
            }
            Some(TranslatorOutput::InputTranscriptDelta(text)) => {
                shared.record_event(json!({ "kind": "input_transcript", "text": text }));
                Next::Continue
            }
            Some(TranslatorOutput::Error { code, message }) => {
                let error = TranslatorError::from_api_error(code, &message);
                shared.record_event(json!({ "kind": "error", "code": error.code() }));
                if is_fatal(&error) {
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
    async fn drain(&mut self) -> bool {
        (self.events)(OutgoingEvent::Status(PipelineStatus::stopping()));
        self.shared
            .record_event(json!({ "kind": "stop_requested" }));
        tracing::info!("outgoing translation stopping: finishing the last sentence");
        let deadline = tokio::time::Instant::now() + STOP_DRAIN_LIMIT;
        self.shared.trailing.store(true, Ordering::Relaxed);
        let mut poll = tokio::time::interval(Duration::from_millis(20));

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum Phase {
            /// Sending the trailing silence.
            Trailing,
            /// `session.close` sent, reading outputs until `session.closed`.
            Closing,
            /// Playing the jitter buffer's tail into the cable.
            Playing,
        }
        let mut phase = Phase::Trailing;
        loop {
            if phase == Phase::Trailing
                && (self.shared.input_done.load(Ordering::Relaxed)
                    || self.input_stream.failure().is_some())
            {
                self.session.stop();
                self.shared
                    .record_event(json!({ "kind": "session_close_sent" }));
                phase = Phase::Closing;
            }
            if phase == Phase::Playing && self.shared.cable_drained.load(Ordering::Relaxed) {
                self.shared.record_event(json!({ "kind": "cable_drained" }));
                return true;
            }
            if self.output_stream.failure().is_some() {
                return false;
            }
            tokio::select! {
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
                output = self.session.next_output(), if phase != Phase::Playing => {
                    match self.on_output(output) {
                        Next::Continue => {}
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
    async fn shutdown(self, failure: Option<AppError>, drained: bool) {
        let Self {
            session,
            input_stream,
            output_stream,
            mut pump,
            shared,
            events,
            ..
        } = self;

        // No more microphone audio.
        stop_streams(vec![input_stream]).await;
        // After a drained stop the session is already closed and this returns at once.
        let dropped_chunks = session.dropped_chunks();
        let close_wait = if drained {
            Duration::from_secs(1)
        } else {
            CLOSE_TIMEOUT
        };
        if tokio::time::timeout(close_wait, session.finish())
            .await
            .is_err()
        {
            tracing::warn!("the translator did not close in time");
        }
        // The pump reports the final audio sent, then the cable stops.
        let _ = tokio::task::spawn_blocking(move || {
            pump.stop();
            let mut output_stream = output_stream;
            output_stream.stop();
        })
        .await;

        log_summary(&shared, dropped_chunks);

        events(OutgoingEvent::Level(InputLevel::default()));
        match failure {
            None => {
                tracing::info!("outgoing translation stopped");
                events(OutgoingEvent::Status(PipelineStatus::idle()));
            }
            Some(error) => {
                tracing::warn!(code = error.code(), %error, "outgoing translation stopped on an error");
                events(OutgoingEvent::Status(PipelineStatus::error(&error)));
            }
        }
    }
}

/// Logs the one-line session summary (and records it), then finishes the recording.
fn log_summary(shared: &Shared, dropped_chunks: u64) {
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
        input_dropped_chunks = dropped_chunks,
        "outgoing session summary"
    );
    let latency = lock(&shared.latency).report();
    tracing::info!("outgoing latency: {latency}");
    let input = lock(&shared.health).summary();
    tracing::info!(
        peak_dbfs = input.peak_dbfs,
        rms_dbfs = input.rms_dbfs,
        speech_rms_dbfs = input.speech_rms_dbfs,
        clipped_samples = input.clipped_samples,
        channel_rms_dbfs = ?input.channel_rms_dbfs,
        "outgoing microphone summary"
    );
    for warning in &input.warnings {
        tracing::warn!("microphone: {warning}");
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
        "jitter_underruns": jitter.underruns,
        "jitter_dropped_samples": jitter.dropped_samples,
        "jitter_max_buffered_ms": jitter.max_buffered_ms(),
        "utterances_played": jitter.utterances,
        "output_deltas": deltas,
        "sent_s": seconds(sent),
        "sent_bytes": sent * 2,
        "received_s": seconds(jitter.pushed_samples),
        "received_bytes": jitter.pushed_samples * 2,
        "input_dropped_chunks": dropped_chunks,
        "latency_start_to_audio": spread(latency.start_to_audio),
        "latency_end_to_audio": spread(latency.end_to_audio),
        "mic_peak_dbfs": input.peak_dbfs,
        "mic_rms_dbfs": input.rms_dbfs,
        "mic_speech_rms_dbfs": input.speech_rms_dbfs,
        "mic_clipped_samples": input.clipped_samples,
        "mic_channel_rms_dbfs": input.channel_rms_dbfs,
        "mic_warnings": input.warnings,
    }));
    if let Some(recorder) = lock(&shared.recorder).take() {
        recorder.finish();
    }
}

/// Errors after which the session cannot go on.
fn is_fatal(error: &TranslatorError) -> bool {
    matches!(
        error,
        TranslatorError::Auth | TranslatorError::NoModelAccess | TranslatorError::RateLimited
    )
}

/// What the pump thread owns.
struct PumpParts {
    input: AudioConsumer,
    to_translator: ToTranslator,
    session_input: AudioInput,
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
            .name("livedub-outgoing".into())
            .spawn(move || run_pump(parts))
            .map_err(|error| {
                AppError::Internal(format!("could not start the outgoing thread: {error}"))
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
            tracing::error!("outgoing pump thread panicked");
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
    let mut mic = vec![0.0_f32; 1024 * channels];
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
        // Microphone -> translator.
        let trailing = shared.trailing.load(Ordering::Relaxed);
        loop {
            let read = parts.input.pop(&mut mic);
            if read == 0 {
                break;
            }
            if !trailing {
                lock(&shared.health).captured(&mic[..read], channels);
            }
            parts.to_translator.process(&mic[..read], &mut pcm);
        }
        if !pcm.is_empty() && !shared.input_done.load(Ordering::Relaxed) {
            if trailing {
                // The user stopped: silence, paced by the microphone's clock, so the model
                // finishes the last sentence.
                let take = (trailing_samples - trailing_sent).min(pcm.len() as u64) as usize;
                pcm.truncate(take);
                pcm.fill(0);
                trailing_sent += take as u64;
            }
            let block_rms = rms(&pcm);
            let at = shared.elapsed();
            let latency_event = lock(&shared.latency).on_input(block_rms, at);
            if let Some(event) = latency_event {
                shared.latency_event(event);
            }
            if !trailing {
                lock(&shared.health).sent(&pcm, block_rms >= SPEECH_RMS);
            }
            if parts.session_input.push(&pcm) {
                sent_samples += pcm.len() as u64;
                shared.sent_samples.store(sent_samples, Ordering::Relaxed);
                shared.record(|recorder| recorder.sent(&pcm, at));
            }
            if trailing && trailing_sent >= trailing_samples {
                shared.input_done.store(true, Ordering::Relaxed);
            }
        }
        pcm.clear();

        // Translator -> cable, at the cable's pace.
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
            shared.record(|recorder| recorder.cable(&device));
            device.clear();
        }

        let now = Instant::now();
        if drained {
            // Wait for the playback queue to reach the cable too.
            let since = *drained_since.get_or_insert(now);
            if now.duration_since(since) >= CABLE_DRAIN_TAIL {
                shared.cable_drained.store(true, Ordering::Relaxed);
            }
        }
        if now >= next_level {
            (parts.events)(OutgoingEvent::Level(parts.level.get()));
            next_level = now + level_interval;
        }
        if now >= next_sent {
            (parts.events)(OutgoingEvent::AudioSent {
                seconds: seconds(sent_samples),
            });
            let (stats, buffered_ms) = {
                let jitter = lock(&shared.jitter);
                (jitter.stats(), jitter.buffered().as_millis() as u64)
            };
            shared.record_event(json!({
                "kind": "jitter",
                "buffered_ms": buffered_ms,
                "underruns": stats.underruns,
                "dropped_samples": stats.dropped_samples,
                "utterances": stats.utterances,
                "rate": (rate * 10_000.0).round() / 10_000.0,
                "input_queued_chunks": parts.session_input.queued_chunks(),
                "input_dropped_chunks": parts.session_input.dropped_chunks(),
            }));
            next_sent = now + AUDIO_SENT_INTERVAL;
        }
        std::thread::sleep(TICK);
    }
    (parts.events)(OutgoingEvent::AudioSent {
        seconds: seconds(sent_samples),
    });
}

/// Output rate factor for the jitter buffer's backlog (audio queued above the prebuffer
/// target): 1.0 at or below the target; then down to `1 - MAX_RATE_ADJUST` at
/// [`DRIFT_RANGE_MS`] of backlog (clock drift, inaudible); then down to `1 - MAX_CATCH_UP` at
/// [`CATCH_UP_FULL_MS`] (a burst of translated speech waiting: play it slightly faster rather
/// than fall seconds behind). Below 1.0 each translated sample becomes fewer cable frames.
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
    use std::sync::OnceLock;

    use tokio::sync::mpsc;

    use super::*;
    use crate::audio::level::LevelMeter;
    use crate::audio::{SampleFormat, audio_ring};
    use crate::pipeline::PipelineState;
    use crate::translator::FakeTranslator;
    use crate::translator::live::input_channel;

    fn format(sample_rate: u32, channels: u16) -> StreamFormat {
        StreamFormat {
            sample_rate,
            channels,
            sample_format: SampleFormat::F32,
        }
    }

    /// An in-memory stream: a thread that runs `tick(frames)` at the stream's real-time pace.
    struct MemoryStream {
        stop: Arc<AtomicBool>,
        thread: Option<ThreadHandle<()>>,
        failure: Arc<OnceLock<String>>,
        stopped: Arc<AtomicBool>,
    }

    impl MemoryStream {
        fn spawn(
            rate: u32,
            failure: Arc<OnceLock<String>>,
            stopped: Arc<AtomicBool>,
            mut tick: impl FnMut(usize) + Send + 'static,
        ) -> Self {
            let stop = Arc::new(AtomicBool::new(false));
            let thread_stop = Arc::clone(&stop);
            let thread = std::thread::spawn(move || {
                let start = Instant::now();
                let mut done = 0;
                while !thread_stop.load(Ordering::Relaxed) {
                    let due = (start.elapsed().as_secs_f64() * f64::from(rate)) as usize;
                    if due > done {
                        tick(due - done);
                        done = due;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            });
            Self {
                stop,
                thread: Some(thread),
                failure,
                stopped,
            }
        }
    }

    impl StreamGuard for MemoryStream {
        fn failure(&self) -> Option<String> {
            self.failure.get().cloned()
        }

        fn stop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(thread) = self.thread.take() {
                thread.join().unwrap();
                self.stopped.store(true, Ordering::Relaxed);
            }
        }
    }

    impl Drop for MemoryStream {
        fn drop(&mut self) {
            StreamGuard::stop(self);
        }
    }

    /// A "microphone" playing a 440 Hz tone and a "cable" that records what it is sent.
    struct MemoryIo {
        input_format: StreamFormat,
        output_format: StreamFormat,
        output_opens: bool,
        input_failure: Arc<OnceLock<String>>,
        input_opened: Arc<AtomicBool>,
        input_stopped: Arc<AtomicBool>,
        output_stopped: Arc<AtomicBool>,
        played: Arc<Mutex<Vec<f32>>>,
    }

    impl MemoryIo {
        fn new() -> Self {
            Self {
                input_format: format(48_000, 2),
                output_format: format(44_100, 2),
                output_opens: true,
                input_failure: Arc::default(),
                input_opened: Arc::default(),
                input_stopped: Arc::default(),
                output_stopped: Arc::default(),
                played: Arc::default(),
            }
        }

        /// Played samples louder than the tone's quietest part.
        fn loud_played_samples(&self) -> usize {
            self.played
                .lock()
                .unwrap()
                .iter()
                .filter(|sample| sample.abs() > 0.1)
                .count()
        }
    }

    impl AudioIo for MemoryIo {
        fn open_input(&self, _device_id: Option<&str>) -> AppResult<InputSide> {
            let format = self.input_format;
            let channels = usize::from(format.channels);
            let (mut producer, consumer) = audio_ring(format.channels, format.frames_for_ms(200));
            let level = SharedLevel::default();
            let mut meter = LevelMeter::new(level.clone(), format.frames_for_ms(50) * channels);
            let mut phase = 0.0_f32;
            let step = 440.0 * std::f32::consts::TAU / format.sample_rate as f32;
            let mut buffer = Vec::new();
            let stream = MemoryStream::spawn(
                format.sample_rate,
                Arc::clone(&self.input_failure),
                Arc::clone(&self.input_stopped),
                move |frames| {
                    buffer.clear();
                    for _ in 0..frames {
                        let sample = 0.3 * phase.sin();
                        phase = (phase + step) % std::f32::consts::TAU;
                        buffer.extend(std::iter::repeat_n(sample, channels));
                    }
                    meter.process(&buffer);
                    producer.push(&buffer);
                },
            );
            self.input_opened.store(true, Ordering::Relaxed);
            Ok(InputSide {
                consumer,
                format,
                level,
                stream: Box::new(stream),
            })
        }

        fn open_output(&self, _device_id: Option<&str>) -> AppResult<OutputSide> {
            if !self.output_opens {
                return Err(AppError::Audio("could not open the output device".into()));
            }
            let format = self.output_format;
            let channels = usize::from(format.channels);
            let (producer, mut consumer) = audio_ring(format.channels, format.frames_for_ms(200));
            let played = Arc::clone(&self.played);
            let mut buffer = Vec::new();
            let stream = MemoryStream::spawn(
                format.sample_rate,
                Arc::default(),
                Arc::clone(&self.output_stopped),
                move |frames| {
                    buffer.resize(frames * channels, 0.0);
                    let read = consumer.pop(&mut buffer);
                    played.lock().unwrap().extend_from_slice(&buffer[..read]);
                },
            );
            Ok(OutputSide {
                producer,
                format,
                stream: Box::new(stream),
            })
        }
    }

    /// A translator that sends `outputs` after 200 ms, then waits for the input to close.
    struct ScriptedTranslator {
        outputs: Vec<TranslatorOutput>,
        rejects_key: bool,
    }

    impl Translator for ScriptedTranslator {
        fn name(&self) -> &'static str {
            "scripted"
        }

        fn start(
            &self,
            _target_language: Language,
        ) -> futures_util::future::BoxFuture<'_, Result<LiveSession, TranslatorError>> {
            Box::pin(async move {
                if self.rejects_key {
                    return Err(TranslatorError::Auth);
                }
                let (input, reader) = input_channel(5);
                let (outputs_tx, outputs_rx) = mpsc::channel(16);
                let outputs = self.outputs.clone();
                let task = tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    for output in outputs {
                        if outputs_tx.send(output).await.is_err() {
                            return;
                        }
                    }
                    while reader.next_chunk().await.is_some() {}
                    let _ = outputs_tx.send(TranslatorOutput::Closed).await;
                });
                Ok(LiveSession::new(input, outputs_rx, task))
            })
        }
    }

    fn recorder() -> (EventSink, Arc<Mutex<Vec<OutgoingEvent>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        (
            Arc::new(move |event| sink.lock().unwrap().push(event)),
            events,
        )
    }

    fn statuses(events: &[OutgoingEvent]) -> Vec<PipelineStatus> {
        events
            .iter()
            .filter_map(|event| match event {
                OutgoingEvent::Status(status) => Some(status.clone()),
                _ => None,
            })
            .collect()
    }

    fn states(events: &[OutgoingEvent]) -> Vec<PipelineState> {
        statuses(events)
            .into_iter()
            .map(|status| status.state)
            .collect()
    }

    fn config() -> OutgoingConfig {
        OutgoingConfig {
            input_device_id: Some("mic".into()),
            output_device_id: Some("cable".into()),
            target_language: Language::English,
        }
    }

    async fn wait_until_finished(pipeline: &OutgoingPipeline) {
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

        let pipeline = OutgoingPipeline::start(config(), translator, io.clone(), events)
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
            Some(&OutgoingEvent::Status(PipelineStatus::connecting()))
        );
        assert_eq!(
            events.last(),
            Some(&OutgoingEvent::Status(PipelineStatus::idle()))
        );
        let live_at = events
            .iter()
            .position(|event| event == &OutgoingEvent::Status(PipelineStatus::live()))
            .unwrap();
        let loud_levels = events[live_at..]
            .iter()
            .filter(|event| matches!(event, OutgoingEvent::Level(level) if level.rms > 0.1))
            .count();
        assert!(loud_levels >= 10, "{loud_levels} loud level events");
        let sent = events
            .iter()
            .rev()
            .find_map(|event| match event {
                OutgoingEvent::AudioSent { seconds } => Some(*seconds),
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

        let pipeline = OutgoingPipeline::start(config(), translator, io.clone(), events)
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
            .position(|event| event == &OutgoingEvent::TranscriptDelta("Hello".into()))
            .unwrap();
        let live_at = events
            .iter()
            .position(|event| event == &OutgoingEvent::Status(PipelineStatus::live()))
            .unwrap();
        assert!(live_at < caption_at);
        let OutgoingEvent::Status(last) = events.last().unwrap() else {
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

        let pipeline = OutgoingPipeline::start(config(), translator, io.clone(), events)
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

        let error = OutgoingPipeline::start(config(), translator, io.clone(), events)
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

        let error = OutgoingPipeline::start(config(), translator, io.clone(), events)
            .await
            .err()
            .unwrap();

        assert_eq!(error.code(), "audio");
        assert!(!io.input_opened.load(Ordering::Relaxed));
        let last = statuses(&recorded.lock().unwrap()).pop().unwrap();
        assert_eq!(last.state, PipelineState::Error);
    }

    fn device(id: &str, kind: DeviceKind, is_virtual_cable: bool) -> AudioDevice {
        AudioDevice {
            id: id.into(),
            name: id.into(),
            kind,
            is_default: false,
            is_virtual_cable,
        }
    }

    fn outgoing(input: Option<&str>, output: Option<&str>) -> OutgoingSettings {
        OutgoingSettings {
            input_device_id: input.map(str::to_owned),
            output_device_id: output.map(str::to_owned),
            ..OutgoingSettings::default()
        }
    }

    #[test]
    fn check_devices_needs_a_connected_output_and_microphone() {
        let devices = [
            device("mic", DeviceKind::Input, false),
            device("cable", DeviceKind::Output, true),
            device("speakers", DeviceKind::Output, false),
        ];

        assert!(matches!(
            check_devices(&outgoing(None, None), &devices),
            Err(AppError::NoOutputDevice)
        ));
        assert!(matches!(
            check_devices(&outgoing(None, Some("gone")), &devices),
            Err(AppError::DeviceMissing(message)) if message.contains("output")
        ));
        // An input id is not an output.
        assert!(check_devices(&outgoing(None, Some("mic")), &devices).is_err());
        assert!(matches!(
            check_devices(&outgoing(Some("gone"), Some("cable")), &devices),
            Err(AppError::DeviceMissing(message)) if message.contains("microphone")
        ));
        assert!(check_devices(&outgoing(Some("mic"), Some("cable")), &devices).unwrap());
        assert!(check_devices(&outgoing(None, Some("cable")), &devices).unwrap());
        // Not a known cable: allowed, but reported.
        assert!(!check_devices(&outgoing(None, Some("speakers")), &devices).unwrap());
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
                        .send(TranslatorOutput::Audio(vec![20_000; SAMPLE_RATE as usize]))
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

        let pipeline = OutgoingPipeline::start(config(), translator, io.clone(), events)
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

    /// Default microphone -> fake echo translator (300 ms) -> VB-CABLE "CABLE Input" for 3 s,
    /// checking that audio comes out of "CABLE Output". Speak during the test to get a latency
    /// sample. Run with
    /// `cargo test --manifest-path src-tauri/Cargo.toml outgoing_to_vb_cable -- --ignored --nocapture`.
    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "needs a microphone and VB-CABLE"]
    async fn outgoing_to_vb_cable() {
        use crate::audio::{CaptureConfig, start_capture};
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
        let pipeline = OutgoingPipeline::start(
            OutgoingConfig {
                input_device_id: None,
                output_device_id: Some(cable_input),
                target_language: Language::English,
            },
            Arc::new(FakeTranslator::echo(Duration::from_millis(300))),
            Arc::new(WasapiIo),
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
                OutgoingEvent::Level(level) => Some(level.peak),
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
}
