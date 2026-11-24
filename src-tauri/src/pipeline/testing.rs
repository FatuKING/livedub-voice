//! Test doubles for the pipelines: in-memory devices ([`MemoryIo`]), fake ducking and
//! translators that follow a script ([`ScriptedTranslator`], [`TestTranslator`]).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle as ThreadHandle;
use std::time::{Duration, Instant};

use futures_util::future::BoxFuture;
use tokio::sync::mpsc;

use super::reconnect::ReconnectPolicy;
use super::translation::{Direction, EventSink, InputSource, PipelineConfig, PipelineEvent};
use super::voice::VoiceConfig;
use super::{
    AudioIo, DuckGuard, InputSide, OutputSide, PipelineState, PipelineStatus, StreamGuard,
};
use crate::audio::ducking::SharedGain;
use crate::audio::level::{LevelMeter, SharedLevel};
use crate::audio::{LoopbackTarget, SampleFormat, StreamFormat, audio_ring};
use crate::error::{AppError, AppResult};
use crate::settings::{Language, SourceApp};
use crate::translator::live::input_channel;
use crate::translator::{LiveSession, Translator, TranslatorError, TranslatorOutput};

pub(crate) fn format(sample_rate: u32, channels: u16) -> StreamFormat {
    StreamFormat {
        sample_rate,
        channels,
        sample_format: SampleFormat::F32,
    }
}

/// How many in-memory streams were opened, and how many still run (their thread not joined).
#[derive(Debug, Clone, Default)]
pub(crate) struct StreamCounters {
    pub(crate) opened: Arc<AtomicUsize>,
    pub(crate) alive: Arc<AtomicUsize>,
}

/// An in-memory stream: a thread that runs `tick(frames)` at the stream's real-time pace.
pub(crate) struct MemoryStream {
    pub(crate) counters: StreamCounters,
    pub(crate) stop: Arc<AtomicBool>,
    pub(crate) thread: Option<ThreadHandle<()>>,
    pub(crate) failure: Arc<OnceLock<String>>,
    pub(crate) stopped: Arc<AtomicBool>,
}

impl MemoryStream {
    pub(crate) fn spawn(
        rate: u32,
        counters: StreamCounters,
        failure: Arc<OnceLock<String>>,
        stopped: Arc<AtomicBool>,
        mut tick: impl FnMut(usize) + Send + 'static,
    ) -> Self {
        counters.opened.fetch_add(1, Ordering::SeqCst);
        counters.alive.fetch_add(1, Ordering::SeqCst);
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
            counters,
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
            self.counters.alive.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

impl Drop for MemoryStream {
    fn drop(&mut self) {
        StreamGuard::stop(self);
    }
}

/// What the fake ducking did.
#[derive(Default)]
pub(crate) struct DuckLog {
    /// `duck` calls: target and original volume.
    pub(crate) started: Vec<(LoopbackTarget, u8)>,
    /// `set_original_volume` calls.
    pub(crate) changed: Vec<u8>,
    /// Whether the output had stopped when the apps were restored.
    pub(crate) restored_after_output: Option<bool>,
}

/// A "microphone" (or captured app) playing a 440 Hz tone (with a 20 ms breath every second) and a "cable" (or headphones) that
/// records what it is sent. The app's tone follows its fake session volume, which the fake
/// ducking lowers, like process loopback does.
pub(crate) struct MemoryIo {
    pub(crate) input_format: StreamFormat,
    pub(crate) output_format: StreamFormat,
    pub(crate) output_opens: bool,
    pub(crate) app_running: bool,
    pub(crate) duck_fails: bool,
    pub(crate) input_failure: Arc<OnceLock<String>>,
    pub(crate) input_opened: Arc<AtomicBool>,
    pub(crate) input_stopped: Arc<AtomicBool>,
    pub(crate) output_stopped: Arc<AtomicBool>,
    pub(crate) played: Arc<Mutex<Vec<f32>>>,
    /// The captured app's session volume (f32 bits).
    pub(crate) app_volume: Arc<AtomicU32>,
    pub(crate) ducking: Arc<Mutex<DuckLog>>,
    pub(crate) streams: StreamCounters,
    /// The input talks (plays the tone) for `.0`, then is silent for `.1`, over and over;
    /// `None` = the tone never stops.
    pub(crate) talk: Option<(Duration, Duration)>,
}

impl MemoryIo {
    pub(crate) fn new() -> Self {
        Self {
            input_format: format(48_000, 2),
            output_format: format(44_100, 2),
            output_opens: true,
            app_running: true,
            duck_fails: false,
            input_failure: Arc::default(),
            input_opened: Arc::default(),
            input_stopped: Arc::default(),
            output_stopped: Arc::default(),
            played: Arc::default(),
            app_volume: Arc::new(AtomicU32::new(1.0_f32.to_bits())),
            ducking: Arc::default(),
            streams: StreamCounters::default(),
            talk: None,
        }
    }

    /// Streams opened so far.
    pub(crate) fn streams_opened(&self) -> usize {
        self.streams.opened.load(Ordering::SeqCst)
    }

    /// Streams still running.
    pub(crate) fn streams_alive(&self) -> usize {
        self.streams.alive.load(Ordering::SeqCst)
    }

    pub(crate) fn app_volume(&self) -> f32 {
        f32::from_bits(self.app_volume.load(Ordering::Relaxed))
    }

    /// A 440 Hz tone at 0.3 times `volume` (read at every tick).
    pub(crate) fn tone(&self, volume: Arc<AtomicU32>) -> InputSide {
        let format = self.input_format;
        let channels = usize::from(format.channels);
        let (mut producer, consumer) = audio_ring(format.channels, format.frames_for_ms(200));
        let level = SharedLevel::default();
        let mut meter = LevelMeter::new(level.clone(), format.frames_for_ms(50) * channels);
        let mut phase = 0.0_f32;
        let step = 440.0 * std::f32::consts::TAU / format.sample_rate as f32;
        let mut buffer = Vec::new();
        let talk = self.talk.map(|(on, off)| {
            let frames = |duration: Duration| {
                (duration.as_secs_f64() * f64::from(format.sample_rate)) as u64
            };
            (frames(on), frames(on) + frames(off))
        });
        let mut frame: u64 = 0;
        let stream = MemoryStream::spawn(
            format.sample_rate,
            self.streams.clone(),
            Arc::clone(&self.input_failure),
            Arc::clone(&self.input_stopped),
            move |frames| {
                let amplitude = 0.3 * f32::from_bits(volume.load(Ordering::Relaxed));
                buffer.clear();
                for _ in 0..frames {
                    // A short breath (20 ms of silence) every second, like speech has: the
                    // voice detector learns a never-ending steady tone as background noise.
                    let breath =
                        frame % u64::from(format.sample_rate) < u64::from(format.sample_rate) / 50;
                    let talking =
                        !breath && talk.is_none_or(|(on, cycle)| frame % cycle.max(1) < on);
                    frame += 1;
                    let sample = if talking {
                        amplitude * phase.sin()
                    } else {
                        0.0
                    };
                    phase = (phase + step) % std::f32::consts::TAU;
                    buffer.extend(std::iter::repeat_n(sample, channels));
                }
                meter.process(&buffer);
                producer.push(&buffer);
            },
        );
        self.input_opened.store(true, Ordering::Relaxed);
        InputSide {
            consumer,
            format,
            level,
            stream: Box::new(stream),
        }
    }

    /// Played samples louder than the tone's quietest part.
    pub(crate) fn loud_played_samples(&self) -> usize {
        self.played
            .lock()
            .unwrap()
            .iter()
            .filter(|sample| sample.abs() > 0.1)
            .count()
    }
}

/// Ducking that sets the fake app's session volume, like the real one.
pub(crate) struct FakeDuck {
    pub(crate) app_volume: Arc<AtomicU32>,
    pub(crate) gain: SharedGain,
    pub(crate) output_stopped: Arc<AtomicBool>,
    pub(crate) log: Arc<Mutex<DuckLog>>,
}

impl DuckGuard for FakeDuck {
    fn gain(&self) -> SharedGain {
        self.gain.clone()
    }

    fn set_original_volume(&self, percent: u8) {
        let level = crate::audio::ducking::duck_level(percent);
        self.app_volume.store(level.to_bits(), Ordering::Relaxed);
        self.gain.set(crate::audio::ducking::capture_gain(level));
        self.log.lock().unwrap().changed.push(percent);
    }

    fn stop(&mut self) {
        self.app_volume.store(1.0_f32.to_bits(), Ordering::Relaxed);
        self.gain.set(1.0);
        self.log.lock().unwrap().restored_after_output =
            Some(self.output_stopped.load(Ordering::Relaxed));
    }
}

impl AudioIo for MemoryIo {
    fn open_input(&self, _device_id: Option<&str>) -> AppResult<InputSide> {
        Ok(self.tone(Arc::new(AtomicU32::new(1.0_f32.to_bits()))))
    }

    fn resolve_loopback(&self, source: &SourceApp) -> AppResult<LoopbackTarget> {
        match source {
            SourceApp::AllExceptSelf => Ok(LoopbackTarget::everything_except_self()),
            SourceApp::Process { .. } if self.app_running => Ok(LoopbackTarget::ProcessTree(4242)),
            SourceApp::Process { name } => Err(AppError::SourceAppNotRunning(name.clone())),
        }
    }

    fn open_loopback(&self, target: LoopbackTarget) -> AppResult<InputSide> {
        let _ = target;
        // Captured after the app's volume, like process loopback.
        Ok(self.tone(Arc::clone(&self.app_volume)))
    }

    fn duck(&self, target: LoopbackTarget, original_volume: u8) -> AppResult<Box<dyn DuckGuard>> {
        if self.duck_fails {
            return Err(AppError::Audio("could not lower the original audio".into()));
        }
        self.ducking
            .lock()
            .unwrap()
            .started
            .push((target, original_volume));
        let level = crate::audio::ducking::duck_level(original_volume);
        self.app_volume.store(level.to_bits(), Ordering::Relaxed);
        Ok(Box::new(FakeDuck {
            app_volume: Arc::clone(&self.app_volume),
            gain: SharedGain::new(crate::audio::ducking::capture_gain(level)),
            output_stopped: Arc::clone(&self.output_stopped),
            log: Arc::clone(&self.ducking),
        }))
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
            self.streams.clone(),
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
pub(crate) struct ScriptedTranslator {
    pub(crate) outputs: Vec<TranslatorOutput>,
    pub(crate) rejects_key: bool,
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

pub(crate) fn recorder() -> (EventSink, Arc<Mutex<Vec<PipelineEvent>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    (
        Arc::new(move |event| sink.lock().unwrap().push(event)),
        events,
    )
}

pub(crate) fn statuses(events: &[PipelineEvent]) -> Vec<PipelineStatus> {
    events
        .iter()
        .filter_map(|event| match event {
            PipelineEvent::Status(status) => Some(status.clone()),
            _ => None,
        })
        .collect()
}

pub(crate) fn states(events: &[PipelineEvent]) -> Vec<PipelineState> {
    statuses(events)
        .into_iter()
        .map(|status| status.state)
        .collect()
}

/// Reconnects quickly: 50 ms, 100 ms, 200 ms, 200 ms…
pub(crate) fn quick_reconnect(max_attempts: u32) -> ReconnectPolicy {
    ReconnectPolicy {
        base_delay: Duration::from_millis(50),
        max_delay: Duration::from_millis(200),
        max_attempts,
        ..ReconnectPolicy::default()
    }
}

/// Microphone -> cable, into English.
pub(crate) fn outgoing_config() -> PipelineConfig {
    PipelineConfig {
        direction: Direction::Outgoing,
        input: InputSource::Device(Some("mic".into())),
        output_device_id: Some("cable".into()),
        target_language: Language::English,
        original_volume: None,
        reconnect: quick_reconnect(3),
        voice: VoiceConfig::default(),
        talk_key: None,
    }
}

/// `source` -> headphones, into Spanish, ducked to 20 %.
pub(crate) fn incoming_config_from(source: SourceApp) -> PipelineConfig {
    PipelineConfig {
        direction: Direction::Incoming,
        input: InputSource::Apps(source),
        output_device_id: Some("headphones".into()),
        target_language: Language::Spanish,
        original_volume: Some(20),
        reconnect: quick_reconnect(3),
        voice: VoiceConfig::default(),
        talk_key: None,
    }
}

/// Discord -> headphones, into Spanish, ducked to 20 %.
pub(crate) fn incoming_config() -> PipelineConfig {
    incoming_config_from(discord())
}

pub(crate) fn discord() -> SourceApp {
    SourceApp::Process {
        name: "Discord.exe".into(),
    }
}

/// What one [`TestTranslator`] session does.
#[derive(Debug, Clone)]
pub(crate) enum Plan {
    /// `start` fails with this error.
    Fail(TranslatorError),
    /// Echoes the input at once until it closes.
    Echo,
    /// Echoes the input, then after this long drops: `Error` (the cause) and `Closed`.
    DropAfter(Duration, TranslatorError),
}

/// A translator that follows a [`Plan`] per `start` (then echoes), counting starts and the
/// sessions whose task still runs.
pub(crate) struct TestTranslator {
    plans: Mutex<VecDeque<Plan>>,
    starts: AtomicUsize,
    /// One clone per running session task.
    token: Arc<()>,
}

impl TestTranslator {
    pub(crate) fn new(plans: Vec<Plan>) -> Arc<Self> {
        Arc::new(Self {
            plans: Mutex::new(plans.into()),
            starts: AtomicUsize::new(0),
            token: Arc::new(()),
        })
    }

    pub(crate) fn echo() -> Arc<Self> {
        Self::new(Vec::new())
    }

    /// `start` calls so far.
    pub(crate) fn starts(&self) -> usize {
        self.starts.load(Ordering::SeqCst)
    }

    /// Sessions whose task still runs.
    pub(crate) fn live_sessions(&self) -> usize {
        Arc::strong_count(&self.token) - 1
    }
}

impl Translator for TestTranslator {
    fn name(&self) -> &'static str {
        "test"
    }

    fn start(
        &self,
        _target_language: Language,
    ) -> BoxFuture<'_, Result<LiveSession, TranslatorError>> {
        Box::pin(async move {
            self.starts.fetch_add(1, Ordering::SeqCst);
            let plan = self.plans.lock().unwrap().pop_front().unwrap_or(Plan::Echo);
            let drop_after = match plan {
                Plan::Fail(error) => return Err(error),
                Plan::Echo => None,
                Plan::DropAfter(after, cause) => Some((tokio::time::Instant::now() + after, cause)),
            };
            let (input, reader) = input_channel(5);
            let (outputs_tx, outputs_rx) = mpsc::channel(64);
            let token = Arc::clone(&self.token);
            let task = tokio::spawn(async move {
                let _token = token;
                let dropped = async {
                    match &drop_after {
                        Some((at, _)) => tokio::time::sleep_until(*at).await,
                        None => std::future::pending().await,
                    }
                };
                tokio::pin!(dropped);
                loop {
                    tokio::select! {
                        chunk = reader.next_chunk() => match chunk {
                            Some(chunk) => {
                                if outputs_tx.send(TranslatorOutput::Audio(chunk.into())).await.is_err() {
                                    return;
                                }
                            }
                            None => break,
                        },
                        () = &mut dropped => {
                            if let Some((_, cause)) = &drop_after {
                                let _ = outputs_tx
                                    .send(TranslatorOutput::Error {
                                        code: cause.code().to_owned(),
                                        message: cause.to_string(),
                                    })
                                    .await;
                            }
                            break;
                        }
                    }
                }
                let _ = outputs_tx.send(TranslatorOutput::Closed).await;
            });
            Ok(LiveSession::new(input, outputs_rx, task))
        })
    }
}
