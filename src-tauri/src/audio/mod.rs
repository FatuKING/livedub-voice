//! Audio capture and playback on Windows (WASAPI).
//!
//! This module will hold:
//! - process loopback capture of the other participants' audio (a target process tree, or the
//!   whole system minus LiveDub Voice's own process tree, to avoid feedback loops);
//! - microphone capture for the outgoing direction;
//! - playback of translated audio to the user's headphones (mixed with the ducked original) and
//!   to the virtual cable's playback endpoint;
//! - resampling to and from the 24 kHz mono PCM16 format the translator expects;
//! - device enumeration, including detection of known virtual audio cables.
