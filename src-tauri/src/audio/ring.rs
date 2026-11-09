//! Bounded, lock-free audio queue between an audio thread and the rest of the app.
//!
//! A thin wrapper over the [`rtrb`] single-producer single-consumer ring buffer that works in
//! whole frames of interleaved `f32` samples and never blocks either side.
//!
//! **Overflow policy: drop the oldest audio, never accumulate latency.** The consumer keeps at
//! most `max_backlog_frames` queued: [`AudioConsumer::pop`] first discards the oldest frames
//! beyond that, so a consumer that fell behind jumps back to live audio. The ring itself holds
//! twice the backlog, so the producer only finds it full when the consumer stopped reading
//! altogether; it then drops the incoming frames (an SPSC producer cannot touch the read side),
//! counts them, and the consumer discards the stale backlog as soon as it reads again.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use rtrb::{Consumer, Producer, RingBuffer};

/// Creates a queue of `channels`-channel frames that keeps at most `max_backlog_frames` frames
/// for the consumer (see the module docs for the overflow policy).
pub fn audio_ring(channels: u16, max_backlog_frames: usize) -> (AudioProducer, AudioConsumer) {
    let channels = usize::from(channels.max(1));
    let max_backlog_frames = max_backlog_frames.max(1);
    let (producer, consumer) = RingBuffer::new(2 * max_backlog_frames * channels);
    let dropped = Arc::new(AtomicU64::new(0));
    (
        AudioProducer {
            inner: producer,
            channels,
            dropped: Arc::clone(&dropped),
        },
        AudioConsumer {
            inner: consumer,
            channels,
            max_backlog_frames,
            dropped,
        },
    )
}

/// Writing side, owned by the producing thread (for example the capture thread).
pub struct AudioProducer {
    inner: Producer<f32>,
    channels: usize,
    dropped: Arc<AtomicU64>,
}

impl AudioProducer {
    /// Queues as many whole frames of `samples` as fit and returns how many samples were queued.
    /// Never blocks or allocates; frames that do not fit are dropped and counted.
    pub fn push(&mut self, samples: &[f32]) -> usize {
        let frames = samples.len() / self.channels;
        let fit = (self.inner.slots() / self.channels).min(frames);
        let count = fit * self.channels;
        if count > 0
            && let Ok(mut chunk) = self.inner.write_chunk(count)
        {
            let (first, second) = chunk.as_mut_slices();
            let split = first.len();
            first.copy_from_slice(&samples[..split]);
            second.copy_from_slice(&samples[split..count]);
            chunk.commit_all();
        }
        let lost = frames - fit;
        if lost > 0 {
            self.dropped.fetch_add(lost as u64, Ordering::Relaxed);
        }
        count
    }

    /// Whether the consumer was dropped (nobody reads any more).
    pub fn is_abandoned(&self) -> bool {
        self.inner.is_abandoned()
    }
}

/// Reading side, owned by the consuming thread (for example the playback thread).
pub struct AudioConsumer {
    inner: Consumer<f32>,
    channels: usize,
    max_backlog_frames: usize,
    dropped: Arc<AtomicU64>,
}

impl AudioConsumer {
    /// Channels per frame.
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Whole frames waiting to be read.
    pub fn available_frames(&self) -> usize {
        self.inner.slots() / self.channels
    }

    /// Frames dropped so far by either side of the overflow policy.
    pub fn dropped_frames(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Discards the oldest frames so that at most `max_frames` stay queued; returns how many
    /// frames were discarded.
    pub fn drop_oldest_beyond(&mut self, max_frames: usize) -> usize {
        let excess = self.available_frames().saturating_sub(max_frames);
        if excess > 0
            && let Ok(chunk) = self.inner.read_chunk(excess * self.channels)
        {
            chunk.commit_all();
            self.dropped.fetch_add(excess as u64, Ordering::Relaxed);
        }
        excess
    }

    /// Reads up to `out.len()` samples (whole frames only), after discarding the backlog beyond
    /// `max_backlog_frames`. Returns how many samples were written to `out`. Never blocks.
    pub fn pop(&mut self, out: &mut [f32]) -> usize {
        self.drop_oldest_beyond(self.max_backlog_frames);
        let frames = (out.len() / self.channels).min(self.available_frames());
        let count = frames * self.channels;
        if count > 0
            && let Ok(chunk) = self.inner.read_chunk(count)
        {
            let (first, second) = chunk.as_slices();
            out[..first.len()].copy_from_slice(first);
            out[first.len()..count].copy_from_slice(second);
            chunk.commit_all();
        }
        count
    }

    /// Whether the producer was dropped (the stream that fed this queue stopped).
    pub fn is_abandoned(&self) -> bool {
        self.inner.is_abandoned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(frames: usize, channels: usize) -> Vec<f32> {
        (0..frames * channels).map(|i| i as f32).collect()
    }

    #[test]
    fn passes_whole_frames_through_in_order() {
        let (mut producer, mut consumer) = audio_ring(2, 8);
        assert_eq!(producer.push(&ramp(3, 2)), 6);

        let mut out = [0.0; 6];
        assert_eq!(consumer.pop(&mut out), 6);
        assert_eq!(out, [0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(consumer.dropped_frames(), 0);
    }

    #[test]
    fn consumer_that_fell_behind_drops_the_oldest_frames() {
        let (mut producer, mut consumer) = audio_ring(1, 4);
        // 6 frames queued, but at most 4 may wait: frames 0 and 1 are the oldest and go.
        producer.push(&ramp(6, 1));

        let mut out = [0.0; 8];
        let read = consumer.pop(&mut out);

        assert_eq!(&out[..read], &[2.0, 3.0, 4.0, 5.0]);
        assert_eq!(consumer.dropped_frames(), 2);
    }

    #[test]
    fn producer_never_blocks_when_the_ring_is_full() {
        // Capacity is twice the backlog: 4 frames of 2 channels.
        let (mut producer, mut consumer) = audio_ring(2, 2);
        assert_eq!(producer.push(&ramp(3, 2)), 6);
        // Only one more frame fits; the other is dropped and counted.
        assert_eq!(producer.push(&ramp(2, 2)), 2);
        assert_eq!(consumer.dropped_frames(), 1);

        // On the next read the consumer keeps only the newest 2 frames.
        let mut out = [0.0; 8];
        assert_eq!(consumer.pop(&mut out), 4);
        assert_eq!(&out[..4], &[4.0, 5.0, 0.0, 1.0]);
        assert_eq!(consumer.dropped_frames(), 3);
    }

    #[test]
    fn never_splits_a_frame() {
        let (mut producer, mut consumer) = audio_ring(2, 8);
        // 5 samples = 2 whole frames + half a frame, which is not queued.
        assert_eq!(producer.push(&[1.0, 2.0, 3.0, 4.0, 5.0]), 4);

        // Room for 3 samples reads one whole frame.
        let mut out = [0.0; 3];
        assert_eq!(consumer.pop(&mut out), 2);
        assert_eq!(consumer.available_frames(), 1);
    }

    #[test]
    fn wraps_around_the_end_of_the_ring() {
        let (mut producer, mut consumer) = audio_ring(1, 2);
        let mut out = [0.0; 4];
        for round in 0..5 {
            let base = round as f32 * 10.0;
            producer.push(&[base, base + 1.0, base + 2.0]);
            let read = consumer.pop(&mut out);
            assert_eq!(&out[..read], &[base + 1.0, base + 2.0], "round {round}");
        }
    }

    #[test]
    fn reports_when_the_other_side_is_gone() {
        let (producer, consumer) = audio_ring(1, 2);
        drop(producer);
        assert!(consumer.is_abandoned());

        let (producer, consumer) = audio_ring(1, 2);
        drop(consumer);
        assert!(producer.is_abandoned());
    }
}
