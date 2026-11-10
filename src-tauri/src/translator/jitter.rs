//! Playback-side jitter buffer for translated audio.
//!
//! The translator delivers audio in deltas of irregular size and timing (often in bursts faster
//! than real time, sometimes late), while the playback device pulls a steady amount every few
//! milliseconds. [`JitterBuffer`] sits between them:
//!
//! - it waits until [`prebuffer`](JitterBuffer::new) worth of audio has arrived before it starts
//!   playing (or until the first audio has waited that long, so a short final word still plays);
//! - while playing, a short dry spell (a late delta mid-utterance) plays silence **without**
//!   resetting: as soon as audio arrives again it plays right away. Only after a real gap of
//!   [`gap`](JitterBuffer::with_gap) without audio (the end of an utterance) does it go back to
//!   collecting a prebuffer;
//! - it never drops translated speech because of a burst: the cap is large
//!   ([`DEFAULT_MAX_BUFFER`]) and the drift controller is expected to drain a backlog by playing
//!   slightly faster. If the cap is ever reached, the oldest audio goes and it is counted in
//!   [`dropped_samples`](JitterBuffer::dropped_samples) (and logged);
//! - [`finish_input`](JitterBuffer::finish_input) plays whatever is left, however short, so the
//!   tail of the last sentence is heard when the session stops.
//!
//! [`buffered_samples`](JitterBuffer::buffered_samples) is the input for clock drift
//! correction: a controller compares it to the prebuffer target and nudges
//! `audio::resample::FromTranslator::set_rate_adjust` (see that module).
//!
//! It works on PCM16 mono 24 kHz samples, before conversion to the device format. It is a plain
//! single-owner struct; share it behind a mutex if producer and consumer live on different
//! threads. It only allocates when its backlog outgrows what it held before.

use std::collections::VecDeque;
use std::time::Duration;

use super::pcm::SAMPLE_RATE;

/// Audio collected before playback starts (and restarts after the end of an utterance).
pub const DEFAULT_PREBUFFER: Duration = Duration::from_millis(200);

/// Silence after which a dry buffer counts as the end of an utterance and prebuffers again.
pub const DEFAULT_GAP: Duration = Duration::from_millis(400);

/// Most audio held; beyond it the oldest samples are dropped. Large on purpose: the model often
/// delivers several seconds at once, and that speech must not be lost.
pub const DEFAULT_MAX_BUFFER: Duration = Duration::from_secs(30);

fn samples_for(duration: Duration) -> usize {
    (duration.as_secs_f64() * f64::from(SAMPLE_RATE)).round() as usize
}

fn ms_for(samples: usize) -> u64 {
    (samples as u64 * 1000) / u64::from(SAMPLE_RATE)
}

/// What the buffer does right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Collecting the prebuffer. `waited` counts the samples pulled while audio was queued.
    Buffering { waited: usize },
    /// Playing audio. `dry` counts the silence played since the buffer ran dry (0 = has audio).
    Playing { dry: usize },
}

/// Counters for diagnostics; see the [module docs](self).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JitterStats {
    /// Dry spells mid-utterance: playback ran out and audio came back before the gap (audible
    /// as a short silence inside a sentence).
    pub underruns: u64,
    /// Times playback started (roughly one per translated utterance).
    pub utterances: u64,
    /// Samples dropped because the buffer was full (should stay 0).
    pub dropped_samples: u64,
    /// Most samples held at once.
    pub max_buffered_samples: usize,
    /// Samples received.
    pub pushed_samples: u64,
    /// Real audio samples played.
    pub played_samples: u64,
}

impl JitterStats {
    /// [`max_buffered_samples`](Self::max_buffered_samples) in milliseconds.
    pub fn max_buffered_ms(&self) -> u64 {
        ms_for(self.max_buffered_samples)
    }
}

/// See the [module docs](self).
#[derive(Debug, Clone)]
pub struct JitterBuffer {
    samples: VecDeque<i16>,
    prebuffer: usize,
    gap: usize,
    max: usize,
    state: State,
    /// Set by [`finish_input`](Self::finish_input): play everything, never prebuffer again.
    draining: bool,
    stats: JitterStats,
}

impl Default for JitterBuffer {
    fn default() -> Self {
        Self::new(DEFAULT_PREBUFFER, DEFAULT_MAX_BUFFER)
    }
}

impl JitterBuffer {
    /// A buffer that starts playing once `prebuffer` is queued and never holds more than `max`
    /// (raised to `prebuffer` if smaller). The end-of-utterance gap is [`DEFAULT_GAP`].
    pub fn new(prebuffer: Duration, max: Duration) -> Self {
        let prebuffer = samples_for(prebuffer).max(1);
        let max = samples_for(max).max(prebuffer);
        Self {
            samples: VecDeque::with_capacity(samples_for(Duration::from_secs(5)).min(max)),
            prebuffer,
            gap: samples_for(DEFAULT_GAP).max(1),
            max,
            state: State::Buffering { waited: 0 },
            draining: false,
            stats: JitterStats::default(),
        }
    }

    /// Changes the silence after which a dry buffer prebuffers again.
    pub fn with_gap(mut self, gap: Duration) -> Self {
        self.gap = samples_for(gap).max(1);
        self
    }

    /// Adds translated audio. If the buffer would exceed its maximum, the oldest samples go.
    pub fn push(&mut self, samples: &[i16]) {
        self.stats.pushed_samples += samples.len() as u64;
        // Only the newest `max` samples can survive.
        let samples = &samples[samples.len().saturating_sub(self.max)..];
        let overflow = (self.samples.len() + samples.len()).saturating_sub(self.max);
        if overflow > 0 {
            self.samples.drain(..overflow.min(self.samples.len()));
            self.stats.dropped_samples += overflow as u64;
            tracing::warn!(
                dropped_ms = ms_for(overflow),
                max_ms = ms_for(self.max),
                "jitter buffer full: dropped the oldest translated audio"
            );
        }
        self.samples.extend(samples.iter().copied());
        self.stats.max_buffered_samples = self.stats.max_buffered_samples.max(self.samples.len());
        if let State::Buffering { .. } = self.state
            && (self.samples.len() >= self.prebuffer || (self.draining && !self.samples.is_empty()))
        {
            self.start_playing();
        }
    }

    fn start_playing(&mut self) {
        self.state = State::Playing { dry: 0 };
        self.stats.utterances += 1;
    }

    /// Fills `out` completely: with audio while playing, with silence while buffering or during a
    /// dry spell. Returns how many samples were real audio.
    pub fn pull(&mut self, out: &mut [i16]) -> usize {
        if let State::Buffering { waited } = self.state {
            if self.samples.is_empty() {
                out.fill(0);
                return 0;
            }
            // Audio is queued but below the prebuffer: start anyway once it has waited as long
            // as the prebuffer would take (a short last word), or right away when draining.
            let waited = waited + out.len();
            if self.draining || waited >= self.prebuffer {
                self.start_playing();
            } else {
                self.state = State::Buffering { waited };
                out.fill(0);
                return 0;
            }
        }

        let real = out.len().min(self.samples.len());
        for (slot, sample) in out.iter_mut().zip(self.samples.drain(..real)) {
            *slot = sample;
        }
        out[real..].fill(0);
        self.stats.played_samples += real as u64;

        let State::Playing { dry } = self.state else {
            return real;
        };
        let silence = out.len() - real;
        let dry = if real > 0 {
            if dry > 0 {
                // Audio came back before the gap: a mid-utterance dry spell.
                self.stats.underruns += 1;
            }
            silence
        } else {
            dry + silence
        };
        self.state = if dry >= self.gap && !self.draining {
            // End of an utterance: collect a prebuffer before the next one.
            State::Buffering { waited: 0 }
        } else {
            State::Playing { dry }
        };
        real
    }

    /// No more audio will come (the session is closing): play what is left right away, however
    /// short, and do not wait for a prebuffer any more.
    pub fn finish_input(&mut self) {
        self.draining = true;
        if let State::Buffering { .. } = self.state
            && !self.samples.is_empty()
        {
            self.start_playing();
        }
    }

    /// Whether [`finish_input`](Self::finish_input) was called and everything has been played.
    pub fn is_drained(&self) -> bool {
        self.draining && self.samples.is_empty()
    }

    /// Samples queued right now.
    pub fn buffered_samples(&self) -> usize {
        self.samples.len()
    }

    /// Audio queued right now.
    pub fn buffered(&self) -> Duration {
        Duration::from_secs_f64(self.samples.len() as f64 / f64::from(SAMPLE_RATE))
    }

    /// The prebuffer target in samples (the level a drift controller aims for).
    pub fn target_samples(&self) -> usize {
        self.prebuffer
    }

    /// Whether audio is being played (as opposed to collecting the prebuffer). Stays true
    /// through short dry spells.
    pub fn is_playing(&self) -> bool {
        matches!(self.state, State::Playing { .. })
    }

    /// Mid-utterance dry spells so far (see [`JitterStats::underruns`]).
    pub fn underruns(&self) -> u64 {
        self.stats.underruns
    }

    /// Samples dropped because the buffer was full.
    pub fn dropped_samples(&self) -> u64 {
        self.stats.dropped_samples
    }

    /// All counters.
    pub fn stats(&self) -> JitterStats {
        self.stats
    }

    /// Empties the buffer and waits for a new prebuffer.
    pub fn clear(&mut self) {
        self.samples.clear();
        self.state = State::Buffering { waited: 0 };
        self.draining = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 10 ms per pull, like the outgoing pump.
    const PULL: usize = 240;

    fn buffer() -> JitterBuffer {
        // 10 ms prebuffer (240 samples), 50 ms max (1200 samples), 20 ms gap (480 samples).
        JitterBuffer::new(Duration::from_millis(10), Duration::from_millis(50))
            .with_gap(Duration::from_millis(20))
    }

    #[test]
    fn plays_silence_until_the_prebuffer_is_reached() {
        let mut jitter = buffer();
        let mut out = [7i16; 100];
        jitter.push(&[1; 200]);
        assert_eq!(jitter.pull(&mut out), 0);
        assert!(out.iter().all(|&s| s == 0));
        assert!(!jitter.is_playing());
        assert_eq!(jitter.buffered_samples(), 200);

        jitter.push(&[2; 40]);
        assert!(jitter.is_playing());
        assert_eq!(jitter.pull(&mut out), 100);
        assert!(out.iter().all(|&s| s == 1));
        assert_eq!(jitter.buffered_samples(), 140);
        assert_eq!(jitter.target_samples(), 240);
    }

    #[test]
    fn short_audio_below_the_prebuffer_still_plays_after_waiting() {
        let mut jitter = buffer();
        let mut out = [0i16; 100];
        jitter.push(&[5; 50]);
        assert_eq!(jitter.pull(&mut out), 0);
        assert_eq!(jitter.pull(&mut out), 0);
        // 300 samples pulled >= 240 prebuffer: it plays.
        assert_eq!(jitter.pull(&mut out), 50);
    }

    #[test]
    fn a_short_dry_spell_plays_silence_and_resumes_immediately() {
        let mut jitter = buffer();
        let mut out = [9i16; 100];
        jitter.push(&[3; 250]);
        assert_eq!(jitter.pull(&mut [0; 300]), 250);
        assert!(
            jitter.is_playing(),
            "a dry spell shorter than the gap keeps playing"
        );

        // A late delta, smaller than the prebuffer: it plays right away.
        jitter.push(&[4; 100]);
        assert_eq!(jitter.pull(&mut out), 100);
        assert!(out.iter().all(|&s| s == 4));
        assert_eq!(jitter.underruns(), 1);
    }

    #[test]
    fn a_real_gap_prebuffers_again() {
        let mut jitter = buffer();
        jitter.push(&[3; 250]);
        let mut out = [0i16; 300];
        assert_eq!(jitter.pull(&mut out), 250);
        // 50 + 300 + 300 samples of silence > 480 gap.
        jitter.pull(&mut out);
        jitter.pull(&mut out);
        assert!(!jitter.is_playing());

        jitter.push(&[4; 100]);
        assert_eq!(jitter.pull(&mut [0; 100]), 0, "waits for a prebuffer again");
        assert_eq!(
            jitter.underruns(),
            0,
            "the end of an utterance is not an underrun"
        );
        assert_eq!(jitter.stats().utterances, 1);
    }

    #[test]
    fn caps_the_buffer_by_dropping_the_oldest() {
        let mut jitter = buffer();
        jitter.push(&[1; 1000]);
        jitter.push(&[2; 500]);
        assert_eq!(jitter.buffered_samples(), 1200);
        assert_eq!(jitter.dropped_samples(), 300);
        let mut out = [0i16; 1200];
        jitter.pull(&mut out);
        assert!(out[..700].iter().all(|&s| s == 1));
        assert!(out[700..].iter().all(|&s| s == 2));

        jitter.push(&[5; 5000]);
        assert_eq!(jitter.buffered_samples(), 1200);
        assert_eq!(jitter.buffered(), Duration::from_millis(50));
    }

    #[test]
    fn finish_input_plays_a_short_tail() {
        let mut jitter = buffer();
        jitter.push(&[6; 30]);
        jitter.finish_input();
        assert!(jitter.is_playing());
        assert!(!jitter.is_drained());
        let mut out = [0i16; 100];
        assert_eq!(jitter.pull(&mut out), 30);
        assert!(jitter.is_drained());
        // Never prebuffers again while draining, however long the silence.
        for _ in 0..10 {
            jitter.pull(&mut out);
        }
        assert!(jitter.is_playing());
    }

    #[test]
    fn clear_resets_playback() {
        let mut jitter = JitterBuffer::default();
        jitter.push(&[1; 4800]);
        assert!(jitter.is_playing());
        jitter.clear();
        assert!(!jitter.is_playing());
        assert_eq!(jitter.buffered_samples(), 0);
    }

    /// Feeds `deltas` (arrival time in ms, sample count) to a default buffer pulled 10 ms at a
    /// time for `total_ms`, returning the buffer and the real samples played.
    fn simulate(deltas: &[(u64, usize)], total_ms: u64) -> (JitterBuffer, u64) {
        let mut jitter = JitterBuffer::default();
        let mut out = [0i16; PULL];
        let mut next = 0;
        let mut played = 0;
        for now in (0..total_ms).step_by(10) {
            while next < deltas.len() && deltas[next].0 <= now {
                jitter.push(&vec![1000; deltas[next].1]);
                next += 1;
            }
            played += jitter.pull(&mut out) as u64;
        }
        (jitter, played)
    }

    #[test]
    fn a_20_second_burst_is_never_dropped() {
        // The model delivers 20 s of speech within 2 s (10x real time).
        let deltas: Vec<_> = (0..100).map(|i| (i * 20, 4800)).collect();
        let (jitter, played) = simulate(&deltas, 25_000);
        let stats = jitter.stats();
        assert_eq!(stats.dropped_samples, 0);
        assert_eq!(played, 100 * 4800);
        assert!(
            stats.max_buffered_ms() > 17_000,
            "{}",
            stats.max_buffered_ms()
        );
        assert_eq!(stats.underruns, 0);
        assert_eq!(stats.utterances, 1);
    }

    #[test]
    fn late_deltas_mid_utterance_do_not_restart_the_prebuffer() {
        // 100 ms deltas every 100 ms, but every fourth one arrives 150 ms late.
        let mut deltas = Vec::new();
        for i in 0..40u64 {
            let late = if i % 4 == 3 { 150 } else { 0 };
            deltas.push((i * 100 + late, 2400));
        }
        deltas.sort_unstable();
        let (jitter, played) = simulate(&deltas, 6_000);
        let stats = jitter.stats();
        assert_eq!(played, 40 * 2400, "everything is played");
        assert_eq!(stats.utterances, 1, "one utterance, no re-prebuffering");
        assert_eq!(stats.dropped_samples, 0);
    }

    #[test]
    fn utterances_separated_by_a_pause_prebuffer_each_time() {
        // Two sentences of 1 s each, 2 s apart.
        let mut deltas: Vec<_> = (0..10).map(|i| (i * 100, 2400)).collect();
        deltas.extend((0..10).map(|i| (3_000 + i * 100, 2400)));
        let (jitter, played) = simulate(&deltas, 5_000);
        assert_eq!(played, 20 * 2400);
        assert_eq!(jitter.stats().utterances, 2);
    }
}
