//! Domain layer for Octava: project model, timeline math, edit commands and
//! playback snapshot compilation.
//!
//! This crate has no dependency on audio drivers or a UI toolkit.

#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod command;
pub mod model;
pub mod persist;
pub mod sample;
pub mod snapshot;
pub mod store;
pub mod time;

pub use command::{EditCommand, EditOutcome};
pub use model::{
    Clip, ClipId, EffectId, EffectKind, EffectSlot, ModelError, Note, NoteId, Pattern, PatternId,
    Preset, Project, SampleClip, SampleId, Track, TrackId,
};
pub use persist::{
    PROJECT_EXTENSION, PROJECT_FORMAT, PROJECT_VERSION, PersistError, decode_project,
    encode_project,
};
pub use sample::{SampleCache, SampleData, SampleError, SampleInfo, load_sample, probe_sample};
pub use snapshot::{EventKind, PlaybackSnapshot, SnapshotTrack, TimedEvent, TimedSample};
pub use store::ProjectStore;
pub use time::{
    DEFAULT_BPM, DEFAULT_PPQ, DEFAULT_SAMPLE_RATE, TempoMap, TempoSegment, Ticks, TimeError,
    TimeSignature,
};
