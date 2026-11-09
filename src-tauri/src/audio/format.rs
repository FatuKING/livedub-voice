//! Stream formats and sample conversion between WASAPI buffers (bytes) and `f32` samples.
//!
//! Everything here is pure and allocation free, so the audio threads can call it in their hot
//! loop and it can be unit tested without audio hardware.

use serde::Serialize;

/// How one sample is stored in a WASAPI buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SampleFormat {
    /// 32-bit IEEE float (the usual shared-mode mix format).
    F32,
    /// 16-bit signed integer.
    I16,
    /// 24-bit signed integer packed in 3 bytes.
    I24,
    /// 32-bit signed integer container (also used for 24 valid bits, left-justified).
    I32,
}

impl SampleFormat {
    /// Bytes one sample takes in the buffer.
    pub const fn bytes(self) -> usize {
        match self {
            Self::I16 => 2,
            Self::I24 => 3,
            Self::F32 | Self::I32 => 4,
        }
    }

    /// Picks the format from a WAVEFORMAT description, or `None` when it is not supported.
    pub fn from_wave(is_float: bool, bits_per_sample: u16) -> Option<Self> {
        match (is_float, bits_per_sample) {
            (true, 32) => Some(Self::F32),
            (false, 16) => Some(Self::I16),
            (false, 24) => Some(Self::I24),
            (false, 32) => Some(Self::I32),
            _ => None,
        }
    }
}

/// Format of a stream: what the device delivers (capture) or expects (playback).
///
/// Samples moving through the app are always `f32` interleaved; `sample_format` only says how the
/// device stores them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamFormat {
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_format: SampleFormat,
}

impl StreamFormat {
    /// Bytes per frame (one sample per channel).
    pub const fn frame_bytes(&self) -> usize {
        self.channels as usize * self.sample_format.bytes()
    }

    /// Number of frames in `ms` milliseconds (at least one).
    pub fn frames_for_ms(&self, ms: u32) -> usize {
        ((u64::from(self.sample_rate) * u64::from(ms) / 1000) as usize).max(1)
    }

    /// Whether `f32` samples captured in `self` can be played as-is on a device in `other`:
    /// same rate and channel count (the device sample format does not matter, conversion to and
    /// from `f32` handles it).
    pub fn matches_samples_of(&self, other: &StreamFormat) -> bool {
        self.sample_rate == other.sample_rate && self.channels == other.channels
    }
}

impl std::fmt::Display for StreamFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} Hz, {} ch, {:?}",
            self.sample_rate, self.channels, self.sample_format
        )
    }
}

const I16_SCALE: f32 = 32_768.0;
const I24_SCALE: f32 = 8_388_608.0;
const I32_SCALE: f32 = 2_147_483_648.0;

/// Converts device bytes to `f32` samples in `[-1.0, 1.0]`.
///
/// Converts `min(bytes / sample size, out.len())` samples and returns that count. Trailing bytes
/// that do not form a whole sample are ignored.
pub fn bytes_to_f32(format: SampleFormat, bytes: &[u8], out: &mut [f32]) -> usize {
    let size = format.bytes();
    let count = (bytes.len() / size).min(out.len());
    let samples = bytes.chunks_exact(size).take(count);
    for (dst, src) in out.iter_mut().zip(samples) {
        *dst = match format {
            SampleFormat::F32 => f32::from_le_bytes([src[0], src[1], src[2], src[3]]),
            SampleFormat::I16 => f32::from(i16::from_le_bytes([src[0], src[1]])) / I16_SCALE,
            // Put the 3 bytes in the top of an i32 so the sign is kept, then shift back down.
            SampleFormat::I24 => {
                (i32::from_le_bytes([0, src[0], src[1], src[2]]) >> 8) as f32 / I24_SCALE
            }
            SampleFormat::I32 => {
                i32::from_le_bytes([src[0], src[1], src[2], src[3]]) as f32 / I32_SCALE
            }
        };
    }
    count
}

/// Converts `f32` samples to device bytes, clamping to `[-1.0, 1.0]`.
///
/// Converts `min(samples.len(), out / sample size)` samples and returns that count.
pub fn f32_to_bytes(format: SampleFormat, samples: &[f32], out: &mut [u8]) -> usize {
    let size = format.bytes();
    let count = samples.len().min(out.len() / size);
    for (src, dst) in samples.iter().zip(out.chunks_exact_mut(size)).take(count) {
        let value = src.clamp(-1.0, 1.0);
        match format {
            SampleFormat::F32 => dst.copy_from_slice(&value.to_le_bytes()),
            SampleFormat::I16 => {
                let int = (value * I16_SCALE).clamp(f32::from(i16::MIN), f32::from(i16::MAX));
                dst.copy_from_slice(&(int as i16).to_le_bytes());
            }
            SampleFormat::I24 => {
                let int = (value * I24_SCALE).clamp(-I24_SCALE, I24_SCALE - 1.0) as i32;
                dst.copy_from_slice(&int.to_le_bytes()[..3]);
            }
            SampleFormat::I32 => {
                // `as` saturates, so 1.0 * 2^31 becomes i32::MAX.
                dst.copy_from_slice(&((value * I32_SCALE) as i32).to_le_bytes());
            }
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_f32(format: SampleFormat, bytes: &[u8]) -> Vec<f32> {
        let mut out = vec![0.0; bytes.len() / format.bytes()];
        let count = bytes_to_f32(format, bytes, &mut out);
        out.truncate(count);
        out
    }

    #[test]
    fn converts_i16_to_f32() {
        let bytes: Vec<u8> = [0_i16, i16::MAX, i16::MIN, 16_384]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();

        let samples = to_f32(SampleFormat::I16, &bytes);

        assert_eq!(samples[0], 0.0);
        assert!((samples[1] - 1.0).abs() < 1e-4);
        assert_eq!(samples[2], -1.0);
        assert_eq!(samples[3], 0.5);
    }

    #[test]
    fn converts_i24_to_f32_keeping_the_sign() {
        // 0x400000 = 0.5, 0xC00000 = -0.5, 0x800000 = -1.0
        let bytes = [0x00, 0x00, 0x40, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x80];

        assert_eq!(to_f32(SampleFormat::I24, &bytes), vec![0.5, -0.5, -1.0]);
    }

    #[test]
    fn converts_i32_to_f32() {
        let bytes: Vec<u8> = [0_i32, i32::MIN, 1 << 30]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();

        assert_eq!(to_f32(SampleFormat::I32, &bytes), vec![0.0, -1.0, 0.5]);
    }

    #[test]
    fn converts_f32_bytes_to_f32() {
        let bytes: Vec<u8> = [0.25_f32, -0.75]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();

        assert_eq!(to_f32(SampleFormat::F32, &bytes), vec![0.25, -0.75]);
    }

    #[test]
    fn conversion_stops_at_the_shorter_buffer() {
        let bytes = [0_u8; 8];
        let mut out = [1.0_f32; 2];

        // 4 i16 samples available, room for 2.
        assert_eq!(bytes_to_f32(SampleFormat::I16, &bytes, &mut out), 2);
        // A trailing partial sample is ignored.
        assert_eq!(bytes_to_f32(SampleFormat::F32, &bytes[..7], &mut out), 1);
    }

    #[test]
    fn f32_round_trips_through_every_format() {
        let samples = [0.0_f32, 0.5, -0.5, 0.999, -1.0];
        for format in [
            SampleFormat::F32,
            SampleFormat::I16,
            SampleFormat::I24,
            SampleFormat::I32,
        ] {
            let mut bytes = vec![0_u8; samples.len() * format.bytes()];
            assert_eq!(f32_to_bytes(format, &samples, &mut bytes), samples.len());
            let back = to_f32(format, &bytes);
            for (a, b) in samples.iter().zip(&back) {
                assert!((a - b).abs() < 1e-4, "{format:?}: {a} became {b}");
            }
        }
    }

    #[test]
    fn f32_to_bytes_clamps_out_of_range_samples() {
        let mut bytes = [0_u8; 4];
        f32_to_bytes(SampleFormat::I16, &[2.0, -2.0], &mut bytes);

        assert_eq!(
            to_f32(SampleFormat::I16, &bytes),
            vec![f32::from(i16::MAX) / 32_768.0, -1.0]
        );
    }

    #[test]
    fn picks_sample_format_from_wave_description() {
        assert_eq!(SampleFormat::from_wave(true, 32), Some(SampleFormat::F32));
        assert_eq!(SampleFormat::from_wave(false, 16), Some(SampleFormat::I16));
        assert_eq!(SampleFormat::from_wave(false, 24), Some(SampleFormat::I24));
        assert_eq!(SampleFormat::from_wave(false, 32), Some(SampleFormat::I32));
        assert_eq!(SampleFormat::from_wave(true, 64), None);
        assert_eq!(SampleFormat::from_wave(false, 8), None);
    }

    #[test]
    fn stream_format_helpers() {
        let format = StreamFormat {
            sample_rate: 48_000,
            channels: 2,
            sample_format: SampleFormat::F32,
        };

        assert_eq!(format.frame_bytes(), 8);
        assert_eq!(format.frames_for_ms(10), 480);
        assert!(format.matches_samples_of(&StreamFormat {
            sample_format: SampleFormat::I16,
            ..format
        }));
        assert!(!format.matches_samples_of(&StreamFormat {
            channels: 1,
            ..format
        }));
    }
}
