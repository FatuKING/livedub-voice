//! A [`Translator`] that spends no money, for development and tests.
//!
//! Two modes:
//! - **Replay** plays a recorded sequence of server events ([`fixtures/fake_session.jsonl`]: a
//!   Spanish → English exchange with captions and short voice-like audio deltas) at its recorded
//!   timing, looping while the session runs. The input audio is consumed and discarded.
//! - **Echo** returns every input chunk as translated audio after a fixed delay, so the whole
//!   audio path (mic → translator → virtual cable) can be heard end to end.
//!
//! The app picks it at runtime with the `LIVEDUB_FAKE_TRANSLATOR` environment variable
//! ([`FakeTranslator::from_env`]): `1` or `replay` for replay, `echo` for echo (1.5 s delay), or
//! `echo:<ms>` for another delay.
//!
//! [`fixtures/fake_session.jsonl`]: https://github.com/FatuKING/livedub-voice/blob/main/src-tauri/src/translator/fixtures/fake_session.jsonl

use std::sync::Arc;
use std::time::Duration;

use futures_util::future::BoxFuture;
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep_until};

use super::TranslatorError;
use super::live::{
    DEFAULT_INPUT_QUEUE, InputReader, LiveSession, OUTPUT_QUEUE, Translator, TranslatorOutput,
    input_channel,
};
use super::session::{Step, handle_event};
use crate::settings::Language;

/// Environment variable that switches the app to the fake translator.
pub const FAKE_TRANSLATOR_ENV: &str = "LIVEDUB_FAKE_TRANSLATOR";

/// Delay of [`FakeMode::Echo`] when none is given (close to the real model's).
pub const DEFAULT_ECHO_DELAY: Duration = Duration::from_millis(1500);

/// Pause between two loops of the replayed script.
const REPLAY_LOOP_GAP: Duration = Duration::from_secs(2);

/// Built-in replay script: one `{"at_ms": …, "event": {…}}` object per line.
const FIXTURE: &str = include_str!("fixtures/fake_session.jsonl");

/// What the fake does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakeMode {
    /// Replays the recorded script.
    Replay,
    /// Echoes the input audio after `delay`.
    Echo { delay: Duration },
}

impl FakeMode {
    /// Parses a `LIVEDUB_FAKE_TRANSLATOR` value. `None` means "use the real translator".
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim().to_ascii_lowercase();
        match value.as_str() {
            "" | "0" | "false" | "off" => None,
            "1" | "true" | "on" | "replay" => Some(Self::Replay),
            "echo" => Some(Self::Echo {
                delay: DEFAULT_ECHO_DELAY,
            }),
            other => other
                .strip_prefix("echo:")
                .and_then(|ms| ms.parse().ok())
                .map(|ms| Self::Echo {
                    delay: Duration::from_millis(ms),
                }),
        }
    }
}

#[derive(Debug, Clone)]
struct ScriptStep {
    at: Duration,
    output: TranslatorOutput,
}

/// The fake provider. See the [module docs](self).
#[derive(Debug, Clone)]
pub struct FakeTranslator {
    mode: FakeMode,
    script: Arc<Vec<ScriptStep>>,
    input_queue: usize,
}

impl FakeTranslator {
    /// Replays the built-in script.
    pub fn replay() -> Self {
        // The fixture is checked by the tests, so this cannot fail at runtime.
        Self::replay_script(FIXTURE).unwrap_or_else(|_| Self::with_script(Vec::new()))
    }

    /// Replays a custom script (same format as the built-in one: JSON lines of
    /// `{"at_ms": <u64>, "event": <server event>}`). Events that produce no output (such as
    /// `session.created`) are skipped; a `session.closed` ends the script.
    pub fn replay_script(jsonl: &str) -> Result<Self, TranslatorError> {
        Ok(Self::with_script(parse_script(jsonl)?))
    }

    /// Echoes the input after `delay`.
    pub fn echo(delay: Duration) -> Self {
        Self {
            mode: FakeMode::Echo { delay },
            script: Arc::new(Vec::new()),
            input_queue: DEFAULT_INPUT_QUEUE,
        }
    }

    /// The fake selected by `LIVEDUB_FAKE_TRANSLATOR`, if any.
    pub fn from_env() -> Option<Self> {
        let value = std::env::var(FAKE_TRANSLATOR_ENV).ok()?;
        Some(Self::from_mode(FakeMode::parse(&value)?))
    }

    /// A fake for `mode`, replaying the built-in script.
    pub fn from_mode(mode: FakeMode) -> Self {
        match mode {
            FakeMode::Replay => Self::replay(),
            FakeMode::Echo { delay } => Self::echo(delay),
        }
    }

    /// Changes how many 200 ms chunks may wait before the oldest is dropped.
    pub fn with_input_queue(mut self, chunks: usize) -> Self {
        self.input_queue = chunks;
        self
    }

    /// The mode in use.
    pub fn mode(&self) -> FakeMode {
        self.mode
    }

    fn with_script(script: Vec<ScriptStep>) -> Self {
        Self {
            mode: FakeMode::Replay,
            script: Arc::new(script),
            input_queue: DEFAULT_INPUT_QUEUE,
        }
    }
}

impl Translator for FakeTranslator {
    fn name(&self) -> &'static str {
        "fake"
    }

    fn start(
        &self,
        target_language: Language,
    ) -> BoxFuture<'_, Result<LiveSession, TranslatorError>> {
        Box::pin(async move {
            tracing::info!(mode = ?self.mode, ?target_language, "fake translation session started");
            let (input, reader) = input_channel(self.input_queue);
            let (outputs_tx, outputs_rx) = mpsc::channel(OUTPUT_QUEUE);
            let task = match self.mode {
                FakeMode::Replay => tokio::spawn(replay(self.script.clone(), reader, outputs_tx)),
                FakeMode::Echo { delay } => tokio::spawn(echo(delay, reader, outputs_tx)),
            };
            Ok(LiveSession::new(input, outputs_rx, task))
        })
    }
}

fn parse_script(jsonl: &str) -> Result<Vec<ScriptStep>, TranslatorError> {
    #[derive(serde::Deserialize)]
    struct Line {
        at_ms: u64,
        event: serde_json::Value,
    }

    let mut script = Vec::new();
    for (number, text) in jsonl.lines().enumerate() {
        if text.trim().is_empty() {
            continue;
        }
        let line: Line = serde_json::from_str(text).map_err(|error| {
            TranslatorError::Protocol(format!("fake script line {}: {error}", number + 1))
        })?;
        let at = Duration::from_millis(line.at_ms);
        match handle_event(&line.event.to_string()) {
            Step::Output(output) => script.push(ScriptStep { at, output }),
            Step::Closed => break,
            Step::Skip => {}
        }
    }
    Ok(script)
}

/// Plays the script in a loop until the input closes, then reports `Closed`.
async fn replay(
    script: Arc<Vec<ScriptStep>>,
    reader: InputReader,
    outputs: mpsc::Sender<TranslatorOutput>,
) {
    // Drain the input on its own task; it ends when the input is closed.
    let mut input_done = tokio::spawn(async move { while reader.next_chunk().await.is_some() {} });

    let period = script.last().map_or(Duration::ZERO, |step| step.at) + REPLAY_LOOP_GAP;
    let mut cycle_start = Instant::now();
    let mut index = 0;
    loop {
        let next = script.get(index);
        tokio::select! {
            _ = &mut input_done => break,
            () = wait_for(next.map(|step| cycle_start + step.at)) => {
                if let Some(step) = next
                    && outputs.send(step.output.clone()).await.is_err()
                {
                    input_done.abort();
                    return;
                }
                index += 1;
                if index == script.len() {
                    index = 0;
                    cycle_start += period;
                }
            }
        }
    }
    let _ = outputs.send(TranslatorOutput::Closed).await;
}

async fn wait_for(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

/// Returns every chunk after `delay`; after the input closes, the pending ones, then `Closed`.
async fn echo(delay: Duration, reader: InputReader, outputs: mpsc::Sender<TranslatorOutput>) {
    let (delayed_tx, mut delayed) = mpsc::unbounded_channel();
    let input_done = tokio::spawn(async move {
        while let Some(chunk) = reader.next_chunk().await {
            if delayed_tx.send((Instant::now() + delay, chunk)).is_err() {
                break;
            }
        }
    });

    while let Some((due, chunk)) = delayed.recv().await {
        sleep_until(due).await;
        if outputs.send(TranslatorOutput::Audio(chunk)).await.is_err() {
            input_done.abort();
            return;
        }
    }
    let _ = outputs.send(TranslatorOutput::Closed).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translator::pcm::CHUNK_SAMPLES;

    fn audio_firsts(outputs: &[TranslatorOutput]) -> Vec<i16> {
        outputs
            .iter()
            .filter_map(|output| match output {
                TranslatorOutput::Audio(samples) => Some(samples[0]),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn parses_env_values() {
        assert_eq!(FakeMode::parse(""), None);
        assert_eq!(FakeMode::parse("0"), None);
        assert_eq!(FakeMode::parse("1"), Some(FakeMode::Replay));
        assert_eq!(FakeMode::parse(" Replay "), Some(FakeMode::Replay));
        assert_eq!(
            FakeMode::parse("echo"),
            Some(FakeMode::Echo {
                delay: DEFAULT_ECHO_DELAY
            })
        );
        assert_eq!(
            FakeMode::parse("echo:250"),
            Some(FakeMode::Echo {
                delay: Duration::from_millis(250)
            })
        );
        assert_eq!(FakeMode::parse("echo:x"), None);
        assert_eq!(FakeMode::parse("nonsense"), None);
    }

    #[test]
    fn built_in_fixture_parses() {
        let script = parse_script(FIXTURE).unwrap();
        assert!(script.len() >= 10, "{} steps", script.len());
        assert!(script.windows(2).all(|pair| pair[0].at <= pair[1].at));
        assert_eq!(
            script[0].output,
            TranslatorOutput::InputTranscriptDelta("Hola".into())
        );
        let audio: usize = script
            .iter()
            .map(|step| match &step.output {
                TranslatorOutput::Audio(samples) => samples.len(),
                _ => 0,
            })
            .sum();
        assert!(audio > CHUNK_SAMPLES, "{audio} samples of audio");
    }

    #[test]
    fn bad_script_is_an_error() {
        assert!(FakeTranslator::replay_script("{not json").is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn replay_follows_the_script_and_loops() {
        let script = "\
{\"at_ms\": 0, \"event\": {\"type\": \"session.created\", \"session\": {}}}
{\"at_ms\": 100, \"event\": {\"type\": \"session.input_transcript.delta\", \"delta\": \"hola\"}}
{\"at_ms\": 300, \"event\": {\"type\": \"session.output_transcript.delta\", \"delta\": \"hello\"}}
";
        let translator = FakeTranslator::replay_script(script).unwrap();
        let started = Instant::now();
        let mut session = translator.start(Language::English).await.unwrap();
        session.push(&vec![0; CHUNK_SAMPLES * 3]);

        let mut seen = Vec::new();
        for _ in 0..4 {
            seen.push((session.next_output().await.unwrap(), started.elapsed()));
        }
        let hola = TranslatorOutput::InputTranscriptDelta("hola".into());
        let hello = TranslatorOutput::OutputTranscriptDelta("hello".into());
        assert_eq!(seen[0], (hola.clone(), Duration::from_millis(100)));
        assert_eq!(seen[1], (hello.clone(), Duration::from_millis(300)));
        // Second loop: 300 ms + the gap later.
        assert_eq!(seen[2], (hola, Duration::from_millis(2400)));
        assert_eq!(seen[3], (hello, Duration::from_millis(2600)));

        assert_eq!(session.finish().await, vec![TranslatorOutput::Closed]);
    }

    #[tokio::test(start_paused = true)]
    async fn built_in_replay_runs() {
        let translator = FakeTranslator::replay();
        let mut session = translator.start(Language::English).await.unwrap();
        let mut kinds = Vec::new();
        for _ in 0..6 {
            match session.next_output().await.unwrap() {
                TranslatorOutput::Audio(_) => kinds.push("audio"),
                TranslatorOutput::InputTranscriptDelta(_) => kinds.push("input"),
                TranslatorOutput::OutputTranscriptDelta(_) => kinds.push("output"),
                other => panic!("unexpected {other:?}"),
            }
        }
        assert_eq!(
            kinds,
            ["input", "input", "output", "audio", "output", "audio"]
        );
        assert_eq!(session.finish().await, vec![TranslatorOutput::Closed]);
    }

    #[tokio::test(start_paused = true)]
    async fn echo_returns_audio_in_order_after_the_delay() {
        let delay = Duration::from_millis(500);
        let translator = FakeTranslator::echo(delay);
        let started = Instant::now();
        let mut session = translator.start(Language::Spanish).await.unwrap();

        for value in 0..3i16 {
            session.push(&vec![value; CHUNK_SAMPLES]);
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        let first = session.next_output().await.unwrap();
        assert_eq!(first, TranslatorOutput::Audio(vec![0; CHUNK_SAMPLES]));
        assert!(started.elapsed() >= delay);

        // A partial chunk is flushed on stop.
        session.push(&[9; 100]);
        let rest = session.finish().await;
        assert_eq!(audio_firsts(&rest), vec![1, 2, 9]);
        assert_eq!(rest.last(), Some(&TranslatorOutput::Closed));
        assert_eq!(rest.len(), 4);
    }

    #[tokio::test(start_paused = true)]
    async fn backpressure_drops_the_oldest_audio() {
        let translator = FakeTranslator::echo(Duration::from_millis(10)).with_input_queue(3);
        let session = translator.start(Language::English).await.unwrap();
        // The fake has not run yet (single-threaded runtime), so the queue fills up.
        for value in 0..10i16 {
            assert!(session.push(&vec![value; CHUNK_SAMPLES]));
        }
        assert_eq!(session.dropped_chunks(), 7);

        let outputs = session.finish().await;
        assert_eq!(audio_firsts(&outputs), vec![7, 8, 9]);
        assert_eq!(outputs.last(), Some(&TranslatorOutput::Closed));
    }

    #[tokio::test]
    async fn stop_is_clean_and_input_is_refused_after() {
        for translator in [
            FakeTranslator::replay(),
            FakeTranslator::echo(Duration::from_millis(5)),
        ] {
            let mut session = translator.start(Language::English).await.unwrap();
            let input = session.input();
            input.push(&[1; 10]);
            session.stop();
            assert!(!input.push(&[1; 10]));

            let mut last = None;
            let drained = tokio::time::timeout(Duration::from_secs(5), async {
                while let Some(output) = session.next_output().await {
                    last = Some(output);
                }
            })
            .await;
            assert!(drained.is_ok(), "the session did not end");
            assert_eq!(last, Some(TranslatorOutput::Closed));
            tokio::task::yield_now().await;
            // The driver ends right after sending `Closed`.
            tokio::time::timeout(Duration::from_secs(1), async {
                while !session.is_finished() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
        }
    }

    #[tokio::test]
    async fn dropping_the_session_ends_the_driver() {
        let translator = FakeTranslator::echo(Duration::from_millis(5));
        let session = translator.start(Language::English).await.unwrap();
        let input = session.input();
        drop(session);
        assert!(input.is_closed());
    }
}
