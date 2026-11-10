//! Per-utterance latency of a translation direction.
//!
//! The input is cut into utterances with a simple level detector: speech starts when a block's
//! RMS reaches [`SPEECH_RMS`] and ends when it stays below it for more than [`END_SILENCE`] (the
//! end time is when the quiet began). For every utterance two times are measured, both against
//! the wall clock at which translated audio deltas are *received* from the translator:
//!
//! - **end → audio**: from the end of speech to the first translated audio received at or after
//!   that point. It answers "after I stop talking, how long until the others hear something?".
//!   If the translation of a sentence was still playing out when the user stopped, this is short;
//!   if the model waits for the end of the sentence, this is the model's tail latency.
//! - **start → audio**: from the start of speech to the first translated audio that begins a new
//!   output burst (a delta after at least [`OUTPUT_GAP`] without deltas, or the very first one)
//!   at or after that start. Bursts are matched to utterances in order, one each, so audio that
//!   belongs to the previous sentence is not counted against the next one.
//!
//! Both are estimates: the model translates simultaneously and may merge or split sentences. The
//! jitter prebuffer and device buffers (a few hundred ms) come on top.

use std::collections::VecDeque;
use std::time::Duration;

/// Input RMS (24 kHz mono, ~-34 dBFS) at or above which a block counts as speech.
pub const SPEECH_RMS: f32 = 0.02;

/// Quiet input needed to end an utterance.
pub const END_SILENCE: Duration = Duration::from_millis(400);

/// No translated audio for this long means the next delta starts a new output burst.
pub const OUTPUT_GAP: Duration = Duration::from_millis(300);

/// Utterances kept for the report.
const MAX_UTTERANCES: usize = 256;

/// One utterance and what was measured for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Utterance {
    pub start: Duration,
    /// `None` while the user is still talking.
    pub end: Option<Duration>,
    pub start_to_audio: Option<Duration>,
    pub end_to_audio: Option<Duration>,
}

/// Something worth logging that happened while feeding the tracker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatencyEvent {
    SpeechStart { at: Duration },
    SpeechEnd { at: Duration },
    StartToAudio { utterance: usize, latency: Duration },
    EndToAudio { utterance: usize, latency: Duration },
}

/// Min / median / max of a set of measurements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spread {
    pub count: usize,
    pub min: Duration,
    pub median: Duration,
    pub max: Duration,
}

impl Spread {
    /// `None` for an empty set.
    pub fn of(values: impl IntoIterator<Item = Duration>) -> Option<Self> {
        let mut values: Vec<_> = values.into_iter().collect();
        if values.is_empty() {
            return None;
        }
        values.sort_unstable();
        Some(Self {
            count: values.len(),
            min: values[0],
            median: values[values.len() / 2],
            max: values[values.len() - 1],
        })
    }
}

impl std::fmt::Display for Spread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "n={} min={}ms median={}ms max={}ms",
            self.count,
            self.min.as_millis(),
            self.median.as_millis(),
            self.max.as_millis()
        )
    }
}

/// The summary at stop.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LatencyReport {
    pub utterances: Vec<Utterance>,
    pub start_to_audio: Option<Spread>,
    pub end_to_audio: Option<Spread>,
}

impl std::fmt::Display for LatencyReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let show = |spread: &Option<Spread>| spread.map_or("none".to_owned(), |s| s.to_string());
        write!(
            f,
            "utterances={} start->audio [{}] end->audio [{}]",
            self.utterances.len(),
            show(&self.start_to_audio),
            show(&self.end_to_audio)
        )
    }
}

/// See the [module docs](self). Feed it input blocks and output deltas in time order.
#[derive(Debug, Default)]
pub struct LatencyTracker {
    utterances: VecDeque<Utterance>,
    /// Since when the input has been quiet during an utterance.
    quiet_since: Option<Duration>,
    speaking: bool,
    /// First output received since `quiet_since` (the end is only known later).
    output_while_quiet: Option<Duration>,
    last_output: Option<Duration>,
    /// Index (in `utterances`) of the next utterance waiting for an output burst.
    next_burst_owner: usize,
}

impl LatencyTracker {
    /// A block of input (its RMS, linear 0–1) sent at `at`.
    pub fn on_input(&mut self, rms: f32, at: Duration) -> Option<LatencyEvent> {
        let speech = rms >= SPEECH_RMS;
        if speech {
            self.quiet_since = None;
            self.output_while_quiet = None;
            if !self.speaking {
                self.speaking = true;
                if self.utterances.len() == MAX_UTTERANCES {
                    self.utterances.pop_front();
                    self.next_burst_owner = self.next_burst_owner.saturating_sub(1);
                }
                self.utterances.push_back(Utterance {
                    start: at,
                    end: None,
                    start_to_audio: None,
                    end_to_audio: None,
                });
                return Some(LatencyEvent::SpeechStart { at });
            }
            return None;
        }
        if !self.speaking {
            return None;
        }
        let quiet_since = *self.quiet_since.get_or_insert(at);
        if at.saturating_sub(quiet_since) > END_SILENCE {
            self.speaking = false;
            self.quiet_since = None;
            if let Some(last) = self.utterances.back_mut() {
                last.end = Some(quiet_since);
                // Audio that arrived while the end was not yet certain counts.
                last.end_to_audio = self
                    .output_while_quiet
                    .take()
                    .map(|output| output - quiet_since);
            }
            return Some(LatencyEvent::SpeechEnd { at: quiet_since });
        }
        None
    }

    /// A translated audio delta received at `at`.
    pub fn on_output(&mut self, at: Duration) -> Vec<LatencyEvent> {
        let mut events = Vec::new();
        let new_burst = self
            .last_output
            .is_none_or(|last| at.saturating_sub(last) >= OUTPUT_GAP);
        self.last_output = Some(at);
        if let Some(quiet_since) = self.quiet_since
            && self.speaking
            && at >= quiet_since
            && self.output_while_quiet.is_none()
        {
            self.output_while_quiet = Some(at);
        }

        for (index, utterance) in self.utterances.iter_mut().enumerate() {
            if let Some(end) = utterance.end
                && utterance.end_to_audio.is_none()
                && at >= end
            {
                let latency = at - end;
                utterance.end_to_audio = Some(latency);
                events.push(LatencyEvent::EndToAudio {
                    utterance: index,
                    latency,
                });
            }
        }

        if new_burst
            && let Some(utterance) = self.utterances.get_mut(self.next_burst_owner)
            && at >= utterance.start
        {
            let latency = at - utterance.start;
            utterance.start_to_audio = Some(latency);
            events.push(LatencyEvent::StartToAudio {
                utterance: self.next_burst_owner,
                latency,
            });
            self.next_burst_owner += 1;
        }
        events
    }

    /// Everything measured so far.
    pub fn report(&self) -> LatencyReport {
        let utterances: Vec<_> = self.utterances.iter().copied().collect();
        LatencyReport {
            start_to_audio: Spread::of(utterances.iter().filter_map(|u| u.start_to_audio)),
            end_to_audio: Spread::of(utterances.iter().filter_map(|u| u.end_to_audio)),
            utterances,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    /// Feeds 10 ms input blocks from `from` to `to` (exclusive) at `rms`.
    fn speak(tracker: &mut LatencyTracker, from: u64, to: u64, rms: f32) {
        for at in (from..to).step_by(10) {
            tracker.on_input(rms, ms(at));
        }
    }

    #[test]
    fn measures_each_utterance_against_its_own_audio() {
        // Sentence 1: speech 0–2000 ms; its translation arrives 1200–2600 ms, a delta every
        // 100 ms. Sentence 2: speech 4000–5000 ms; its translation arrives 5300–6000 ms.
        let mut tracker = LatencyTracker::default();
        let mut outputs: Vec<u64> = (1200..=2600).step_by(100).collect();
        outputs.extend((5300..=6000).step_by(100));
        let mut inputs: Vec<(u64, f32)> = Vec::new();
        for at in (0..6000).step_by(10) {
            let rms = if at < 2000 || (4000..5000).contains(&at) {
                0.2
            } else {
                0.0
            };
            inputs.push((at, rms));
        }
        // Merge both timelines.
        let (mut i, mut o) = (0, 0);
        while i < inputs.len() || o < outputs.len() {
            let take_input = o == outputs.len() || (i < inputs.len() && inputs[i].0 <= outputs[o]);
            if take_input {
                tracker.on_input(inputs[i].1, ms(inputs[i].0));
                i += 1;
            } else {
                tracker.on_output(ms(outputs[o]));
                o += 1;
            }
        }
        let report = tracker.report();
        assert_eq!(report.utterances.len(), 2, "{report:?}");
        let first = report.utterances[0];
        assert_eq!(first.end, Some(ms(2000)));
        assert_eq!(first.start_to_audio, Some(ms(1200)));
        // First delta at or after 2000 ms: 2000 ms itself.
        assert_eq!(first.end_to_audio, Some(ms(0)));
        let second = report.utterances[1];
        assert_eq!(second.start, ms(4000));
        assert_eq!(second.end, Some(ms(5000)));
        // Not 2600 ms of the previous sentence: the next burst at 5300 ms.
        assert_eq!(second.start_to_audio, Some(ms(1300)));
        assert_eq!(second.end_to_audio, Some(ms(300)));

        let start = report.start_to_audio.unwrap();
        assert_eq!((start.min, start.max, start.count), (ms(1200), ms(1300), 2));
    }

    #[test]
    fn short_pauses_do_not_split_an_utterance() {
        let mut tracker = LatencyTracker::default();
        speak(&mut tracker, 0, 500, 0.2);
        speak(&mut tracker, 500, 800, 0.0);
        speak(&mut tracker, 800, 1500, 0.2);
        speak(&mut tracker, 1500, 2000, 0.0);
        let report = tracker.report();
        assert_eq!(report.utterances.len(), 1);
        assert_eq!(report.utterances[0].end, Some(ms(1500)));
    }

    #[test]
    fn output_before_any_speech_is_ignored_and_spread_is_ordered() {
        let mut tracker = LatencyTracker::default();
        assert!(tracker.on_output(ms(10)).is_empty());
        assert_eq!(tracker.report(), LatencyReport::default());

        let spread = Spread::of([ms(300), ms(100), ms(200)]).unwrap();
        assert_eq!(
            (spread.min, spread.median, spread.max),
            (ms(100), ms(200), ms(300))
        );
        assert_eq!(spread.to_string(), "n=3 min=100ms median=200ms max=300ms");
        assert_eq!(Spread::of([]), None);
    }
}
