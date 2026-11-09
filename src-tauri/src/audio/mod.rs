//! Audio capture and playback on Windows (WASAPI).
//!
//! Today this module lists audio devices ([`devices`]), detects known virtual audio cables
//! ([`virtual_cable`]), captures input devices ([`capture`]), plays audio ([`playback`]) and
//! tests the microphone ([`monitor`]), and resamples to and from the 24 kHz mono PCM16 format the
//! translator expects ([`resample`]). Audio threads hand samples to the rest of the app through
//! lock-free bounded queues ([`ring`]). It will also hold:
//! - process loopback capture of the other participants' audio (a target process tree, or the
//!   whole system minus LiveDub Voice's own process tree, to avoid feedback loops);
//! - playback of translated audio to the user's headphones (mixed with the ducked original) and
//!   to the virtual cable's playback endpoint.

pub mod capture;
pub mod devices;
pub mod format;
pub mod level;
pub mod monitor;
pub mod playback;
pub mod resample;
pub mod ring;
pub mod stream;
pub mod virtual_cable;

pub use capture::{CaptureConfig, CaptureStream, start_capture};
pub use devices::{AudioDevice, DeviceKind, list_devices};
pub use format::{SampleFormat, StreamFormat};
pub use level::InputLevel;
pub use monitor::{Monitor, MonitorEvent};
pub use playback::{PlaybackConfig, PlaybackStream, start_playback};
pub use resample::{FromTranslator, ToTranslator};
pub use ring::{AudioConsumer, AudioProducer, audio_ring};
pub use virtual_cable::{KNOWN_VIRTUAL_CABLES, VirtualCable, detect_virtual_cable};
