//! Local usage metering.
//!
//! This module will track translated audio minutes per session and per direction, estimate their
//! cost from the model's per-minute price, and persist the history as a JSON file in the app data
//! folder.
