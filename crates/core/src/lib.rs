//! Domain layer for Octava: project model, timeline math, edit commands and
//! playback snapshot compilation.
//!
//! This crate has no dependency on audio drivers or a UI toolkit.

#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod command;
pub mod model;
pub mod snapshot;
pub mod store;
pub mod time;

pub use command::{EditCommand, EditOutcome};
pub use model::{ClipId, MidiClip, ModelError, Note, NoteId, Preset, Project, Track, TrackId};
pub use snapshot::{EventKind, PlaybackSnapshot, SnapshotTrack, TimedEvent};
pub use store::ProjectStore;
pub use time::{
    DEFAULT_BPM, DEFAULT_PPQ, DEFAULT_SAMPLE_RATE, TempoMap, TempoSegment, Ticks, TimeError,
    TimeSignature,
};
