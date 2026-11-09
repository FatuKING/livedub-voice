//! Input level metering (RMS and peak) for the panel's VU meter.
//!
//! The capture thread feeds every sample to a [`LevelMeter`], which publishes one reading per
//! window (~50 ms, so ~20 Hz) into a [`SharedLevel`]: two atomics, no locks, no allocation.
//! Anyone holding a clone of the [`SharedLevel`] can read the latest value at any time.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use serde::Serialize;

/// Length of one metering window. 50 ms gives ~20 readings per second.
pub const LEVEL_WINDOW_MS: u32 = 50;

/// One level reading, linear amplitude in `[0.0, 1.0]` (1.0 = full scale).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputLevel {
    /// Root mean square over the window: how loud it sounds.
    pub rms: f32,
    /// Largest absolute sample in the window: how close it is to clipping.
    pub peak: f32,
}

/// Latest level, shared lock-free between the capture thread and readers.
#[derive(Debug, Clone, Default)]
pub struct SharedLevel {
    rms: Arc<AtomicU32>,
    peak: Arc<AtomicU32>,
}

impl SharedLevel {
    /// The latest published reading.
    pub fn get(&self) -> InputLevel {
        InputLevel {
            rms: f32::from_bits(self.rms.load(Ordering::Relaxed)),
            peak: f32::from_bits(self.peak.load(Ordering::Relaxed)),
        }
    }

    fn set(&self, level: InputLevel) {
        self.rms.store(level.rms.to_bits(), Ordering::Relaxed);
        self.peak.store(level.peak.to_bits(), Ordering::Relaxed);
    }
}

/// Accumulates samples and publishes a reading every `window_samples` samples.
#[derive(Debug)]
pub struct LevelMeter {
    shared: SharedLevel,
    window_samples: usize,
    count: usize,
    sum_squares: f64,
    peak: f32,
}

impl LevelMeter {
    /// A meter that publishes into `shared` every `window_samples` samples (all channels count).
    pub fn new(shared: SharedLevel, window_samples: usize) -> Self {
        Self {
            shared,
            window_samples: window_samples.max(1),
            count: 0,
            sum_squares: 0.0,
            peak: 0.0,
        }
    }

    /// Adds interleaved samples, publishing a reading each time a window fills up.
    pub fn process(&mut self, samples: &[f32]) {
        for &sample in samples {
            self.sum_squares += f64::from(sample) * f64::from(sample);
            self.peak = self.peak.max(sample.abs());
            self.count += 1;
            if self.count == self.window_samples {
                self.shared.set(InputLevel {
                    rms: (self.sum_squares / self.count as f64).sqrt().min(1.0) as f32,
                    peak: self.peak.min(1.0),
                });
                self.count = 0;
                self.sum_squares = 0.0;
                self.peak = 0.0;
            }
        }
    }

    /// Publishes silence (used when the stream stops, so the meter falls back to zero).
    pub fn reset(&mut self) {
        self.count = 0;
        self.sum_squares = 0.0;
        self.peak = 0.0;
        self.shared.set(InputLevel::default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meter(window: usize) -> (LevelMeter, SharedLevel) {
        let shared = SharedLevel::default();
        (LevelMeter::new(shared.clone(), window), shared)
    }

    #[test]
    fn silence_reads_zero() {
        let (mut meter, shared) = meter(4);
        meter.process(&[0.0; 4]);

        assert_eq!(
            shared.get(),
            InputLevel {
                rms: 0.0,
                peak: 0.0
            }
        );
    }

    #[test]
    fn full_scale_square_wave_reads_one() {
        let (mut meter, shared) = meter(4);
        meter.process(&[1.0, -1.0, 1.0, -1.0]);

        assert_eq!(
            shared.get(),
            InputLevel {
                rms: 1.0,
                peak: 1.0
            }
        );
    }

    #[test]
    fn sine_rms_is_peak_over_root_two() {
        let (mut meter, shared) = meter(4800);
        let sine: Vec<f32> = (0..4800)
            .map(|i| 0.5 * (i as f32 * 2.0 * std::f32::consts::PI * 440.0 / 48_000.0).sin())
            .collect();
        meter.process(&sine);

        let level = shared.get();
        assert!((level.rms - 0.5 / 2_f32.sqrt()).abs() < 1e-3, "{level:?}");
        assert!((level.peak - 0.5).abs() < 1e-3, "{level:?}");
    }

    #[test]
    fn publishes_only_when_a_window_is_complete() {
        let (mut meter, shared) = meter(4);
        meter.process(&[0.5, -0.5, 0.5]);
        assert_eq!(shared.get(), InputLevel::default());

        meter.process(&[-0.5]);
        assert_eq!(
            shared.get(),
            InputLevel {
                rms: 0.5,
                peak: 0.5
            }
        );
    }

    #[test]
    fn each_window_starts_from_scratch() {
        let (mut meter, shared) = meter(2);
        meter.process(&[1.0, 1.0, 0.25, -0.25]);

        assert_eq!(
            shared.get(),
            InputLevel {
                rms: 0.25,
                peak: 0.25
            }
        );
    }

    #[test]
    fn reset_publishes_silence() {
        let (mut meter, shared) = meter(2);
        meter.process(&[1.0, 1.0]);
        meter.reset();

        assert_eq!(shared.get(), InputLevel::default());
    }
}
