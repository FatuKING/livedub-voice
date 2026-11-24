//! Sound cues for the shortcuts: a short, quiet sound in the headphones when a shortcut turns a
//! direction on or off, so the user knows it worked without looking at the panel.
//!
//! Each cue is synthesized (soft sine tones with smooth edges, about −20 dBFS) and played on its
//! own short-lived WASAPI stream on the headphones (the incoming output, or the default device),
//! from a background thread. LiveDub Voice's own audio is never captured by the incoming
//! direction, so a cue is never translated; it never goes to the virtual cable either.

use std::time::Duration;

use crate::audio::{PlaybackConfig, StreamFormat, audio_ring, start_playback};

/// Which sound to play.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    /// A direction turned on: two rising tones.
    On,
    /// A direction turned off: two falling tones.
    Off,
    /// A shortcut could not start a direction: two low tones.
    Error,
}

/// Peak amplitude of a tone (about −20 dBFS: audible over a game, never startling).
const AMPLITUDE: f32 = 0.1;

/// Fade in and out of each tone, so it starts and ends without a click.
const FADE_MS: f32 = 8.0;

/// Silence played after the cue, so the stream does not end on the last samples.
const TAIL_MS: u32 = 60;

/// The tones of a cue: (frequency in Hz, or 0 for a pause; length in ms).
fn tones(cue: Cue) -> &'static [(f32, u32)] {
    match cue {
        Cue::On => &[(660.0, 70), (0.0, 20), (990.0, 90)],
        Cue::Off => &[(990.0, 70), (0.0, 20), (660.0, 90)],
        Cue::Error => &[(330.0, 90), (0.0, 50), (330.0, 90)],
    }
}

/// The cue's length.
pub fn duration(cue: Cue) -> Duration {
    let ms: u32 = tones(cue).iter().map(|(_, ms)| ms).sum();
    Duration::from_millis(u64::from(ms + TAIL_MS))
}

/// The cue as interleaved `f32` samples at `sample_rate`, the same on every channel.
pub fn render(cue: Cue, sample_rate: u32, channels: u16) -> Vec<f32> {
    let rate = sample_rate as f32;
    let mut mono = Vec::new();
    for &(frequency, ms) in tones(cue) {
        let length = (rate * ms as f32 / 1000.0).round() as usize;
        let fade = (rate * FADE_MS / 1000.0).max(1.0);
        for n in 0..length {
            if frequency <= 0.0 {
                mono.push(0.0);
                continue;
            }
            let edge = (n.min(length - 1 - n) as f32 / fade).min(1.0);
            // Raised cosine: 0 at the edges, 1 inside.
            let envelope = 0.5 - 0.5 * (edge * std::f32::consts::PI).cos();
            let phase = n as f32 / rate * frequency * std::f32::consts::TAU;
            mono.push(AMPLITUDE * envelope * phase.sin());
        }
    }
    mono.extend(std::iter::repeat_n(
        0.0,
        (rate * TAIL_MS as f32 / 1000.0) as usize,
    ));
    let channels = usize::from(channels.max(1));
    mono.iter()
        .flat_map(|&sample| std::iter::repeat_n(sample, channels))
        .collect()
}

/// Plays `cue` on `device_id` (`None` = the default output) in the background. Failures are
/// only logged: a missing cue must never get in the way.
pub fn play(cue: Cue, device_id: Option<String>) {
    let spawned = std::thread::Builder::new()
        .name("livedub-cue".into())
        .spawn(move || {
            if let Err(error) = play_blocking(cue, device_id.as_deref()) {
                tracing::debug!(%error, ?cue, "could not play a sound cue");
            }
        });
    if let Err(error) = spawned {
        tracing::debug!(%error, "could not start the sound cue thread");
    }
}

fn play_blocking(cue: Cue, device_id: Option<&str>) -> crate::error::AppResult<()> {
    let format: StreamFormat = crate::audio::playback::output_device_format(device_id)?;
    let samples = render(cue, format.sample_rate, format.channels);
    let length = duration(cue);
    let queue_ms = length.as_millis() as u32 + 100;
    let (mut producer, consumer) = audio_ring(format.channels, format.frames_for_ms(queue_ms));
    producer.push(&samples);
    let mut stream = start_playback(
        device_id,
        consumer,
        format,
        PlaybackConfig {
            prebuffer_ms: 10,
            max_queue_ms: queue_ms,
        },
    )?;
    std::thread::sleep(length + Duration::from_millis(100));
    stream.stop();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cues_are_short_quiet_and_click_free() {
        for cue in [Cue::On, Cue::Off, Cue::Error] {
            let samples = render(cue, 48_000, 2);
            let frames = samples.len() / 2;
            assert_eq!(
                frames as u128,
                duration(cue).as_millis() * 48,
                "{cue:?} length"
            );
            assert!(
                duration(cue) <= Duration::from_millis(300),
                "{cue:?} too long"
            );
            let peak = samples.iter().fold(0.0_f32, |peak, s| peak.max(s.abs()));
            assert!((0.05..=AMPLITUDE).contains(&peak), "{cue:?} peak {peak}");
            // Starts and ends at silence (no click), the same on both channels.
            assert!(samples[0].abs() < 1e-3);
            assert!(samples.last().unwrap().abs() < 1e-3);
            assert!(samples.chunks(2).all(|frame| frame[0] == frame[1]));
        }
    }

    #[test]
    fn on_rises_and_off_falls() {
        assert!(tones(Cue::On)[0].0 < tones(Cue::On)[2].0);
        assert!(tones(Cue::Off)[0].0 > tones(Cue::Off)[2].0);
    }
}
