//! Local voice activity detection: what is appended to the translation session, and when a
//! direction that hears nobody turns itself off.
//!
//! # Detector
//!
//! [`VoiceDetector`] works on the translator's input (PCM16 mono 24 kHz) in 10 ms frames. A
//! frame is *loud* when its level is at least [`FLOOR_MARGIN_DB`] above the noise floor and
//! above [`MIN_SPEECH_DBFS`]:
//! - the **noise floor** is the quietest frame of the last [`FLOOR_WINDOW`] ("minimum
//!   statistics"): speech always has quieter gaps between syllables and words within a few
//!   seconds, so a sentence does not raise it, while steady background noise (a fan, a game's
//!   ambience) becomes the floor within [`FLOOR_WINDOW`];
//! - **onset:** voice starts after [`ONSET_FRAMES`] loud frames out of the last
//!   [`ONSET_WINDOW`], so a click or a key press does not count;
//! - **hangover:** voice ends [`HANGOVER`] after the last loud frame, so the short pauses between
//!   words do not end it.
//!
//! It is deliberately simple (energy against an adaptive floor, no spectral model): it never
//! has to tell speech from music, it only has to find stretches where nothing is said. Anything
//! loud and steady enough (music in a game) counts as voice, which only costs what streaming
//! everything already did.
//!
//! # Gate
//!
//! [`VoiceGate`] decides which input is appended to the session (with `pause_when_silent`):
//! - while voice is detected, and for [`TRAILING`] after it (so the model closes the phrase:
//!   the guide asks for silence after speech), everything is appended;
//! - then appending stops, at a 200 ms chunk boundary (a shorter chunk would wait in the
//!   server's buffer). The input is kept in a [`PRE_ROLL`] ring instead;
//! - when voice starts again the ring is appended first, so the first syllable (and the frames
//!   the onset needed) is not cut, then everything again.
//!
//! OpenAI's client events reference says that "if a client stops sending audio and later
//! resumes, model time treats the resumed audio as contiguous with the previous audio", so the
//! session stays open (kept alive by the WebSocket pings, see `translator::session`) and the
//! gap simply does not exist for the model.
//!
//! # Push-to-talk
//!
//! With [`VoiceConfig::push_to_talk`] (outgoing only) the gate opens while the push-to-talk key
//! is held instead of while voice is detected ([`VoiceGate::set_key_held`]): everything else is
//! the same machinery. Pressing the key appends the [`PRE_ROLL`] first (people start speaking as
//! they press), then everything while it is held; releasing it appends [`TRAILING`] more (to a
//! chunk boundary) so the model finishes the phrase, then appending stops. Pausing is always on
//! in this mode, and auto-off counts from the last time the key was held. The detector still
//! runs, for the "speaking" indicator.
//!
//! The gate also measures how long nobody spoke, for **auto-off**: after the configured time it
//! reports it once (the pipeline then stops through the manager). The time is the audio's own
//! clock (samples processed), so it is exact and needs no timer; the captured audio keeps
//! flowing at real time even when nothing is appended or the session is reconnecting.

use std::collections::VecDeque;
use std::time::Duration;

use crate::translator::pcm::{CHUNK_SAMPLES, SAMPLE_RATE, samples_for_ms};

/// Samples per detector frame (10 ms).
pub const FRAME_SAMPLES: usize = samples_for_ms(10);

/// How far above the noise floor a frame must be to count as loud.
pub const FLOOR_MARGIN_DB: f32 = 9.0;

/// Frames quieter than this never count as loud (about a whisper at normal mic gain). Keeps a
/// silent room with a very low floor from turning breath noise into voice.
pub const MIN_SPEECH_DBFS: f32 = -50.0;

/// How far back the noise floor looks for the quietest frame.
pub const FLOOR_WINDOW: Duration = Duration::from_secs(3);

/// Frames per block of the floor's history (100 ms): the history keeps one minimum per block.
const FLOOR_BLOCK_FRAMES: usize = 10;

/// Loud frames needed within [`ONSET_WINDOW`] for voice to start.
pub const ONSET_FRAMES: usize = 3;

/// Frames looked at for the onset.
pub const ONSET_WINDOW: usize = 5;

/// Voice continues this long after the last loud frame.
pub const HANGOVER: Duration = Duration::from_millis(300);

/// Audio appended after the last loud frame before appending pauses: the model needs some
/// silence after a phrase to finish translating it.
pub const TRAILING: Duration = Duration::from_millis(1200);

/// Audio kept while paused and appended first when voice starts again.
pub const PRE_ROLL: Duration = Duration::from_millis(300);

/// Level used for digital silence (log of zero).
const SILENCE_DBFS: f32 = -100.0;

/// Level of a frame of PCM16 samples, in dBFS.
pub fn frame_dbfs(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return SILENCE_DBFS;
    }
    let sum: f64 = samples
        .iter()
        .map(|&sample| {
            let value = f64::from(sample) / 32_768.0;
            value * value
        })
        .sum();
    let rms = (sum / samples.len() as f64).sqrt();
    if rms <= 0.0 {
        SILENCE_DBFS
    } else {
        (20.0 * rms.log10()).max(f64::from(SILENCE_DBFS)) as f32
    }
}

/// Finished blocks the floor remembers.
fn floor_blocks() -> usize {
    (FLOOR_WINDOW.as_millis() / (FLOOR_BLOCK_FRAMES as u128 * 10)) as usize
}

fn samples_of(duration: Duration) -> u64 {
    (duration.as_secs_f64() * f64::from(SAMPLE_RATE)).round() as u64
}

/// Energy detector with an adaptive noise floor. See the [module docs](self).
#[derive(Debug, Clone)]
pub struct VoiceDetector {
    /// The quietest frame of each finished block, oldest first ([`FLOOR_WINDOW`] of them).
    block_minima: VecDeque<f32>,
    /// The quietest frame of the block in progress, and its frames so far.
    block_min: f32,
    block_frames: usize,
    /// Loudness of the last [`ONSET_WINDOW`] frames.
    recent: VecDeque<bool>,
    active: bool,
    /// Samples since the last loud frame while active.
    since_loud: u64,
    hangover: u64,
}

impl Default for VoiceDetector {
    fn default() -> Self {
        Self {
            block_minima: VecDeque::with_capacity(floor_blocks() + 1),
            block_min: f32::INFINITY,
            block_frames: 0,
            recent: VecDeque::with_capacity(ONSET_WINDOW + 1),
            active: false,
            since_loud: 0,
            hangover: samples_of(HANGOVER),
        }
    }
}

impl VoiceDetector {
    /// The current noise floor, in dBFS (`None` before the first frame).
    pub fn floor_dbfs(&self) -> Option<f32> {
        let floor = self
            .block_minima
            .iter()
            .copied()
            .fold(self.block_min, f32::min);
        floor.is_finite().then_some(floor)
    }

    /// Whether voice is detected (onset reached, hangover not over).
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Feeds one frame (normally [`FRAME_SAMPLES`] long). Returns whether voice is detected
    /// after it.
    pub fn process(&mut self, frame: &[i16]) -> bool {
        let level = frame_dbfs(frame);
        let floor = self.floor_dbfs().unwrap_or(level).min(level);
        let loud = level >= MIN_SPEECH_DBFS && level >= floor + FLOOR_MARGIN_DB;

        self.block_min = self.block_min.min(level);
        self.block_frames += 1;
        if self.block_frames == FLOOR_BLOCK_FRAMES {
            self.block_minima.push_back(self.block_min);
            while self.block_minima.len() > floor_blocks() {
                self.block_minima.pop_front();
            }
            self.block_min = f32::INFINITY;
            self.block_frames = 0;
        }

        self.recent.push_back(loud);
        while self.recent.len() > ONSET_WINDOW {
            self.recent.pop_front();
        }
        if loud && self.active {
            self.since_loud = 0;
        } else if self.active {
            self.since_loud += frame.len() as u64;
            if self.since_loud > self.hangover {
                self.active = false;
            }
        } else if self.recent.iter().filter(|&&loud| loud).count() >= ONSET_FRAMES {
            self.active = true;
            self.since_loud = 0;
        }
        self.active
    }
}

/// How the gate behaves (from the settings).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceConfig {
    /// Stop appending while nobody speaks (see the [module docs](self)). Off = append
    /// everything, as before (the detector still runs, for the indicator and auto-off).
    pub pause_when_silent: bool,
    /// Report after this long without voice; `None` = never.
    pub auto_off: Option<Duration>,
    /// Open the gate while the push-to-talk key is held instead of on voice (see the
    /// [module docs](self)). `pause_when_silent` is ignored: nothing is appended otherwise.
    pub push_to_talk: bool,
}

impl VoiceConfig {
    /// The gate's configuration from the user's settings (voice detection; the outgoing
    /// direction sets [`push_to_talk`](Self::push_to_talk) from its talk mode).
    pub fn from_settings(settings: &crate::settings::VoiceActivitySettings) -> Self {
        Self {
            pause_when_silent: settings.pause_when_silent,
            auto_off: settings
                .auto_off_minutes
                .map(|minutes| Duration::from_secs(u64::from(minutes) * 60)),
            push_to_talk: false,
        }
    }

    /// Whether the gate starts (and stays) appending everything.
    fn appends_everything(self) -> bool {
        !self.pause_when_silent && !self.push_to_talk
    }
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            pause_when_silent: true,
            auto_off: None,
            push_to_talk: false,
        }
    }
}

/// The push-to-talk key's state, shared by the hotkey handler (which presses and releases it)
/// and the outgoing pipeline's pump (which reads it every tick). Cheap to clone.
#[derive(Debug, Clone, Default)]
pub struct TalkKey(std::sync::Arc<TalkKeyState>);

#[derive(Debug, Default)]
pub struct TalkKeyState {
    held: std::sync::atomic::AtomicBool,
    /// A press not seen by the pump yet: a tap shorter than a tick still opens the gate once.
    pressed: std::sync::atomic::AtomicBool,
}

impl TalkKey {
    /// The key went down (`true`) or up (`false`).
    pub fn set_held(&self, held: bool) {
        use std::sync::atomic::Ordering;
        if held {
            self.0.pressed.store(true, Ordering::SeqCst);
        }
        self.0.held.store(held, Ordering::SeqCst);
    }

    /// Whether the key is down now.
    pub fn is_held(&self) -> bool {
        self.0.held.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Forgets a press the pump has not seen (one from before the pipeline started).
    pub fn clear_press(&self) {
        self.0
            .pressed
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// Whether the key is down, or was pressed since the last call (for the pump).
    pub fn take_held(&self) -> bool {
        use std::sync::atomic::Ordering;
        let pressed = self.0.pressed.swap(false, Ordering::SeqCst);
        self.0.held.load(Ordering::SeqCst) || pressed
    }
}

impl PartialEq for TalkKey {
    /// The same key (not the same state).
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for TalkKey {}

/// What the panel's indicator shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceActivity {
    /// Voice is detected (with hangover).
    pub speaking: bool,
    /// Audio is being appended to the session (voice, its trailing silence, or everything when
    /// pausing is off).
    pub sending: bool,
}

/// What one [`VoiceGate::process`] call noticed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GateReport {
    /// The indicator changed (the new value).
    pub activity: Option<VoiceActivity>,
    /// Nobody spoke for the auto-off time (reported once).
    pub auto_off: bool,
}

/// Decides what is appended to the session. See the [module docs](self).
#[derive(Debug, Clone)]
pub struct VoiceGate {
    config: VoiceConfig,
    detector: VoiceDetector,
    /// Samples waiting for a full frame.
    pending: Vec<i16>,
    /// Input kept while paused (at most [`PRE_ROLL`]).
    pre_roll: VecDeque<i16>,
    pre_roll_samples: usize,
    sending: bool,
    /// Samples appended so far (for the chunk boundary).
    appended: u64,
    /// Samples since the gate's source (voice, or the push-to-talk key) was last on, or the
    /// start.
    since_voice: u64,
    /// The push-to-talk key is held (only with [`VoiceConfig::push_to_talk`]).
    key_held: bool,
    trailing: u64,
    auto_off: Option<u64>,
    auto_off_reported: bool,
    activity: VoiceActivity,
}

impl VoiceGate {
    /// A gate that starts paused (nothing is appended until someone speaks).
    pub fn new(config: VoiceConfig) -> Self {
        let pre_roll_samples = samples_of(PRE_ROLL) as usize;
        Self {
            config,
            detector: VoiceDetector::default(),
            pending: Vec::with_capacity(FRAME_SAMPLES),
            pre_roll: VecDeque::with_capacity(pre_roll_samples + FRAME_SAMPLES),
            pre_roll_samples,
            sending: config.appends_everything(),
            appended: 0,
            since_voice: 0,
            key_held: false,
            trailing: samples_of(TRAILING),
            auto_off: config.auto_off.map(samples_of),
            auto_off_reported: false,
            activity: VoiceActivity {
                speaking: false,
                sending: config.appends_everything(),
            },
        }
    }

    /// What the indicator shows now.
    pub fn activity(&self) -> VoiceActivity {
        self.activity
    }

    /// Sets whether the push-to-talk key is held; applies from the next frame. Ignored without
    /// [`VoiceConfig::push_to_talk`].
    pub fn set_key_held(&mut self, held: bool) {
        self.key_held = held;
    }

    /// Whether appending is paused (nothing said for a while).
    pub fn is_paused(&self) -> bool {
        !self.sending
    }

    /// Runs `input` through the gate, adding what must be appended to `out`. Audio is
    /// processed in whole frames, so up to 10 ms waits for the next call.
    pub fn process(&mut self, input: &[i16], out: &mut Vec<i16>) -> GateReport {
        let mut report = GateReport::default();
        let mut rest = input;
        while !rest.is_empty() {
            let take = (FRAME_SAMPLES - self.pending.len()).min(rest.len());
            self.pending.extend_from_slice(&rest[..take]);
            rest = &rest[take..];
            if self.pending.len() == FRAME_SAMPLES {
                let frame = std::mem::take(&mut self.pending);
                self.frame(&frame, out, &mut report);
                self.pending = frame;
                self.pending.clear();
            }
        }
        let activity = VoiceActivity {
            speaking: self.detector.is_active(),
            sending: self.sending,
        };
        if activity != self.activity {
            self.activity = activity;
            report.activity = Some(activity);
        }
        report
    }

    fn frame(&mut self, frame: &[i16], out: &mut Vec<i16>, report: &mut GateReport) {
        let voice = self.detector.process(frame);
        let active = if self.config.push_to_talk {
            self.key_held
        } else {
            voice
        };
        if active {
            self.since_voice = 0;
        } else {
            self.since_voice += frame.len() as u64;
        }
        if let Some(limit) = self.auto_off
            && !self.auto_off_reported
            && self.since_voice >= limit
        {
            self.auto_off_reported = true;
            report.auto_off = true;
        }

        if self.config.appends_everything() {
            self.append(frame, out);
            return;
        }
        if self.sending {
            self.append(frame, out);
            // With voice, the hangover is part of the trailing time: `since_voice` starts when
            // it ends. A released key has none.
            let hangover = if self.config.push_to_talk {
                0
            } else {
                samples_of(HANGOVER)
            };
            let quiet = self.since_voice + hangover;
            if !active
                && quiet >= self.trailing
                && self.appended.is_multiple_of(CHUNK_SAMPLES as u64)
            {
                self.sending = false;
            }
        } else if active {
            let pre_roll: Vec<i16> = self.pre_roll.drain(..).collect();
            self.append(&pre_roll, out);
            self.append(frame, out);
            self.sending = true;
        } else {
            self.pre_roll.extend(frame);
            let excess = self.pre_roll.len().saturating_sub(self.pre_roll_samples);
            self.pre_roll.drain(..excess);
        }
    }

    fn append(&mut self, samples: &[i16], out: &mut Vec<i16>) {
        out.extend_from_slice(samples);
        self.appended += samples.len() as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = SAMPLE_RATE as f32;

    /// A deterministic pseudo-random generator (no dependency needed).
    struct Noise(u32);

    impl Noise {
        fn next(&mut self) -> f32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 17;
            self.0 ^= self.0 << 5;
            (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
        }
    }

    fn ms(ms: u32) -> usize {
        samples_for_ms(ms)
    }

    fn silence(ms_: u32) -> Vec<i16> {
        vec![0; ms(ms_)]
    }

    /// White noise at about `dbfs` RMS.
    fn noise(ms_: u32, dbfs: f32, seed: u32) -> Vec<i16> {
        let mut noise = Noise(seed.max(1));
        // Uniform noise in [-1, 1] has an RMS of 1/sqrt(3).
        let amplitude = 10_f32.powf(dbfs / 20.0) * 3_f32.sqrt();
        (0..ms(ms_))
            .map(|_| (noise.next() * amplitude * 32_767.0) as i16)
            .collect()
    }

    /// A voice-like burst: a 180 Hz tone with harmonics, modulated by syllables (4 per second,
    /// with short dips between them), at about `dbfs` RMS, over a noise bed.
    fn speech(ms_: u32, dbfs: f32) -> Vec<i16> {
        let amplitude = 10_f32.powf(dbfs / 20.0) * 2.0;
        let bed = noise(ms_, -60.0, 7);
        (0..ms(ms_))
            .map(|n| {
                let t = n as f32 / RATE;
                let syllable = (t * 4.0 * std::f32::consts::PI).sin().abs().max(0.15);
                let voice = (0..4)
                    .map(|h| {
                        let f = 180.0 * (h + 1) as f32;
                        (t * f * std::f32::consts::TAU).sin() / (h + 1) as f32
                    })
                    .sum::<f32>()
                    / 2.0;
                let value = amplitude * syllable * voice * 32_767.0 + f32::from(bed[n]);
                value.clamp(-32_768.0, 32_767.0) as i16
            })
            .collect()
    }

    fn concat(parts: &[Vec<i16>]) -> Vec<i16> {
        parts.concat()
    }

    /// Feeds `input` to `gate` in 10 ms blocks (like the pump), returning what was appended
    /// and every report.
    fn run(gate: &mut VoiceGate, input: &[i16]) -> (Vec<i16>, Vec<GateReport>) {
        let mut out = Vec::new();
        let mut reports = Vec::new();
        for block in input.chunks(240) {
            let report = gate.process(block, &mut out);
            if report != GateReport::default() {
                reports.push(report);
            }
        }
        (out, reports)
    }

    /// Whether the detector says "voice" after each 10 ms frame.
    fn detect(input: &[i16]) -> Vec<bool> {
        let mut detector = VoiceDetector::default();
        input
            .chunks(FRAME_SAMPLES)
            .map(|frame| detector.process(frame))
            .collect()
    }

    fn pause_on() -> VoiceConfig {
        VoiceConfig {
            pause_when_silent: true,
            auto_off: None,
            push_to_talk: false,
        }
    }

    #[test]
    fn frame_level_in_dbfs() {
        assert_eq!(frame_dbfs(&[0; 240]), SILENCE_DBFS);
        assert_eq!(frame_dbfs(&[]), SILENCE_DBFS);
        assert!((frame_dbfs(&[16_384; 240]) - -6.02).abs() < 0.05);
        let level = frame_dbfs(&noise(10, -30.0, 3));
        assert!((level - -30.0).abs() < 2.0, "{level}");
    }

    #[test]
    fn silence_is_never_voice() {
        assert!(detect(&silence(5000)).iter().all(|&voice| !voice));
    }

    #[test]
    fn steady_noise_is_never_voice() {
        // A fan or a game's ambience: loud, but constant.
        for dbfs in [-55.0, -40.0, -25.0] {
            let frames = detect(&noise(10_000, dbfs, 11));
            assert!(
                frames.iter().all(|&voice| !voice),
                "noise at {dbfs} dBFS detected as voice"
            );
        }
    }

    #[test]
    fn a_noise_that_starts_suddenly_is_learned() {
        // Silence, then a fan turns on: it counts as voice at first, but only until the floor
        // window has seen nothing else.
        let input = concat(&[silence(2000), noise(20_000, -30.0, 5)]);
        let frames = detect(&input);
        let last_voice = frames.iter().rposition(|&voice| voice).unwrap_or(0);
        assert!(
            last_voice * 10 <= 2000 + 3000 + 400,
            "still voice {} ms in",
            last_voice * 10
        );
        assert!(!frames.last().unwrap());
    }

    #[test]
    fn speech_over_noise_is_voice_quickly() {
        let input = concat(&[noise(3000, -50.0, 2), speech(2000, -25.0)]);
        let frames = detect(&input);
        // Voice within 50 ms of the burst, and through it (the syllable dips are bridged).
        let onset = frames.iter().position(|&voice| voice).unwrap();
        assert!((300..305).contains(&onset), "onset at {} ms", onset * 10);
        assert!(frames[onset..].iter().all(|&voice| voice));
    }

    #[test]
    fn quiet_speech_in_a_silent_room_is_voice() {
        let frames = detect(&concat(&[silence(1000), speech(1000, -42.0)]));
        assert!(frames[100..].iter().filter(|&&voice| voice).count() > 90);
    }

    #[test]
    fn a_click_is_not_voice() {
        let mut click = silence(1000);
        click.extend(noise(20, -10.0, 9));
        click.extend(silence(1000));
        assert!(detect(&click).iter().all(|&voice| !voice));
    }

    #[test]
    fn hangover_bridges_pauses_between_words_and_then_ends() {
        let input = concat(&[
            silence(500),
            speech(800, -25.0),
            silence(200),
            speech(800, -25.0),
            silence(1000),
        ]);
        let frames = detect(&input);
        let onset = frames.iter().position(|&voice| voice).unwrap();
        let end = frames.iter().rposition(|&voice| voice).unwrap();
        // One stretch of voice across the 200 ms pause...
        assert!(frames[onset..=end].iter().all(|&voice| voice));
        // ...ending about HANGOVER after the last loud frame (at 2300 ms).
        let end_ms = (end + 1) * 10;
        assert!(
            (2550..=2650).contains(&end_ms),
            "voice ended at {end_ms} ms"
        );
    }

    #[test]
    fn gate_appends_nothing_while_silent() {
        let mut gate = VoiceGate::new(pause_on());
        let (out, reports) = run(&mut gate, &concat(&[noise(5000, -45.0, 4), silence(3000)]));
        assert!(out.is_empty());
        assert!(reports.is_empty());
        assert!(gate.is_paused());
    }

    #[test]
    fn gate_appends_speech_with_pre_roll_and_trailing_silence() {
        let mut gate = VoiceGate::new(pause_on());
        let burst = speech(2000, -25.0);
        let input = concat(&[
            noise(3000, -55.0, 3),
            burst.clone(),
            noise(30_000, -55.0, 6),
        ]);
        let (out, reports) = run(&mut gate, &input);

        // One contiguous stretch of the input is appended, starting a full pre-roll before the
        // onset (a few frames into the burst): the first syllable is kept, and the noise just
        // before it too.
        let burst_start = ms(3000);
        let start = (0..=ms(400))
            .step_by(FRAME_SAMPLES)
            .map(|back| burst_start + ms(100) - back)
            .find(|&start| input[start..start + out.len()] == out[..])
            .expect("the appended audio is not a stretch of the input");
        assert!(
            (burst_start - ms(300)..=burst_start - ms(250)).contains(&start),
            "appending started {} ms before the burst",
            (burst_start - start) / 24
        );
        let burst_at = burst_start - start;
        // Then about TRAILING of what followed (to a 200 ms chunk boundary), and no more.
        let total = out.len();
        assert_eq!(total % CHUNK_SAMPLES, 0);
        let after = total - burst_at - burst.len();
        assert!(
            (ms(1200)..ms(1600)).contains(&after),
            "{} ms appended after the burst",
            after / 24
        );
        assert!(gate.is_paused());

        // Indicator: speaking + sending, then not speaking (still sending), then paused.
        let activity: Vec<VoiceActivity> = reports.iter().filter_map(|r| r.activity).collect();
        assert_eq!(
            activity,
            [
                VoiceActivity {
                    speaking: true,
                    sending: true
                },
                VoiceActivity {
                    speaking: false,
                    sending: true
                },
                VoiceActivity {
                    speaking: false,
                    sending: false
                },
            ]
        );
    }

    #[test]
    fn gate_resumes_on_each_utterance() {
        let mut gate = VoiceGate::new(pause_on());
        let input = concat(&[
            silence(1000),
            speech(1000, -25.0),
            silence(30_000),
            speech(1000, -25.0),
            silence(5000),
        ]);
        let (out, _) = run(&mut gate, &input);
        // Two utterances: each about pre-roll + 1 s + trailing, nowhere near the 38 s input.
        let seconds = out.len() as f64 / f64::from(SAMPLE_RATE);
        assert!((4.5..6.0).contains(&seconds), "{seconds} s appended");
    }

    #[test]
    fn without_pausing_everything_is_appended() {
        let mut gate = VoiceGate::new(VoiceConfig {
            pause_when_silent: false,
            auto_off: None,
            push_to_talk: false,
        });
        let input = concat(&[silence(2000), speech(1000, -25.0), silence(2000)]);
        let (out, _) = run(&mut gate, &input);
        assert_eq!(out, input);
        assert!(!gate.is_paused());
    }

    #[test]
    fn auto_off_reports_once_after_the_quiet_time() {
        let mut gate = VoiceGate::new(VoiceConfig {
            pause_when_silent: true,
            auto_off: Some(Duration::from_secs(60)),
            push_to_talk: false,
        });
        let mut quiet_ms = 0;
        let mut reported = Vec::new();
        // Talk for a second, then 2 minutes of silence, fed in 1 s blocks (audio clock).
        let (_, reports) = run(&mut gate, &speech(1000, -25.0));
        assert!(reports.iter().all(|report| !report.auto_off));
        for _ in 0..120 {
            let (_, reports) = run(&mut gate, &silence(1000));
            quiet_ms += 1000;
            if reports.iter().any(|report| report.auto_off) {
                reported.push(quiet_ms);
            }
        }
        // Once, 60 s after the voice ended (its hangover included).
        assert_eq!(reported, [61_000]);
    }

    #[test]
    fn voice_restarts_the_auto_off_time() {
        let mut gate = VoiceGate::new(VoiceConfig {
            pause_when_silent: false,
            auto_off: Some(Duration::from_secs(10)),
            push_to_talk: false,
        });
        let input = concat(&[
            silence(8000),
            speech(500, -25.0),
            silence(8000),
            speech(500, -25.0),
            silence(8000),
        ]);
        let (_, reports) = run(&mut gate, &input);
        assert!(reports.iter().all(|report| !report.auto_off));
        let (_, reports) = run(&mut gate, &silence(3000));
        assert!(reports.iter().any(|report| report.auto_off));
    }

    #[test]
    fn auto_off_counts_from_the_start() {
        let mut gate = VoiceGate::new(VoiceConfig {
            pause_when_silent: true,
            auto_off: Some(Duration::from_secs(5)),
            push_to_talk: false,
        });
        let (_, reports) = run(&mut gate, &silence(6000));
        assert_eq!(reports.iter().filter(|report| report.auto_off).count(), 1);
    }

    fn push_to_talk() -> VoiceConfig {
        VoiceConfig {
            pause_when_silent: false,
            auto_off: None,
            push_to_talk: true,
        }
    }

    /// Feeds `input` in 10 ms blocks with the key held over `held` (`[from, to)` in samples), like the
    /// pump does (the key is read before each block).
    fn run_with_key(
        gate: &mut VoiceGate,
        input: &[i16],
        held: &[(usize, usize)],
    ) -> (Vec<i16>, Vec<GateReport>) {
        let mut out = Vec::new();
        let mut reports = Vec::new();
        for (index, block) in input.chunks(240).enumerate() {
            let at = index * 240;
            gate.set_key_held(held.iter().any(|&(from, to)| (from..to).contains(&at)));
            let report = gate.process(block, &mut out);
            if report != GateReport::default() {
                reports.push(report);
            }
        }
        (out, reports)
    }

    #[test]
    fn push_to_talk_sends_nothing_while_the_key_is_up_even_with_speech() {
        // `pause_when_silent: false` is ignored: push-to-talk always pauses.
        let mut gate = VoiceGate::new(push_to_talk());
        assert!(gate.is_paused());
        let input = concat(&[silence(500), speech(3000, -25.0), silence(2000)]);
        let (out, reports) = run_with_key(&mut gate, &input, &[]);
        assert!(out.is_empty());
        // The indicator still says someone speaks, but nothing is sent.
        assert!(
            reports
                .iter()
                .filter_map(|report| report.activity)
                .all(|activity| !activity.sending)
        );
        assert!(
            reports
                .iter()
                .any(|report| report.activity.is_some_and(|activity| activity.speaking))
        );
    }

    #[test]
    fn push_to_talk_sends_pre_roll_held_audio_and_a_tail_after_release() {
        let mut gate = VoiceGate::new(push_to_talk());
        // Background noise throughout; the key is held from 2 s to 4 s.
        let input = noise(10_000, -55.0, 21);
        let press = ms(2000);
        let release = ms(4000);
        let (out, reports) = run_with_key(&mut gate, &input, &[(press, release)]);

        // One contiguous stretch of the input: the pre-roll before the press, everything while
        // held, then TRAILING after the release, to a 200 ms chunk boundary.
        let start = press - ms(300);
        assert_eq!(out[..], input[start..start + out.len()]);
        assert_eq!(out.len() % CHUNK_SAMPLES, 0);
        let after = start + out.len() - release;
        assert!(
            (ms(1200)..ms(1400)).contains(&after),
            "{} ms sent after the release",
            after / 24
        );
        assert!(gate.is_paused());

        let sending: Vec<bool> = reports
            .iter()
            .filter_map(|report| report.activity)
            .map(|activity| activity.sending)
            .collect();
        assert_eq!(sending, [true, false]);
    }

    #[test]
    fn pressing_again_during_the_tail_keeps_one_stretch() {
        let mut gate = VoiceGate::new(push_to_talk());
        let input = noise(8000, -55.0, 22);
        let held = [(ms(1000), ms(2000)), (ms(2500), ms(3500))];
        let (out, _) = run_with_key(&mut gate, &input, &held);

        let start = ms(1000) - ms(300);
        assert_eq!(out[..], input[start..start + out.len()]);
        let after = start + out.len() - ms(3500);
        assert!((ms(1200)..ms(1400)).contains(&after), "{} ms", after / 24);
    }

    #[test]
    fn two_presses_far_apart_send_two_stretches() {
        let mut gate = VoiceGate::new(push_to_talk());
        let input = noise(20_000, -55.0, 23);
        let held = [(ms(1000), ms(2000)), (ms(10_000), ms(11_000))];
        let (out, _) = run_with_key(&mut gate, &input, &held);
        // Each: 300 ms pre-roll + 1 s held + ~1.2–1.4 s tail.
        let seconds = out.len() as f64 / f64::from(SAMPLE_RATE);
        assert!((5.0..5.5).contains(&seconds), "{seconds} s sent");
    }

    #[test]
    fn push_to_talk_auto_off_counts_from_the_last_press() {
        let mut gate = VoiceGate::new(VoiceConfig {
            auto_off: Some(Duration::from_secs(5)),
            ..push_to_talk()
        });
        // Talking without the key does not count as activity.
        let input = concat(&[speech(4000, -25.0), silence(2000)]);
        let (_, reports) = run_with_key(&mut gate, &input, &[(ms(3000), ms(3500))]);
        assert!(reports.iter().all(|report| !report.auto_off));
        // 5 s after the release (at 3.5 s).
        let (_, reports) = run_with_key(&mut gate, &silence(2400), &[]);
        assert!(reports.iter().all(|report| !report.auto_off));
        let (_, reports) = run_with_key(&mut gate, &silence(200), &[]);
        assert!(reports.iter().any(|report| report.auto_off));
    }

    #[test]
    fn a_tap_between_two_reads_still_opens_the_gate_once() {
        let key = TalkKey::default();
        assert!(!key.take_held());
        key.set_held(true);
        key.set_held(false);
        assert!(key.take_held(), "the tap is seen once");
        assert!(!key.take_held());
        key.set_held(true);
        assert!(key.take_held());
        assert!(key.take_held(), "still held");
        assert!(key.is_held());
        key.set_held(false);
        assert!(!key.take_held(), "released, and the press was already seen");
        key.set_held(true);
        key.set_held(false);
        key.clear_press();
        assert!(!key.take_held(), "an old tap is forgotten");
        assert_eq!(key.clone(), key);
        assert_ne!(key, TalkKey::default());
    }
}
