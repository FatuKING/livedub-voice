//! Opt-in session recorder for diagnosing audio quality. **Local only, off by default.**
//!
//! When the `LIVEDUB_RECORD_DIR` environment variable names a directory, every outgoing session
//! writes a folder `outgoing-<unix seconds>` there with:
//!
//! - `sent.wav`: the PCM16 mono 24 kHz audio actually sent to the translator, silence included;
//! - `received.wav`: the translated audio as received, placed on a wall-clock timeline: a gap
//!   between deltas becomes silence, so timing problems are audible (deltas that arrive faster
//!   than real time are appended back to back);
//! - `cable.wav`: what was written to the virtual cable, in the cable's rate and channels;
//! - `events.jsonl`: one JSON object per line with `t_ms` (since start) and `kind`: each 200 ms
//!   block sent (with its RMS/peak), each output delta (sample count, buffered ms), transcript
//!   deltas (source and translated text), speech start/end, latency measurements, jitter stats
//!   every second, the stop sequence and a final summary.
//!
//! Nothing is uploaded anywhere; the files stay where the user pointed. They contain the user's
//! voice and the transcript, so the variable must never be set by default.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::translator::pcm::{CHUNK_SAMPLES, SAMPLE_RATE};

/// Environment variable naming the directory sessions are recorded to.
pub const RECORD_DIR_ENV: &str = "LIVEDUB_RECORD_DIR";

/// A PCM16 WAV file whose sizes are patched in when it is finished.
pub struct WavWriter {
    file: BufWriter<File>,
    channels: u16,
    data_bytes: u64,
}

impl WavWriter {
    /// Creates `path` for PCM16 audio at `sample_rate` Hz with `channels` interleaved channels.
    pub fn create(path: &Path, sample_rate: u32, channels: u16) -> std::io::Result<Self> {
        let mut file = BufWriter::with_capacity(64 * 1024, File::create(path)?);
        let channels = channels.max(1);
        let block_align = channels * 2;
        file.write_all(b"RIFF")?;
        file.write_all(&0u32.to_le_bytes())?;
        file.write_all(b"WAVEfmt ")?;
        file.write_all(&16u32.to_le_bytes())?;
        file.write_all(&1u16.to_le_bytes())?; // PCM
        file.write_all(&channels.to_le_bytes())?;
        file.write_all(&sample_rate.to_le_bytes())?;
        file.write_all(&(sample_rate * u32::from(block_align)).to_le_bytes())?;
        file.write_all(&block_align.to_le_bytes())?;
        file.write_all(&16u16.to_le_bytes())?;
        file.write_all(b"data")?;
        file.write_all(&0u32.to_le_bytes())?;
        Ok(Self {
            file,
            channels,
            data_bytes: 0,
        })
    }

    /// Appends interleaved PCM16 samples.
    pub fn write(&mut self, samples: &[i16]) -> std::io::Result<()> {
        for sample in samples {
            self.file.write_all(&sample.to_le_bytes())?;
        }
        self.data_bytes += samples.len() as u64 * 2;
        Ok(())
    }

    /// Appends interleaved `f32` samples (clipped to PCM16).
    pub fn write_f32(&mut self, samples: &[f32]) -> std::io::Result<()> {
        for sample in samples {
            let value = (sample.clamp(-1.0, 1.0) * 32_767.0).round() as i16;
            self.file.write_all(&value.to_le_bytes())?;
        }
        self.data_bytes += samples.len() as u64 * 2;
        Ok(())
    }

    /// Appends `frames` frames of silence.
    pub fn write_silence(&mut self, frames: u64) -> std::io::Result<()> {
        let zeros = [0u8; 4096];
        let mut left = frames * u64::from(self.channels) * 2;
        self.data_bytes += left;
        while left > 0 {
            let take = left.min(zeros.len() as u64) as usize;
            self.file.write_all(&zeros[..take])?;
            left -= take as u64;
        }
        Ok(())
    }

    /// Frames written so far.
    pub fn frames(&self) -> u64 {
        self.data_bytes / (u64::from(self.channels) * 2)
    }

    /// Writes the final sizes into the header.
    pub fn finish(mut self) -> std::io::Result<()> {
        let data = u32::try_from(self.data_bytes).unwrap_or(u32::MAX);
        self.file.flush()?;
        let file = self.file.get_mut();
        file.seek(SeekFrom::Start(4))?;
        file.write_all(&data.saturating_add(36).to_le_bytes())?;
        file.seek(SeekFrom::Start(40))?;
        file.write_all(&data.to_le_bytes())?;
        file.flush()
    }
}

/// See the [module docs](self).
pub struct SessionRecorder {
    dir: PathBuf,
    sent: WavWriter,
    received: WavWriter,
    cable: WavWriter,
    events: BufWriter<File>,
    /// Sent samples not yet summarized in an event (one event per 200 ms block).
    block_samples: usize,
    block_sum_squares: f64,
    block_peak: i16,
    failed: bool,
}

impl SessionRecorder {
    /// A recorder when [`RECORD_DIR_ENV`] is set, writing `<dir>/<direction>-<unix seconds>`.
    /// Logs and returns `None` if the folder cannot be created.
    pub fn from_env(direction: &str, cable_rate: u32, cable_channels: u16) -> Option<Self> {
        let root = std::env::var_os(RECORD_DIR_ENV)?;
        if root.is_empty() {
            return None;
        }
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        let dir = PathBuf::from(root).join(format!("{direction}-{stamp}"));
        match Self::create(&dir, cable_rate, cable_channels) {
            Ok(recorder) => {
                tracing::info!(dir = %dir.display(), "recording this session for diagnostics");
                Some(recorder)
            }
            Err(error) => {
                tracing::warn!(%error, dir = %dir.display(), "could not start the session recorder");
                None
            }
        }
    }

    /// A recorder writing into `dir` (created if needed).
    pub fn create(dir: &Path, cable_rate: u32, cable_channels: u16) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        Ok(Self {
            dir: dir.to_owned(),
            sent: WavWriter::create(&dir.join("sent.wav"), SAMPLE_RATE, 1)?,
            received: WavWriter::create(&dir.join("received.wav"), SAMPLE_RATE, 1)?,
            cable: WavWriter::create(&dir.join("cable.wav"), cable_rate, cable_channels)?,
            events: BufWriter::new(File::create(dir.join("events.jsonl"))?),
            block_samples: 0,
            block_sum_squares: 0.0,
            block_peak: 0,
            failed: false,
        })
    }

    /// Where the files go.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn check(&mut self, result: std::io::Result<()>) {
        if let Err(error) = result
            && !self.failed
        {
            self.failed = true;
            tracing::warn!(%error, "the session recorder stopped writing");
        }
    }

    /// Audio sent to the translator at `at` (since start).
    pub fn sent(&mut self, samples: &[i16], at: Duration) {
        if self.failed {
            return;
        }
        let result = self.sent.write(samples);
        self.check(result);
        for &sample in samples {
            let value = f64::from(sample) / 32_768.0;
            self.block_sum_squares += value * value;
            self.block_peak = self.block_peak.max(sample.saturating_abs());
            self.block_samples += 1;
            if self.block_samples == CHUNK_SAMPLES {
                let rms = (self.block_sum_squares / CHUNK_SAMPLES as f64).sqrt();
                let peak = f64::from(self.block_peak) / 32_768.0;
                self.event(
                    at,
                    json!({
                        "kind": "sent",
                        "samples": CHUNK_SAMPLES,
                        "total_samples": self.sent.frames(),
                        "rms": round3(rms),
                        "peak": round3(peak),
                    }),
                );
                self.block_samples = 0;
                self.block_sum_squares = 0.0;
                self.block_peak = 0;
            }
        }
    }

    /// A translated audio delta received at `at`, with what the jitter buffer held after it.
    pub fn received(&mut self, samples: &[i16], at: Duration, buffered_ms: u64) {
        if self.failed {
            return;
        }
        let due = (at.as_secs_f64() * f64::from(SAMPLE_RATE)) as u64;
        let gap = due.saturating_sub(self.received.frames());
        let mut result = self.received.write_silence(gap);
        if result.is_ok() {
            result = self.received.write(samples);
        }
        self.check(result);
        self.event(
            at,
            json!({
                "kind": "output_delta",
                "samples": samples.len(),
                "ms": samples.len() as u64 * 1000 / u64::from(SAMPLE_RATE),
                "buffered_ms": buffered_ms,
            }),
        );
    }

    /// Interleaved samples written to the cable.
    pub fn cable(&mut self, interleaved: &[f32]) {
        if self.failed {
            return;
        }
        let result = self.cable.write_f32(interleaved);
        self.check(result);
    }

    /// Appends an event line; `fields` must be a JSON object (`t_ms` is added).
    pub fn event(&mut self, at: Duration, mut fields: Value) {
        if self.failed {
            return;
        }
        if let Some(object) = fields.as_object_mut() {
            object.insert("t_ms".into(), json!(at.as_millis() as u64));
        }
        let result = serde_json::to_writer(&mut self.events, &fields)
            .map_err(std::io::Error::other)
            .and_then(|()| self.events.write_all(b"\n"));
        self.check(result);
    }

    /// Finishes every file. Returns the folder.
    pub fn finish(mut self) -> PathBuf {
        let result = self.events.flush();
        self.check(result);
        for wav in [self.sent, self.received, self.cable] {
            if let Err(error) = wav.finish() {
                tracing::warn!(%error, "could not finish a recorded WAV file");
            }
        }
        tracing::info!(dir = %self.dir.display(), "session recording saved");
        self.dir
    }
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// Health of the microphone signal sent to the translator: level, clipping and how the device's
/// channels were downmixed.
#[derive(Debug, Clone, Default)]
pub struct InputHealth {
    samples: u64,
    sum_squares: f64,
    peak: i16,
    clipped: u64,
    speech_samples: u64,
    speech_sum_squares: f64,
    /// Per device channel: sum of squares of the raw capture (before downmix).
    channel_sum_squares: Vec<f64>,
    channel_frames: u64,
    /// Sum of squares of the downmixed (mono) raw capture.
    mono_sum_squares: f64,
}

/// What [`InputHealth`] concluded.
#[derive(Debug, Clone, PartialEq)]
pub struct InputSummary {
    pub peak_dbfs: f64,
    pub rms_dbfs: f64,
    pub speech_rms_dbfs: f64,
    pub clipped_samples: u64,
    pub channel_rms_dbfs: Vec<f64>,
    /// Problems worth a warning, as short sentences.
    pub warnings: Vec<&'static str>,
}

fn dbfs(rms: f64) -> f64 {
    if rms <= 1e-9 {
        -120.0
    } else {
        (20.0 * rms.log10() * 10.0).round() / 10.0
    }
}

impl InputHealth {
    /// PCM16 mono 24 kHz samples sent; `speech` marks a block the level detector took as speech.
    pub fn sent(&mut self, samples: &[i16], speech: bool) {
        for &sample in samples {
            let value = f64::from(sample) / 32_768.0;
            self.sum_squares += value * value;
            if speech {
                self.speech_sum_squares += value * value;
            }
            self.peak = self.peak.max(sample.saturating_abs());
            if sample == i16::MAX || sample <= -i16::MAX {
                self.clipped += 1;
            }
        }
        self.samples += samples.len() as u64;
        if speech {
            self.speech_samples += samples.len() as u64;
        }
    }

    /// Raw capture frames (interleaved `f32`, `channels` per frame), before the downmix.
    pub fn captured(&mut self, interleaved: &[f32], channels: usize) {
        if channels == 0 {
            return;
        }
        self.channel_sum_squares.resize(channels, 0.0);
        let scale = 1.0 / channels as f64;
        for frame in interleaved.chunks_exact(channels) {
            let mut mono = 0.0;
            for (channel, &sample) in frame.iter().enumerate() {
                let sample = f64::from(sample);
                self.channel_sum_squares[channel] += sample * sample;
                mono += sample;
            }
            mono *= scale;
            self.mono_sum_squares += mono * mono;
            self.channel_frames += 1;
        }
    }

    /// The summary, with warnings for clipping, a too quiet signal, a dead channel (the downmix
    /// then halves the level) or channels that cancel out.
    pub fn summary(&self) -> InputSummary {
        let rms = |sum: f64, count: u64| {
            if count == 0 {
                0.0
            } else {
                (sum / count as f64).sqrt()
            }
        };
        let speech_rms = rms(self.speech_sum_squares, self.speech_samples);
        let channel_rms: Vec<f64> = self
            .channel_sum_squares
            .iter()
            .map(|&sum| rms(sum, self.channel_frames))
            .collect();
        let mut warnings = Vec::new();
        // More than 0.1 % of the samples at full scale.
        if self.samples > 0 && self.clipped * 1000 > self.samples {
            warnings.push("the microphone clips: lower its level in Windows");
        }
        if self.speech_samples > 0 && speech_rms < 0.03 {
            warnings.push("speech is quiet (below -30 dBFS): raise the microphone level");
        }
        if channel_rms.len() > 1 {
            let loudest = channel_rms.iter().copied().fold(0.0, f64::max);
            let quietest = channel_rms.iter().copied().fold(f64::MAX, f64::min);
            if loudest > 0.01 && quietest < loudest * 0.1 {
                warnings.push("one microphone channel is silent: the downmix lowers the level");
            }
            let mean_power =
                channel_rms.iter().map(|r| r * r).sum::<f64>() / channel_rms.len() as f64;
            let mono = rms(self.mono_sum_squares, self.channel_frames);
            if mean_power.sqrt() > 0.01 && mono < mean_power.sqrt() * 0.3 {
                warnings.push("the microphone channels cancel out when downmixed (phase inverted)");
            }
        }
        InputSummary {
            peak_dbfs: dbfs(f64::from(self.peak) / 32_768.0),
            rms_dbfs: dbfs(rms(self.sum_squares, self.samples)),
            speech_rms_dbfs: dbfs(speech_rms),
            clipped_samples: self.clipped,
            channel_rms_dbfs: channel_rms.into_iter().map(dbfs).collect(),
            warnings,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_wav(path: &Path) -> (hound::WavSpec, Vec<i16>) {
        let mut reader = hound::WavReader::open(path).unwrap();
        let spec = reader.spec();
        let samples = reader.samples::<i16>().map(Result::unwrap).collect();
        (spec, samples)
    }

    #[test]
    fn writes_valid_wavs_and_events() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path().join("session");
        let mut recorder = SessionRecorder::create(&session, 48_000, 2).unwrap();

        recorder.sent(&vec![1000; CHUNK_SAMPLES + 100], Duration::from_millis(200));
        // A delta at 0.5 s, then one at 1.0 s: the gap between them becomes silence.
        recorder.received(&[7; 2400], Duration::from_millis(500), 100);
        recorder.received(&[8; 2400], Duration::from_millis(1000), 0);
        recorder.cable(&[0.5, -0.5, 0.25, -0.25]);
        recorder.event(Duration::from_secs(2), json!({ "kind": "summary" }));
        assert_eq!(recorder.finish(), session);

        let (spec, sent) = read_wav(&session.join("sent.wav"));
        assert_eq!(
            (spec.sample_rate, spec.channels, spec.bits_per_sample),
            (24_000, 1, 16)
        );
        assert_eq!(sent.len(), CHUNK_SAMPLES + 100);

        let (_, received) = read_wav(&session.join("received.wav"));
        assert_eq!(received.len(), 24_000 + 2400);
        assert!(received[..12_000].iter().all(|&s| s == 0));
        assert!(received[12_000..14_400].iter().all(|&s| s == 7));
        assert!(received[14_400..24_000].iter().all(|&s| s == 0));
        assert!(received[24_000..].iter().all(|&s| s == 8));

        let (spec, cable) = read_wav(&session.join("cable.wav"));
        assert_eq!((spec.sample_rate, spec.channels), (48_000, 2));
        assert_eq!(cable, vec![16_384, -16_384, 8_192, -8_192]);

        let events = std::fs::read_to_string(session.join("events.jsonl")).unwrap();
        let events: Vec<Value> = events
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let kinds: Vec<_> = events.iter().map(|e| e["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["sent", "output_delta", "output_delta", "summary"]);
        assert_eq!(events[0]["rms"], json!(0.031));
        assert_eq!(events[1]["t_ms"], json!(500));
        assert_eq!(events[1]["samples"], json!(2400));
    }

    #[test]
    fn deltas_faster_than_real_time_are_appended() {
        let dir = tempfile::tempdir().unwrap();
        let mut recorder = SessionRecorder::create(dir.path(), 24_000, 1).unwrap();
        recorder.received(&[1; 24_000], Duration::from_millis(100), 0);
        recorder.received(&[2; 2400], Duration::from_millis(200), 0);
        let session = recorder.finish();
        let (_, received) = read_wav(&session.join("received.wav"));
        assert_eq!(received.len(), 2400 + 24_000 + 2400);
    }

    #[test]
    fn input_health_flags_clipping_quiet_speech_and_dead_channels() {
        let mut health = InputHealth::default();
        health.sent(&[i16::MAX; 100], true);
        health.sent(&[0; 100], false);
        // Left channel loud, right silent.
        let frames: Vec<f32> = (0..1000).flat_map(|_| [0.5, 0.0]).collect();
        health.captured(&frames, 2);
        let summary = health.summary();
        assert_eq!(summary.clipped_samples, 100);
        assert!(summary.peak_dbfs > -0.1);
        assert!(summary.warnings.iter().any(|w| w.contains("clips")));
        assert!(summary.warnings.iter().any(|w| w.contains("silent")));
        assert_eq!(summary.channel_rms_dbfs.len(), 2);

        let mut inverted = InputHealth::default();
        let frames: Vec<f32> = (0..1000).flat_map(|_| [0.5, -0.5]).collect();
        inverted.captured(&frames, 2);
        inverted.sent(&[200; 100], true);
        let summary = inverted.summary();
        assert!(summary.warnings.iter().any(|w| w.contains("cancel")));
        assert!(summary.warnings.iter().any(|w| w.contains("quiet")));

        let mut healthy = InputHealth::default();
        healthy.sent(&[6000; 1000], true);
        let frames: Vec<f32> = (0..1000).flat_map(|_| [0.2, 0.2]).collect();
        healthy.captured(&frames, 2);
        assert!(
            healthy.summary().warnings.is_empty(),
            "{:?}",
            healthy.summary()
        );
    }
}
