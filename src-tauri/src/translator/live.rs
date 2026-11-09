//! Provider-agnostic live translation.
//!
//! A [`Translator`] starts a [`LiveSession`] for a target language. Audio goes in through an
//! [`AudioInput`] (any length, from any thread, never blocks); outputs come out of
//! [`LiveSession::next_output`] as [`TranslatorOutput`]s, ending with
//! [`TranslatorOutput::Closed`].
//!
//! The input side cuts the audio into [`CHUNK_SAMPLES`] chunks (200 ms) and keeps at most a few
//! of them waiting. If the provider stalls (a slow socket), the oldest chunk is dropped so the
//! translation stays close to real time instead of building up delay.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use futures_util::future::BoxFuture;
use tokio::sync::{Notify, mpsc};
use tokio::task::JoinHandle;

use super::TranslatorError;
use super::pcm::CHUNK_SAMPLES;
pub use super::session::TranslatorOutput;
use crate::settings::Language;

/// Chunks waiting to be sent before the oldest is dropped (1 s of audio).
pub const DEFAULT_INPUT_QUEUE: usize = 5;

/// Outputs buffered for a slow consumer before the provider waits.
pub(crate) const OUTPUT_QUEUE: usize = 256;

/// A translation provider.
///
/// Implemented by [`OpenAiTranslator`](super::OpenAiTranslator) and
/// [`FakeTranslator`](super::FakeTranslator). Object safe, so the app can pick one at runtime
/// (`Arc<dyn Translator>`).
pub trait Translator: Send + Sync {
    /// Short provider name, for logs.
    fn name(&self) -> &'static str;

    /// Starts a session that translates into `target_language` (the source language is
    /// detected).
    fn start(
        &self,
        target_language: Language,
    ) -> BoxFuture<'_, Result<LiveSession, TranslatorError>>;
}

struct InputState {
    /// Samples waiting for a full chunk.
    pending: Vec<i16>,
    /// Full chunks waiting for the provider.
    queue: VecDeque<Vec<i16>>,
    capacity: usize,
    closed: bool,
    dropped_chunks: u64,
}

struct InputShared {
    state: Mutex<InputState>,
    ready: Notify,
}

impl InputShared {
    fn lock(&self) -> MutexGuard<'_, InputState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Where the pipeline pushes PCM16 mono 24 kHz audio. Cheap to clone; safe to use from audio
/// threads (it only takes a short lock and never waits for the provider).
///
/// Push everything the capture delivers, silence included: the model expects a continuous
/// stream.
#[derive(Clone)]
pub struct AudioInput {
    shared: Arc<InputShared>,
}

impl AudioInput {
    /// Queues samples (any length). Returns `false` once the session is stopped or closed; the
    /// samples are then discarded.
    pub fn push(&self, samples: &[i16]) -> bool {
        let mut state = self.shared.lock();
        if state.closed {
            return false;
        }
        let mut rest = samples;
        let mut completed = false;
        while !rest.is_empty() {
            let take = (CHUNK_SAMPLES - state.pending.len()).min(rest.len());
            state.pending.extend_from_slice(&rest[..take]);
            rest = &rest[take..];
            if state.pending.len() == CHUNK_SAMPLES {
                let chunk =
                    std::mem::replace(&mut state.pending, Vec::with_capacity(CHUNK_SAMPLES));
                enqueue(&mut state, chunk);
                completed = true;
            }
        }
        drop(state);
        if completed {
            self.shared.ready.notify_one();
        }
        true
    }

    /// Stops taking audio: the partial chunk is queued and the provider finishes the session
    /// once the queue is empty.
    pub fn close(&self) {
        let mut state = self.shared.lock();
        if state.closed {
            return;
        }
        state.closed = true;
        if !state.pending.is_empty() {
            let chunk = std::mem::take(&mut state.pending);
            enqueue(&mut state, chunk);
        }
        drop(state);
        self.shared.ready.notify_one();
    }

    /// Whether the input no longer takes audio.
    pub fn is_closed(&self) -> bool {
        self.shared.lock().closed
    }

    /// Chunks dropped so far because the provider fell behind.
    pub fn dropped_chunks(&self) -> u64 {
        self.shared.lock().dropped_chunks
    }

    /// Chunks waiting to be sent.
    pub fn queued_chunks(&self) -> usize {
        self.shared.lock().queue.len()
    }
}

fn enqueue(state: &mut InputState, chunk: Vec<i16>) {
    state.queue.push_back(chunk);
    while state.queue.len() > state.capacity {
        state.queue.pop_front();
        state.dropped_chunks += 1;
    }
}

impl std::fmt::Debug for AudioInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AudioInput")
    }
}

/// The provider's end of an [`AudioInput`]. Use from a single task.
pub(crate) struct InputReader {
    shared: Arc<InputShared>,
}

impl InputReader {
    /// The next chunk, waiting for one; `None` once the input is closed and drained.
    pub(crate) async fn next_chunk(&self) -> Option<Vec<i16>> {
        loop {
            let ready = self.shared.ready.notified();
            {
                let mut state = self.shared.lock();
                if let Some(chunk) = state.queue.pop_front() {
                    return Some(chunk);
                }
                if state.closed {
                    return None;
                }
            }
            ready.await;
        }
    }
}

/// A new input with room for `capacity` chunks (at least 1).
pub(crate) fn input_channel(capacity: usize) -> (AudioInput, InputReader) {
    let shared = Arc::new(InputShared {
        state: Mutex::new(InputState {
            pending: Vec::with_capacity(CHUNK_SAMPLES),
            queue: VecDeque::with_capacity(capacity.max(1) + 1),
            capacity: capacity.max(1),
            closed: false,
            dropped_chunks: 0,
        }),
        ready: Notify::new(),
    });
    (
        AudioInput {
            shared: shared.clone(),
        },
        InputReader { shared },
    )
}

/// A running translation session.
///
/// Read outputs until [`TranslatorOutput::Closed`]. To finish, call [`stop`](Self::stop) and keep
/// reading (the provider still delivers the translation of the audio it holds), or use
/// [`finish`](Self::finish). Dropping the session stops it too.
#[derive(Debug)]
pub struct LiveSession {
    input: AudioInput,
    outputs: mpsc::Receiver<TranslatorOutput>,
    /// `None` only after [`finish`](Self::finish) took it.
    task: Option<JoinHandle<()>>,
}

impl LiveSession {
    /// Assembles a session from a provider's driver task.
    pub(crate) fn new(
        input: AudioInput,
        outputs: mpsc::Receiver<TranslatorOutput>,
        task: JoinHandle<()>,
    ) -> Self {
        Self {
            input,
            outputs,
            task: Some(task),
        }
    }

    /// A handle to push audio from other threads (for example the capture thread).
    pub fn input(&self) -> AudioInput {
        self.input.clone()
    }

    /// Pushes PCM16 mono 24 kHz samples (see [`AudioInput::push`]).
    pub fn push(&self, samples: &[i16]) -> bool {
        self.input.push(samples)
    }

    /// The next output, or `None` after [`TranslatorOutput::Closed`].
    pub async fn next_output(&mut self) -> Option<TranslatorOutput> {
        self.outputs.recv().await
    }

    /// The next output if one is ready, without waiting.
    pub fn try_next_output(&mut self) -> Option<TranslatorOutput> {
        self.outputs.try_recv().ok()
    }

    /// Starts a graceful stop: no more audio is taken; the provider sends what is queued, then
    /// the remaining outputs and [`TranslatorOutput::Closed`].
    pub fn stop(&self) {
        self.input.close();
    }

    /// Stops, collects the remaining outputs and waits for the provider task to end.
    pub async fn finish(mut self) -> Vec<TranslatorOutput> {
        self.stop();
        let mut rest = Vec::new();
        while let Some(output) = self.outputs.recv().await {
            rest.push(output);
        }
        if let Some(task) = self.task.take()
            && let Err(error) = task.await
        {
            tracing::warn!(%error, "translator task ended abnormally");
        }
        rest
    }

    /// Chunks dropped so far because the provider fell behind.
    pub fn dropped_chunks(&self) -> u64 {
        self.input.dropped_chunks()
    }

    /// Whether the provider task has ended.
    pub fn is_finished(&self) -> bool {
        self.task.as_ref().is_none_or(JoinHandle::is_finished)
    }
}

impl Drop for LiveSession {
    fn drop(&mut self) {
        // The task notices the closed input or the dropped receiver and ends on its own.
        self.input.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn input_cuts_chunks_and_flushes_on_close() {
        let (input, reader) = input_channel(10);
        assert!(input.push(&vec![1; CHUNK_SAMPLES - 100]));
        assert!(input.push(&vec![2; 300]));
        input.close();
        assert!(!input.push(&[3]));

        let first = reader.next_chunk().await.unwrap();
        assert_eq!(first.len(), CHUNK_SAMPLES);
        assert_eq!(first[CHUNK_SAMPLES - 1], 2);
        assert_eq!(reader.next_chunk().await.unwrap(), vec![2; 200]);
        assert_eq!(reader.next_chunk().await, None);
    }

    #[tokio::test]
    async fn full_queue_drops_the_oldest_chunks() {
        let (input, reader) = input_channel(3);
        for value in 0..10i16 {
            input.push(&vec![value; CHUNK_SAMPLES]);
        }
        assert_eq!(input.dropped_chunks(), 7);
        assert_eq!(input.queued_chunks(), 3);
        input.close();
        let mut firsts = Vec::new();
        while let Some(chunk) = reader.next_chunk().await {
            firsts.push(chunk[0]);
        }
        assert_eq!(firsts, vec![7, 8, 9]);
    }

    #[tokio::test]
    async fn reader_wakes_up_on_push_from_another_thread() {
        let (input, reader) = input_channel(4);
        let pusher = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(20));
            input.push(&vec![5; CHUNK_SAMPLES]);
            input.close();
        });
        assert_eq!(reader.next_chunk().await.unwrap()[0], 5);
        assert_eq!(reader.next_chunk().await, None);
        pusher.join().unwrap();
    }
}
