//! Playback-side jitter buffer for translated audio.
//!
//! The translator delivers audio in deltas of irregular size and timing, while the playback
//! device pulls a steady amount every few milliseconds. [`JitterBuffer`] sits between them: it
//! waits until [`prebuffer`](JitterBuffer::new) worth of audio has arrived before it starts
//! playing, plays silence when it runs dry (and then waits for the prebuffer again), and caps its
//! size so delay cannot build up without bound.
//!
//! [`buffered_samples`](JitterBuffer::buffered_samples) is the input for clock drift
//! correction: a controller compares it to the prebuffer target and nudges
//! `audio::resample::FromTranslator::set_rate_adjust` (see that module).
//!
//! It works on PCM16 mono 24 kHz samples, before conversion to the device format. It is a plain
//! single-owner struct; share it behind a mutex if producer and consumer live on different
//! threads. It never allocates after construction.

use std::collections::VecDeque;
use std::time::Duration;

use super::pcm::SAMPLE_RATE;

/// Audio collected before playback starts (and restarts after running dry).
pub const DEFAULT_PREBUFFER: Duration = Duration::from_millis(200);

/// Most audio held; beyond it the oldest samples are dropped.
pub const DEFAULT_MAX_BUFFER: Duration = Duration::from_secs(3);

fn samples_for(duration: Duration) -> usize {
    (duration.as_secs_f64() * f64::from(SAMPLE_RATE)).round() as usize
}

/// See the [module docs](self).
#[derive(Debug, Clone)]
pub struct JitterBuffer {
    samples: VecDeque<i16>,
    prebuffer: usize,
    max: usize,
    playing: bool,
    underruns: u64,
    dropped_samples: u64,
}

impl Default for JitterBuffer {
    fn default() -> Self {
        Self::new(DEFAULT_PREBUFFER, DEFAULT_MAX_BUFFER)
    }
}

impl JitterBuffer {
    /// A buffer that starts playing once `prebuffer` is queued and never holds more than `max`
    /// (raised to `prebuffer` if smaller).
    pub fn new(prebuffer: Duration, max: Duration) -> Self {
        let prebuffer = samples_for(prebuffer).max(1);
        let max = samples_for(max).max(prebuffer);
        Self {
            samples: VecDeque::with_capacity(max),
            prebuffer,
            max,
            playing: false,
            underruns: 0,
            dropped_samples: 0,
        }
    }

    /// Adds translated audio. If the buffer would exceed its maximum, the oldest samples go.
    pub fn push(&mut self, samples: &[i16]) {
        // Only the newest `max` samples can survive.
        let samples = &samples[samples.len().saturating_sub(self.max)..];
        let overflow = (self.samples.len() + samples.len()).saturating_sub(self.max);
        if overflow > 0 {
            self.samples.drain(..overflow);
            self.dropped_samples += overflow as u64;
        }
        self.samples.extend(samples.iter().copied());
        if !self.playing && self.samples.len() >= self.prebuffer {
            self.playing = true;
        }
    }

    /// Fills `out` completely: with audio while playing, with silence while buffering or after
    /// running dry. Returns how many samples were real audio.
    pub fn pull(&mut self, out: &mut [i16]) -> usize {
        if !self.playing {
            out.fill(0);
            return 0;
        }
        let real = out.len().min(self.samples.len());
        for (slot, sample) in out.iter_mut().zip(self.samples.drain(..real)) {
            *slot = sample;
        }
        out[real..].fill(0);
        if self.samples.is_empty() {
            // Ran dry (end of an utterance or a late delta): collect a prebuffer again.
            self.playing = false;
            if real < out.len() {
                self.underruns += 1;
            }
        }
        real
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

    /// Whether audio is being played (as opposed to collecting the prebuffer).
    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Times playback ran out of audio mid-pull. Includes the natural end of every utterance.
    pub fn underruns(&self) -> u64 {
        self.underruns
    }

    /// Samples dropped because the buffer was full.
    pub fn dropped_samples(&self) -> u64 {
        self.dropped_samples
    }

    /// Empties the buffer and waits for a new prebuffer.
    pub fn clear(&mut self) {
        self.samples.clear();
        self.playing = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer() -> JitterBuffer {
        // 10 ms prebuffer (240 samples), 50 ms max (1200 samples).
        JitterBuffer::new(Duration::from_millis(10), Duration::from_millis(50))
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
    fn running_dry_pads_with_silence_and_rebuffers() {
        let mut jitter = buffer();
        jitter.push(&[3; 250]);
        let mut out = [9i16; 300];
        assert_eq!(jitter.pull(&mut out), 250);
        assert!(out[..250].iter().all(|&s| s == 3));
        assert!(out[250..].iter().all(|&s| s == 0));
        assert_eq!(jitter.underruns(), 1);
        assert!(!jitter.is_playing());

        // Below the prebuffer again: silence.
        jitter.push(&[4; 100]);
        assert_eq!(jitter.pull(&mut out), 0);
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
    fn clear_resets_playback() {
        let mut jitter = JitterBuffer::default();
        jitter.push(&[1; 4800]);
        assert!(jitter.is_playing());
        jitter.clear();
        assert!(!jitter.is_playing());
        assert_eq!(jitter.buffered_samples(), 0);
    }
}
