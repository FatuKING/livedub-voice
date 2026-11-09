//! OpenAI realtime translation client.
//!
//! This module will hold the WebSocket client for `gpt-realtime-translate`: opening a session with
//! the target language, streaming PCM16 input audio, and surfacing translated audio chunks and
//! transcript deltas. Each pipeline direction owns its own translator session.
