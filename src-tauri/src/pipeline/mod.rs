//! Translation pipelines.
//!
//! This module will wire audio capture, the translator and audio output together into the two
//! independent directions:
//! - **incoming:** another app's audio -> translator (to the user's language) -> headphones;
//! - **outgoing:** microphone -> translator (to the other side's language) -> virtual cable.
//!
//! It will also own starting and stopping each direction, and emit status, caption and usage
//! events to the panel.
