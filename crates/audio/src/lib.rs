//! Real-time audio engine for Octava.
//!
//! The engine runs a minimal CPAL output stream and a FluidLite SoundFont
//! synthesizer. UI and transport commands reach the callback through a
//! wait-free SPSC queue; playback snapshots are reference counted and retired
//! on the UI thread so the audio callback never deallocates a project.

#![cfg_attr(test, allow(clippy::unwrap_used))]

mod engine;
mod offline;
mod runtime;
mod soundfont;

pub use engine::{
    AudioEngine, AudioError, AudioSettingsConfig, AudioTelemetry, PreviewNote, TransportState,
    list_output_devices,
};
pub use offline::OfflineRenderer;
pub use soundfont::{SoundFontError, SoundFontPreset, default_soundfont_path, list_presets};
