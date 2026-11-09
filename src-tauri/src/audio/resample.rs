//! Streaming format conversion between audio devices and the translator.
//!
//! Devices deliver and expect `f32` interleaved audio at their own rate and channel count (for
//! example 48 kHz stereo); the translator works with PCM16 mono 24 kHz. Two converters bridge
//! them:
//!
//! - [`ToTranslator`]: device format → PCM16 mono 24 kHz (downmix, resample, quantize).
//! - [`FromTranslator`]: PCM16 mono 24 kHz → device format (resample, upmix).
//!
//! Both accept chunks of any size and buffer internally, so they can be fed straight from capture
//! callbacks or from translator deltas of irregular length. Resampling always happens on the
//! mono signal (one channel of work regardless of the device's channel count), with a short
//! windowed-sinc filter from [`rubato`] and 10 ms processing blocks, which adds a few
//! milliseconds of latency. Once the caller's output `Vec` has grown to its working size, no
//! allocation happens per chunk.
//!
//! # Clock drift
//!
//! The capture device, the playback device and the translator all run on different clocks, so
//! a buffer between them slowly fills up or drains even when the nominal rates match.
//! [`FromTranslator::set_rate_adjust`] (and the same method on [`ToTranslator`]) nudge the
//! effective resampling ratio by up to ±[`MAX_RATE_ADJUST`] (2 %, inaudible at the small values
//! a controller needs) with a smooth ramp. A playback controller can read the buffered amount
//! (see `translator::jitter::JitterBuffer::buffered_samples`) and, for example, call
//! `set_rate_adjust(1.0 + k * (target - level) / target)` to produce slightly more output when
//! the device buffer runs low and slightly less when it grows.

use rubato::audioadapter_buffers::direct::SequentialSlice;
use rubato::{
    Adjustable, Async, FixedAsync, Indexing, Resampler, SincInterpolationParameters,
    SincInterpolationType, WindowFunction,
};

use crate::translator::pcm::SAMPLE_RATE as TRANSLATOR_RATE;

/// Largest relative change [`FromTranslator::set_rate_adjust`] accepts in either direction.
pub const MAX_RATE_ADJUST: f64 = 0.02;

/// Processing block, in milliseconds of input audio.
const BLOCK_MS: usize = 10;

/// Windowed-sinc filter length. Short enough to keep the delay and CPU low (about 1.3 ms at
/// 48 kHz), long enough for speech.
const SINC_LEN: usize = 128;

/// Why a converter could not be created.
#[derive(Debug, thiserror::Error)]
pub enum ConvertError {
    #[error("unsupported audio format: {0}")]
    Format(String),
}

/// Mono `f32` resampler that takes input of any length.
struct MonoResampler {
    resampler: Async<f32>,
    /// Input waiting for a full block.
    pending: Vec<f32>,
    /// Output of one block.
    block_out: Vec<f32>,
    block_len: usize,
    base_ratio: f64,
}

impl MonoResampler {
    fn new(input_rate: u32, output_rate: u32) -> Result<Self, ConvertError> {
        if input_rate == 0 || output_rate == 0 {
            return Err(ConvertError::Format(
                "sample rate must be above zero".into(),
            ));
        }
        let ratio = f64::from(output_rate) / f64::from(input_rate);
        let block_len = (input_rate as usize * BLOCK_MS / 1000).max(1);
        let parameters =
            SincInterpolationParameters::new(SINC_LEN, WindowFunction::BlackmanHarris2)
                .oversampling_factor(128)
                .interpolation(SincInterpolationType::Linear);
        let resampler = Async::<f32>::new_sinc(
            ratio,
            1.0 + MAX_RATE_ADJUST * 2.0,
            &parameters,
            block_len,
            1,
            FixedAsync::Input,
        )
        .map_err(|error| ConvertError::Format(error.to_string()))?;
        let block_out = vec![0.0; resampler.output_frames_max()];
        Ok(Self {
            resampler,
            pending: Vec::with_capacity(block_len),
            block_out,
            block_len,
            base_ratio: ratio,
        })
    }

    /// Feeds samples and calls `emit` with every block of output produced.
    fn push(&mut self, input: impl IntoIterator<Item = f32>, mut emit: impl FnMut(&[f32])) {
        for sample in input {
            self.pending.push(sample);
            if self.pending.len() == self.block_len {
                self.run_block(None, &mut emit);
            }
        }
    }

    /// Pushes the buffered remainder (padded with silence) and the filter's tail.
    fn flush(&mut self, mut emit: impl FnMut(&[f32])) {
        let partial = self.pending.len();
        let tail = self.resampler.output_delay()
            + (partial as f64 * self.resampler.resample_ratio()).ceil() as usize;
        let mut produced = 0;
        self.run_block(Some(partial), &mut |out: &[f32]| {
            produced += out.len();
            emit(out);
        });
        // Push silence until the filter delay has come out too.
        let mut guard = 0;
        while produced < tail && guard < 64 {
            self.run_block(Some(0), &mut |out: &[f32]| {
                produced += out.len();
                emit(out);
            });
            guard += 1;
        }
    }

    fn run_block(&mut self, partial_len: Option<usize>, emit: &mut impl FnMut(&[f32])) {
        let valid = partial_len.unwrap_or(self.block_len);
        self.pending.resize(self.block_len, 0.0);
        let input = SequentialSlice::new(&self.pending[..], 1, self.block_len);
        let output_len = self.block_out.len();
        let output = SequentialSlice::new_mut(&mut self.block_out[..], 1, output_len);
        let indexing = partial_len.map(|len| Indexing::new().partial_len(len));
        let result = match (input, output) {
            (Ok(input), Ok(mut output)) => self
                .resampler
                .process_into_buffer(&input, &mut output, indexing.as_ref())
                .map_err(|error| error.to_string()),
            _ => Err("buffer size mismatch".to_owned()),
        };
        self.pending.clear();
        match result {
            Ok((_, written)) => emit(&self.block_out[..written]),
            Err(error) => tracing::warn!(%error, valid, "resampling block dropped"),
        }
    }

    fn set_rate_adjust(&mut self, factor: f64) -> f64 {
        let factor = factor.clamp(1.0 - MAX_RATE_ADJUST, 1.0 + MAX_RATE_ADJUST);
        if let Err(error) = self
            .resampler
            .set_resample_ratio(self.base_ratio * factor, true)
        {
            tracing::warn!(%error, factor, "could not adjust the resampling ratio");
        }
        factor
    }

    fn rate_adjust(&self) -> f64 {
        self.resampler.resample_ratio() / self.base_ratio
    }

    fn reset(&mut self) {
        self.resampler.reset();
        self.pending.clear();
    }
}

impl std::fmt::Debug for MonoResampler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MonoResampler")
            .field("base_ratio", &self.base_ratio)
            .field("block_len", &self.block_len)
            .finish_non_exhaustive()
    }
}

/// Converts a float sample (nominally -1.0..=1.0) to PCM16, clipping anything outside the range.
pub fn quantize(sample: f32) -> i16 {
    // NaN becomes 0 through the saturating cast.
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16
}

/// Converts a PCM16 sample to float in -1.0..=1.0.
pub fn dequantize(sample: i16) -> f32 {
    f32::from(sample) / f32::from(i16::MAX)
}

/// Device format (f32 interleaved, any rate and channel count) → PCM16 mono 24 kHz.
#[derive(Debug)]
pub struct ToTranslator {
    channels: usize,
    resampler: MonoResampler,
}

impl ToTranslator {
    /// A converter for audio captured at `device_rate` Hz with `channels` interleaved channels.
    pub fn new(device_rate: u32, channels: u16) -> Result<Self, ConvertError> {
        if channels == 0 {
            return Err(ConvertError::Format(
                "channel count must be above zero".into(),
            ));
        }
        Ok(Self {
            channels: usize::from(channels),
            resampler: MonoResampler::new(device_rate, TRANSLATOR_RATE)?,
        })
    }

    /// Converts interleaved samples (whole frames; a trailing partial frame is ignored) and
    /// appends the PCM16 mono 24 kHz result to `out`.
    pub fn process(&mut self, interleaved: &[f32], out: &mut Vec<i16>) {
        let channels = self.channels;
        debug_assert!(
            interleaved.len().is_multiple_of(channels),
            "input must hold whole frames"
        );
        let scale = 1.0 / channels as f32;
        let mono = interleaved
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() * scale);
        self.resampler.push(mono, |block| {
            out.extend(block.iter().copied().map(quantize))
        });
    }

    /// Ends the stream: converts what is still buffered (padded with silence).
    pub fn flush(&mut self, out: &mut Vec<i16>) {
        self.resampler
            .flush(|block| out.extend(block.iter().copied().map(quantize)));
    }

    /// Drift hook: scales the output rate by `factor` (clamped to 1 ± [`MAX_RATE_ADJUST`]),
    /// ramped over the next block. Returns the factor applied.
    pub fn set_rate_adjust(&mut self, factor: f64) -> f64 {
        self.resampler.set_rate_adjust(factor)
    }

    /// The rate factor currently applied (1.0 when not adjusted).
    pub fn rate_adjust(&self) -> f64 {
        self.resampler.rate_adjust()
    }

    /// Drops buffered audio and filter state (for example after a device change).
    pub fn reset(&mut self) {
        self.resampler.reset();
    }
}

/// PCM16 mono 24 kHz → device format (f32 interleaved, any rate and channel count).
#[derive(Debug)]
pub struct FromTranslator {
    channels: usize,
    resampler: MonoResampler,
}

impl FromTranslator {
    /// A converter for a device playing at `device_rate` Hz with `channels` interleaved channels.
    /// The mono signal is copied to every channel.
    pub fn new(device_rate: u32, channels: u16) -> Result<Self, ConvertError> {
        if channels == 0 {
            return Err(ConvertError::Format(
                "channel count must be above zero".into(),
            ));
        }
        Ok(Self {
            channels: usize::from(channels),
            resampler: MonoResampler::new(TRANSLATOR_RATE, device_rate)?,
        })
    }

    /// Converts PCM16 mono 24 kHz samples and appends interleaved device frames to `out`.
    pub fn process(&mut self, pcm: &[i16], out: &mut Vec<f32>) {
        let channels = self.channels;
        self.resampler
            .push(pcm.iter().copied().map(dequantize), |block| {
                upmix(block, channels, out);
            });
    }

    /// Ends the stream: converts what is still buffered (padded with silence).
    pub fn flush(&mut self, out: &mut Vec<f32>) {
        let channels = self.channels;
        self.resampler.flush(|block| upmix(block, channels, out));
    }

    /// Drift hook: scales the output rate by `factor` (clamped to 1 ± [`MAX_RATE_ADJUST`]),
    /// ramped over the next block. Above 1.0 produces more device frames per translator sample
    /// (use it when the playback buffer runs low), below 1.0 fewer. Returns the factor applied.
    pub fn set_rate_adjust(&mut self, factor: f64) -> f64 {
        self.resampler.set_rate_adjust(factor)
    }

    /// The rate factor currently applied (1.0 when not adjusted).
    pub fn rate_adjust(&self) -> f64 {
        self.resampler.rate_adjust()
    }

    /// Drops buffered audio and filter state.
    pub fn reset(&mut self) {
        self.resampler.reset();
    }
}

fn upmix(mono: &[f32], channels: usize, out: &mut Vec<f32>) {
    out.reserve(mono.len() * channels);
    for &sample in mono {
        out.extend(std::iter::repeat_n(sample, channels));
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::*;

    fn sine(rate: u32, channels: usize, freq: f32, seconds: f32, amplitude: f32) -> Vec<f32> {
        let frames = (rate as f32 * seconds) as usize;
        (0..frames)
            .flat_map(|i| {
                let value = amplitude * (TAU * freq * i as f32 / rate as f32).sin();
                std::iter::repeat_n(value, channels)
            })
            .collect()
    }

    /// Frequency estimate from rising zero crossings.
    fn estimate_frequency(samples: &[f32], rate: u32) -> f32 {
        let crossings: Vec<usize> = samples
            .windows(2)
            .enumerate()
            .filter(|(_, pair)| pair[0] < 0.0 && pair[1] >= 0.0)
            .map(|(i, _)| i)
            .collect();
        let (Some(first), Some(last)) = (crossings.first(), crossings.last()) else {
            return 0.0;
        };
        let periods = (crossings.len() - 1) as f32;
        periods * rate as f32 / (last - first) as f32
    }

    fn to_f32(pcm: &[i16]) -> Vec<f32> {
        pcm.iter().copied().map(dequantize).collect()
    }

    #[test]
    fn downmixes_and_resamples_48k_stereo_keeping_the_frequency() {
        let input = sine(48_000, 2, 440.0, 1.0, 0.5);
        let mut converter = ToTranslator::new(48_000, 2).unwrap();
        let mut out = Vec::new();
        converter.process(&input, &mut out);

        let expected = 24_000;
        assert!(
            (out.len() as i64 - expected).abs() < 600,
            "got {} samples",
            out.len()
        );
        // Skip the filter's start-up.
        let samples = to_f32(&out[200..]);
        let freq = estimate_frequency(&samples, 24_000);
        assert!((freq - 440.0).abs() < 2.0, "estimated {freq} Hz");
        let peak = samples.iter().fold(0.0f32, |peak, s| peak.max(s.abs()));
        assert!((peak - 0.5).abs() < 0.05, "peak {peak}");
    }

    #[test]
    fn downmix_averages_channels() {
        // Left and right in opposite phase cancel out.
        let input: Vec<f32> = sine(48_000, 1, 300.0, 0.2, 0.5)
            .into_iter()
            .flat_map(|s| [s, -s])
            .collect();
        let mut converter = ToTranslator::new(48_000, 2).unwrap();
        let mut out = Vec::new();
        converter.process(&input, &mut out);
        assert!(!out.is_empty());
        assert!(out.iter().all(|&s| s.abs() <= 1), "{:?}", &out[..10]);
    }

    #[test]
    fn round_trip_keeps_the_length() {
        for (rate, channels) in [
            (48_000, 2),
            (44_100, 2),
            (44_100, 1),
            (24_000, 1),
            (96_000, 6),
        ] {
            let seconds = 0.5;
            let input = sine(rate, usize::from(channels), 1000.0, seconds, 0.3);
            let mut down = ToTranslator::new(rate, channels).unwrap();
            let mut up = FromTranslator::new(rate, channels).unwrap();
            let mut pcm = Vec::new();
            down.process(&input, &mut pcm);
            down.flush(&mut pcm);
            let mut back = Vec::new();
            up.process(&pcm, &mut back);
            up.flush(&mut back);

            let expected_pcm = (24_000.0 * seconds) as i64;
            assert!(
                (pcm.len() as i64 - expected_pcm).abs() <= 500,
                "{rate}/{channels}: {} translator samples",
                pcm.len()
            );
            assert_eq!(back.len() % usize::from(channels), 0);
            let frames = (back.len() / usize::from(channels)) as i64;
            let expected = (rate as f32 * seconds) as i64;
            let tolerance = i64::from(rate) / 20; // 50 ms
            assert!(
                (frames - expected).abs() <= tolerance,
                "{rate}/{channels}: {frames} frames, expected {expected}"
            );
        }
    }

    #[test]
    fn output_does_not_depend_on_chunk_size() {
        let input = sine(44_100, 2, 523.0, 0.3, 0.4);
        let run = |chunk: usize| {
            let mut converter = ToTranslator::new(44_100, 2).unwrap();
            let mut out = Vec::new();
            for piece in input.chunks(chunk * 2) {
                converter.process(piece, &mut out);
            }
            converter.flush(&mut out);
            out
        };
        let reference = run(441);
        for chunk in [1, 7, 128, 1000, 13_230] {
            assert_eq!(run(chunk), reference, "chunk of {chunk} frames");
        }

        let pcm: Vec<i16> = reference.clone();
        let run_up = |chunk: usize| {
            let mut converter = FromTranslator::new(48_000, 2).unwrap();
            let mut out = Vec::new();
            for piece in pcm.chunks(chunk) {
                converter.process(piece, &mut out);
            }
            converter.flush(&mut out);
            out
        };
        let reference_up = run_up(240);
        for chunk in [1, 333, 4800] {
            assert_eq!(run_up(chunk), reference_up, "chunk of {chunk} samples");
        }
    }

    #[test]
    fn quantize_clips_out_of_range_samples() {
        assert_eq!(quantize(0.0), 0);
        assert_eq!(quantize(1.0), i16::MAX);
        assert_eq!(quantize(-1.0), -i16::MAX);
        assert_eq!(quantize(3.5), i16::MAX);
        assert_eq!(quantize(-7.0), -i16::MAX);
        assert_eq!(quantize(f32::NAN), 0);
        assert_eq!(quantize(0.5), 16_384);

        // A loud signal through the converter is clipped, not wrapped around.
        let input = vec![1.8f32; 4800 * 2];
        let mut converter = ToTranslator::new(48_000, 2).unwrap();
        let mut out = Vec::new();
        converter.process(&input, &mut out);
        assert!(out[100..].iter().all(|&s| s == i16::MAX), "{:?}", &out[..4]);
    }

    #[test]
    fn rate_adjust_is_clamped_and_changes_the_output_rate() {
        let mut converter = FromTranslator::new(48_000, 1).unwrap();
        assert!((converter.rate_adjust() - 1.0).abs() < 1e-9);
        assert!((converter.set_rate_adjust(1.5) - (1.0 + MAX_RATE_ADJUST)).abs() < 1e-9);
        assert!((converter.set_rate_adjust(0.1) - (1.0 - MAX_RATE_ADJUST)).abs() < 1e-9);

        let pcm = vec![1000i16; 24_000];
        let mut normal = FromTranslator::new(48_000, 1).unwrap();
        let mut faster = FromTranslator::new(48_000, 1).unwrap();
        faster.set_rate_adjust(1.01);
        let (mut a, mut b) = (Vec::new(), Vec::new());
        normal.process(&pcm, &mut a);
        faster.process(&pcm, &mut b);
        let gained = b.len() as i64 - a.len() as i64;
        // About 1 % of 48 000 frames, minus the ramp.
        assert!((380..=500).contains(&gained), "gained {gained} frames");
        assert!((faster.rate_adjust() - 1.01).abs() < 1e-9);
    }

    #[test]
    fn rejects_invalid_formats() {
        assert!(ToTranslator::new(0, 2).is_err());
        assert!(ToTranslator::new(48_000, 0).is_err());
        assert!(FromTranslator::new(48_000, 0).is_err());
    }
}
