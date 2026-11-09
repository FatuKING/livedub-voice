//! Audio capture and playback on Windows (WASAPI).
//!
//! Today this module lists audio devices ([`devices`]) and detects known virtual audio cables
//! ([`virtual_cable`]). It will also hold:
//! - process loopback capture of the other participants' audio (a target process tree, or the
//!   whole system minus LiveDub Voice's own process tree, to avoid feedback loops);
//! - microphone capture for the outgoing direction;
//! - playback of translated audio to the user's headphones (mixed with the ducked original) and
//!   to the virtual cable's playback endpoint;
//! - resampling to and from the 24 kHz mono PCM16 format the translator expects.

pub mod devices;
pub mod resample;
pub mod virtual_cable;

pub use devices::{AudioDevice, DeviceKind, list_devices};
pub use resample::{FromTranslator, ToTranslator};
pub use virtual_cable::{KNOWN_VIRTUAL_CABLES, VirtualCable, detect_virtual_cable};
