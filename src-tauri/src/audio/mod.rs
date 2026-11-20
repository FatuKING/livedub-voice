//! Audio capture and playback on Windows (WASAPI).
//!
//! Today this module lists audio devices ([`devices`]), detects known virtual audio cables
//! ([`virtual_cable`]), captures input devices ([`capture`]), lists the apps playing audio
//! ([`apps`]) and captures them through process loopback ([`loopback`]: one app's process tree,
//! or the whole system minus LiveDub Voice's own tree, to avoid feedback loops), plays audio
//! ([`playback`]) and tests the microphone ([`monitor`]), and resamples to and from the 24 kHz
//! mono PCM16 format the translator expects ([`resample`]). Audio threads hand samples to the
//! rest of the app through lock-free bounded queues ([`ring`]). While the incoming direction
//! runs, [`ducking`] lowers the captured apps' own volume under the translation.

pub mod apps;
pub mod capture;
pub mod devices;
pub mod ducking;
pub mod format;
pub mod level;
pub mod loopback;
pub mod monitor;
pub mod playback;
pub mod resample;
pub mod ring;
pub mod stream;
pub mod virtual_cable;

pub use apps::{AudioApp, list_audio_apps, resolve_source};
pub use capture::{CaptureConfig, CaptureStream, start_capture};
pub use devices::{AudioDevice, DeviceKind, list_devices};
pub use format::{SampleFormat, StreamFormat};
pub use level::InputLevel;
pub use loopback::{LoopbackConfig, LoopbackTarget, start_loopback_capture};
pub use monitor::{Monitor, MonitorEvent};
pub use playback::{PlaybackConfig, PlaybackStream, start_playback};
pub use resample::{FromTranslator, ToTranslator};
pub use ring::{AudioConsumer, AudioProducer, audio_ring};
pub use virtual_cable::{KNOWN_VIRTUAL_CABLES, VirtualCable, detect_virtual_cable};
