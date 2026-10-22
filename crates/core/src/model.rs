use serde::{Deserialize, Serialize};

use crate::time::{TempoMap, Ticks, TimeError, TimeSignature};

macro_rules! id_type {
    ($name:ident) => {
        #[derive(
            Clone,
            Copy,
            Debug,
            Default,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            Serialize,
            Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub u64);

        impl $name {
            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}#{}", stringify!($name), self.0)
            }
        }
    };
}

id_type!(TrackId);
id_type!(ClipId);
id_type!(NoteId);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preset {
    pub bank: u8,
    pub program: u8,
}

impl Preset {
    #[must_use]
    pub const fn new(bank: u8, program: u8) -> Self {
        Self { bank, program }
    }
}

impl std::fmt::Display for Preset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "bank {}, program {}", self.bank, self.program)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub id: NoteId,
    pub key: u8,
    pub velocity: u8,
    /// Start relative to the containing clip.
    pub start: Ticks,
    pub length: Ticks,
}

impl Note {
    #[must_use]
    pub fn end(&self) -> Ticks {
        self.start.saturating_add(self.length)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MidiClip {
    pub id: ClipId,
    pub start: Ticks,
    pub length: Ticks,
    pub notes: Vec<Note>,
}

impl MidiClip {
    #[must_use]
    pub fn end(&self) -> Ticks {
        self.start.saturating_add(self.length)
    }

    #[must_use]
    pub fn note(&self, id: NoteId) -> Option<&Note> {
        self.notes.iter().find(|note| note.id == id)
    }

    #[must_use]
    pub fn note_index(&self, id: NoteId) -> Option<usize> {
        self.notes.iter().position(|note| note.id == id)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: TrackId,
    pub name: String,
    /// MIDI channel in `0..=15`.
    pub channel: u8,
    pub preset: Preset,
    /// Linear gain, `1.0` is unity. Values above unity are not supported by
    /// the current engine, so this is also the maximum.
    pub gain: f32,
    /// Stereo pan in `-1.0..=1.0`.
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    pub clips: Vec<MidiClip>,
}

impl Track {
    fn new(id: TrackId, name: String, channel: u8) -> Self {
        Self {
            id,
            name,
            channel,
            preset: Preset::default(),
            gain: 1.0,
            pan: 0.0,
            mute: false,
            solo: false,
            clips: Vec::new(),
        }
    }

    #[must_use]
    pub fn is_audible(&self, any_solo: bool) -> bool {
        !self.mute && (!any_solo || self.solo)
    }

    #[must_use]
    pub fn clip(&self, id: ClipId) -> Option<&MidiClip> {
        self.clips.iter().find(|clip| clip.id == id)
    }

    #[must_use]
    pub fn clip_index(&self, id: ClipId) -> Option<usize> {
        self.clips.iter().position(|clip| clip.id == id)
    }

    #[must_use]
    pub fn note_count(&self) -> usize {
        self.clips.iter().map(|clip| clip.notes.len()).sum()
    }
}

/// Authoritative project document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub tempo_map: TempoMap,
    pub time_signature: TimeSignature,
    pub tracks: Vec<Track>,
    #[serde(default = "next_id_default")]
    next_track_id: u64,
    #[serde(default = "next_id_default")]
    next_clip_id: u64,
    #[serde(default = "next_id_default")]
    next_note_id: u64,
}

impl Default for Project {
    fn default() -> Self {
        Self::new("Untitled")
    }
}

impl Project {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            tempo_map: TempoMap::default(),
            time_signature: TimeSignature::default(),
            tracks: Vec::new(),
            next_track_id: 1,
            next_clip_id: 1,
            next_note_id: 1,
        }
    }

    #[must_use]
    pub fn track(&self, id: TrackId) -> Option<&Track> {
        self.tracks.iter().find(|track| track.id == id)
    }

    #[must_use]
    pub fn track_mut(&mut self, id: TrackId) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|track| track.id == id)
    }

    #[must_use]
    pub fn track_index(&self, id: TrackId) -> Option<usize> {
        self.tracks.iter().position(|track| track.id == id)
    }

    pub fn add_track(&mut self, name: impl Into<String>) -> Result<TrackId, ModelError> {
        let mut used_channels = [false; 16];
        for track in &self.tracks {
            if let Some(slot) = used_channels.get_mut(usize::from(track.channel)) {
                *slot = true;
            }
        }
        let channel = used_channels
            .iter()
            .position(|used| !used)
            .ok_or(ModelError::NoFreeChannel)? as u8;

        let id = TrackId(self.next_track_id);
        self.next_track_id += 1;
        self.tracks.push(Track::new(id, name.into(), channel));
        Ok(id)
    }

    pub fn add_clip(
        &mut self,
        track_id: TrackId,
        start: Ticks,
        length: Ticks,
    ) -> Result<ClipId, ModelError> {
        if length == Ticks::ZERO {
            return Err(ModelError::InvalidClipLength);
        }

        let id = ClipId(self.next_clip_id);
        self.next_clip_id += 1;
        let track = self
            .track_mut(track_id)
            .ok_or(ModelError::TrackNotFound(track_id))?;
        track.clips.push(MidiClip {
            id,
            start,
            length,
            notes: Vec::new(),
        });
        Ok(id)
    }

    pub fn add_note(
        &mut self,
        track_id: TrackId,
        clip_id: ClipId,
        key: u8,
        velocity: u8,
        start: Ticks,
        length: Ticks,
    ) -> Result<NoteId, ModelError> {
        validate_key(key)?;
        validate_velocity(velocity)?;
        {
            let track = self
                .track(track_id)
                .ok_or(ModelError::TrackNotFound(track_id))?;
            let clip = track
                .clip(clip_id)
                .ok_or(ModelError::ClipNotFound(clip_id))?;
            validate_note_geometry(start, length, clip.length)?;
        }

        let id = NoteId(self.next_note_id);
        self.next_note_id += 1;
        let track = self
            .track_mut(track_id)
            .ok_or(ModelError::TrackNotFound(track_id))?;
        let clip = track
            .clips
            .iter_mut()
            .find(|clip| clip.id == clip_id)
            .ok_or(ModelError::ClipNotFound(clip_id))?;
        clip.notes.push(Note {
            id,
            key,
            velocity,
            start,
            length,
        });
        Ok(id)
    }

    #[must_use]
    pub fn any_solo(&self) -> bool {
        self.tracks.iter().any(|track| track.solo)
    }

    /// Length of the arrangement in ticks, based on the furthest clip end.
    #[must_use]
    pub fn length_ticks(&self) -> Ticks {
        self.tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .map(MidiClip::end)
            .max()
            .unwrap_or(Ticks::ZERO)
    }

    #[must_use]
    pub fn note_count(&self) -> usize {
        self.tracks.iter().map(Track::note_count).sum()
    }

    pub fn apply(
        &mut self,
        command: &crate::command::EditCommand,
    ) -> Result<crate::command::EditOutcome, ModelError> {
        crate::command::apply(self, command)
    }
}

fn next_id_default() -> u64 {
    1
}

pub(crate) fn validate_key(key: u8) -> Result<(), ModelError> {
    if key <= 127 {
        Ok(())
    } else {
        Err(ModelError::InvalidKey(i32::from(key)))
    }
}

pub(crate) fn validate_velocity(velocity: u8) -> Result<(), ModelError> {
    if (1..=127).contains(&velocity) {
        Ok(())
    } else {
        Err(ModelError::InvalidVelocity(velocity))
    }
}

pub(crate) fn validate_note_geometry(
    start: Ticks,
    length: Ticks,
    clip_length: Ticks,
) -> Result<(), ModelError> {
    if length == Ticks::ZERO {
        return Err(ModelError::InvalidNoteLength);
    }
    if start >= clip_length {
        return Err(ModelError::NoteOutsideClip);
    }
    Ok(())
}

pub(crate) fn validate_preset(preset: Preset) -> Result<(), ModelError> {
    if preset.program > 127 {
        return Err(ModelError::InvalidPreset(preset));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ModelError {
    #[error("track {0} not found")]
    TrackNotFound(TrackId),
    #[error("clip {0} not found")]
    ClipNotFound(ClipId),
    #[error("note {0} not found")]
    NoteNotFound(NoteId),
    #[error("all 16 MIDI channels are already in use")]
    NoFreeChannel,
    #[error("MIDI key must be in 0..=127, got {0}")]
    InvalidKey(i32),
    #[error("MIDI velocity must be in 1..=127, got {0}")]
    InvalidVelocity(u8),
    #[error("clip length must be greater than zero")]
    InvalidClipLength,
    #[error("note length must be greater than zero")]
    InvalidNoteLength,
    #[error("note start must be inside its clip")]
    NoteOutsideClip,
    #[error("track name must not be empty")]
    EmptyTrackName,
    #[error("track gain must be finite and in 0.0..=1.0")]
    InvalidGain,
    #[error("track pan must be in -1.0..=1.0")]
    InvalidPan,
    #[error("invalid preset: {0}")]
    InvalidPreset(Preset),
    #[error("time signature is invalid")]
    InvalidTimeSignature,
    #[error(transparent)]
    Time(#[from] TimeError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_counters_survive_roundtrip() {
        let mut project = Project::new("Test");
        let track = project.add_track("Piano").unwrap();
        let clip = project.add_clip(track, Ticks::ZERO, Ticks(3840)).unwrap();
        let first = project
            .add_note(track, clip, 60, 100, Ticks::ZERO, Ticks(480))
            .unwrap();
        let second = project
            .add_note(track, clip, 64, 100, Ticks(480), Ticks(480))
            .unwrap();

        assert_ne!(first, second);

        let json = serde_json::to_string(&project).unwrap();
        let restored: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(project, restored);

        let mut restored = restored;
        let third = restored
            .add_note(track, clip, 67, 100, Ticks(960), Ticks(480))
            .unwrap();
        assert_ne!(third, first);
        assert_ne!(third, second);
    }

    #[test]
    fn rejects_invalid_notes() {
        let mut project = Project::new("Test");
        let track = project.add_track("Piano").unwrap();
        let clip = project.add_clip(track, Ticks::ZERO, Ticks(960)).unwrap();

        assert!(
            project
                .add_note(track, clip, 128, 100, Ticks::ZERO, Ticks(100))
                .is_err()
        );
        assert!(
            project
                .add_note(track, clip, 60, 0, Ticks::ZERO, Ticks(100))
                .is_err()
        );
        assert!(
            project
                .add_note(track, clip, 60, 100, Ticks(960), Ticks(100))
                .is_err()
        );
        assert!(
            project
                .add_note(track, clip, 60, 100, Ticks::ZERO, Ticks::ZERO)
                .is_err()
        );
    }

    #[test]
    fn channels_are_allocated_without_collisions() {
        let mut project = Project::new("Test");
        for index in 0..16 {
            let id = project.add_track(format!("Track {index}")).unwrap();
            assert_eq!(project.track(id).unwrap().channel, index);
        }
        assert_eq!(
            project.add_track("Overflow"),
            Err(ModelError::NoFreeChannel)
        );
    }

    #[test]
    fn length_is_the_furthest_clip_end() {
        let mut project = Project::new("Test");
        let a = project.add_track("A").unwrap();
        let b = project.add_track("B").unwrap();
        project.add_clip(a, Ticks(100), Ticks(500)).unwrap();
        project.add_clip(b, Ticks(2000), Ticks(10)).unwrap();

        assert_eq!(project.length_ticks(), Ticks(2010));
    }
}
