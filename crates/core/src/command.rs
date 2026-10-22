use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::model::{
    ClipId, ModelError, NoteId, PatternId, Preset, Project, TrackId, validate_note_geometry,
    validate_preset, validate_velocity,
};
use crate::time::{Ticks, TimeSignature};

/// A single user-visible project mutation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EditCommand {
    AddTrack {
        name: String,
    },
    RemoveTrack {
        track: TrackId,
    },
    MoveTrack {
        track: TrackId,
        to_index: usize,
    },
    SetTrackName {
        track: TrackId,
        name: String,
    },
    SetTrackParams {
        track: TrackId,
        gain: Option<f32>,
        pan: Option<f32>,
        mute: Option<bool>,
        solo: Option<bool>,
        preset: Option<Preset>,
    },
    /// Creates an empty reusable pattern in the clips library.
    AddPattern {
        name: String,
        length: Ticks,
    },
    /// Copies a pattern and all of its notes under a new id.
    ClonePattern {
        pattern: PatternId,
    },
    /// Removes a pattern together with every clip that plays it.
    RemovePattern {
        pattern: PatternId,
    },
    SetPatternPreset {
        pattern: PatternId,
        preset: Preset,
    },
    /// Places a pattern on a track's timeline.
    AddClip {
        track: TrackId,
        pattern: PatternId,
        start: Ticks,
    },
    RemoveClip {
        track: TrackId,
        clip: ClipId,
    },
    MoveClip {
        track: TrackId,
        clip: ClipId,
        start: Ticks,
    },
    ResizeClip {
        track: TrackId,
        clip: ClipId,
        length: Ticks,
    },
    AddNote {
        pattern: PatternId,
        key: u8,
        velocity: u8,
        start: Ticks,
        length: Ticks,
    },
    RemoveNotes {
        pattern: PatternId,
        notes: Vec<NoteId>,
    },
    MoveNotes {
        pattern: PatternId,
        notes: Vec<NoteId>,
        delta_ticks: i64,
        delta_key: i32,
    },
    ResizeNote {
        pattern: PatternId,
        note: NoteId,
        start: Ticks,
        length: Ticks,
    },
    SetNoteVelocity {
        pattern: PatternId,
        note: NoteId,
        velocity: u8,
    },
    SetTempo {
        tick: Ticks,
        bpm: f64,
    },
    SetTimeSignature {
        time_signature: TimeSignature,
    },
}

/// Side effects of a command that the UI may need, such as freshly allocated IDs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EditOutcome {
    pub track: Option<TrackId>,
    pub pattern: Option<PatternId>,
    pub clip: Option<ClipId>,
    pub note: Option<NoteId>,
    pub removed_notes: usize,
}

pub(crate) fn apply(
    project: &mut Project,
    command: &EditCommand,
) -> Result<EditOutcome, ModelError> {
    let mut outcome = EditOutcome::default();

    match command {
        EditCommand::AddTrack { name } => {
            validate_track_name(name)?;
            let track = project.add_track(name.trim())?;
            outcome.track = Some(track);
        }
        EditCommand::RemoveTrack { track } => {
            let index = project
                .track_index(*track)
                .ok_or(ModelError::TrackNotFound(*track))?;
            project.tracks.remove(index);
            outcome.track = Some(*track);
        }
        EditCommand::MoveTrack { track, to_index } => {
            let from = project
                .track_index(*track)
                .ok_or(ModelError::TrackNotFound(*track))?;
            let last = project.tracks.len().saturating_sub(1);
            let to = (*to_index).min(last);
            if from != to {
                let stored = project.tracks.remove(from);
                project.tracks.insert(to, stored);
            }
            outcome.track = Some(*track);
        }
        EditCommand::SetTrackName { track, name } => {
            validate_track_name(name)?;
            let stored = project
                .track_mut(*track)
                .ok_or(ModelError::TrackNotFound(*track))?;
            stored.name = name.trim().to_owned();
            outcome.track = Some(*track);
        }
        EditCommand::SetTrackParams {
            track,
            gain,
            pan,
            mute,
            solo,
            preset,
        } => {
            if let Some(gain) = gain
                && (!gain.is_finite() || !(0.0..=1.0).contains(gain))
            {
                return Err(ModelError::InvalidGain);
            }
            if let Some(pan) = pan
                && (!pan.is_finite() || !(-1.0..=1.0).contains(pan))
            {
                return Err(ModelError::InvalidPan);
            }
            if let Some(preset) = preset {
                validate_preset(*preset)?;
            }

            let stored = project
                .track_mut(*track)
                .ok_or(ModelError::TrackNotFound(*track))?;
            if let Some(gain) = gain {
                stored.gain = *gain;
            }
            if let Some(pan) = pan {
                stored.pan = *pan;
            }
            if let Some(mute) = mute {
                stored.mute = *mute;
            }
            if let Some(solo) = solo {
                stored.solo = *solo;
            }
            if let Some(preset) = preset {
                stored.preset = *preset;
            }
            outcome.track = Some(*track);
        }
        EditCommand::AddPattern { name, length } => {
            validate_pattern_name(name)?;
            let pattern = project.add_pattern(name.trim(), *length)?;
            outcome.pattern = Some(pattern);
        }
        EditCommand::ClonePattern { pattern } => {
            let clone = project.clone_pattern(*pattern)?;
            outcome.pattern = Some(clone);
        }
        EditCommand::RemovePattern { pattern } => {
            let index = project
                .pattern_index(*pattern)
                .ok_or(ModelError::PatternNotFound(*pattern))?;
            project.patterns.remove(index);
            for track in &mut project.tracks {
                track.clips.retain(|clip| clip.pattern != *pattern);
            }
            outcome.pattern = Some(*pattern);
        }
        EditCommand::SetPatternPreset { pattern, preset } => {
            validate_preset(*preset)?;
            let stored = project
                .pattern_mut(*pattern)
                .ok_or(ModelError::PatternNotFound(*pattern))?;
            stored.preset = *preset;
            outcome.pattern = Some(*pattern);
        }
        EditCommand::AddClip {
            track,
            pattern,
            start,
        } => {
            let clip = project.add_clip(*track, *pattern, *start)?;
            outcome.clip = Some(clip);
            outcome.pattern = Some(*pattern);
        }
        EditCommand::RemoveClip { track, clip } => {
            let (track_index, clip_index) = locate_clip(project, *track, *clip)?;
            project.tracks[track_index].clips.remove(clip_index);
            outcome.clip = Some(*clip);
        }
        EditCommand::MoveClip { track, clip, start } => {
            let (track_index, clip_index) = locate_clip(project, *track, *clip)?;
            project.tracks[track_index].clips[clip_index].start = *start;
            outcome.clip = Some(*clip);
        }
        EditCommand::ResizeClip {
            track,
            clip,
            length,
        } => {
            if *length == Ticks::ZERO {
                return Err(ModelError::InvalidClipLength);
            }
            let (track_index, clip_index) = locate_clip(project, *track, *clip)?;
            project.tracks[track_index].clips[clip_index].length = *length;
            outcome.clip = Some(*clip);
        }
        EditCommand::AddNote {
            pattern,
            key,
            velocity,
            start,
            length,
        } => {
            let note = project.add_note(*pattern, *key, *velocity, *start, *length)?;
            outcome.note = Some(note);
            outcome.pattern = Some(*pattern);
        }
        EditCommand::RemoveNotes { pattern, notes } => {
            let index = project
                .pattern_index(*pattern)
                .ok_or(ModelError::PatternNotFound(*pattern))?;
            let stored = &project.patterns[index];
            let unique: HashSet<NoteId> = notes.iter().copied().collect();
            for note in &unique {
                if stored.note(*note).is_none() {
                    return Err(ModelError::NoteNotFound(*note));
                }
            }
            if unique.is_empty() {
                return Ok(outcome);
            }

            let before = stored.notes.len();
            project.patterns[index]
                .notes
                .retain(|note| !unique.contains(&note.id));
            outcome.removed_notes = before - project.patterns[index].notes.len();
            outcome.pattern = Some(*pattern);
        }
        EditCommand::MoveNotes {
            pattern,
            notes,
            delta_ticks,
            delta_key,
        } => {
            let index = project
                .pattern_index(*pattern)
                .ok_or(ModelError::PatternNotFound(*pattern))?;
            let stored = &project.patterns[index];
            let indices = unique_note_indices(stored, notes)?;
            if indices.is_empty() {
                return Ok(outcome);
            }

            for note_index in &indices {
                let note = &stored.notes[*note_index];
                let new_start = note.start.0 as i128 + i128::from(*delta_ticks);
                let new_key = i128::from(note.key) + i128::from(*delta_key);
                if new_start < 0 || new_start > u64::MAX as i128 {
                    return Err(ModelError::NoteOutsidePattern);
                }
                if !(0..=127).contains(&new_key) {
                    return Err(ModelError::InvalidKey(new_key as i32));
                }
                validate_note_geometry(Ticks(new_start as u64), note.length, stored.length)?;
            }

            let stored = &mut project.patterns[index];
            for note_index in indices {
                let note = &mut stored.notes[note_index];
                note.start = Ticks((note.start.0 as i128 + i128::from(*delta_ticks)) as u64);
                note.key = (i128::from(note.key) + i128::from(*delta_key)) as u8;
            }
            outcome.pattern = Some(*pattern);
        }
        EditCommand::ResizeNote {
            pattern,
            note,
            start,
            length,
        } => {
            let index = project
                .pattern_index(*pattern)
                .ok_or(ModelError::PatternNotFound(*pattern))?;
            let stored = &project.patterns[index];
            let note_index = stored
                .note_index(*note)
                .ok_or(ModelError::NoteNotFound(*note))?;
            validate_note_geometry(*start, *length, stored.length)?;

            let stored = &mut project.patterns[index];
            stored.notes[note_index].start = *start;
            stored.notes[note_index].length = *length;
            outcome.note = Some(*note);
            outcome.pattern = Some(*pattern);
        }
        EditCommand::SetNoteVelocity {
            pattern,
            note,
            velocity,
        } => {
            validate_velocity(*velocity)?;
            let index = project
                .pattern_index(*pattern)
                .ok_or(ModelError::PatternNotFound(*pattern))?;
            let stored = &mut project.patterns[index];
            let note_index = stored
                .note_index(*note)
                .ok_or(ModelError::NoteNotFound(*note))?;
            stored.notes[note_index].velocity = *velocity;
            outcome.note = Some(*note);
            outcome.pattern = Some(*pattern);
        }
        EditCommand::SetTempo { tick, bpm } => {
            project.tempo_map.insert_tempo(*tick, *bpm)?;
        }
        EditCommand::SetTimeSignature { time_signature } => {
            if !time_signature.is_valid() {
                return Err(ModelError::InvalidTimeSignature);
            }
            project.time_signature = *time_signature;
        }
    }

    Ok(outcome)
}

fn validate_track_name(name: &str) -> Result<(), ModelError> {
    if name.trim().is_empty() {
        Err(ModelError::EmptyTrackName)
    } else {
        Ok(())
    }
}

fn validate_pattern_name(name: &str) -> Result<(), ModelError> {
    if name.trim().is_empty() {
        Err(ModelError::EmptyPatternName)
    } else {
        Ok(())
    }
}

fn locate_clip(
    project: &Project,
    track: TrackId,
    clip: ClipId,
) -> Result<(usize, usize), ModelError> {
    let track_index = project
        .track_index(track)
        .ok_or(ModelError::TrackNotFound(track))?;
    let clip_index = project.tracks[track_index]
        .clip_index(clip)
        .ok_or(ModelError::ClipNotFound(clip))?;
    Ok((track_index, clip_index))
}

fn unique_note_indices(
    pattern: &crate::model::Pattern,
    notes: &[NoteId],
) -> Result<Vec<usize>, ModelError> {
    let unique: HashSet<NoteId> = notes.iter().copied().collect();
    let mut indices = Vec::with_capacity(unique.len());
    for note in unique {
        let index = pattern
            .note_index(note)
            .ok_or(ModelError::NoteNotFound(note))?;
        indices.push(index);
    }
    indices.sort_unstable();
    Ok(indices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Note, Project};
    use crate::store::ProjectStore;

    fn fixture() -> (ProjectStore, TrackId, PatternId, ClipId) {
        let mut project = Project::new("Test");
        let track = project.add_track("Piano").unwrap();
        let pattern = project.add_pattern("Verse", Ticks(3840)).unwrap();
        let clip = project.add_clip(track, pattern, Ticks::ZERO).unwrap();
        (ProjectStore::new(project), track, pattern, clip)
    }

    #[test]
    fn gain_is_capped_at_unity() {
        let (mut store, track, _pattern, _clip) = fixture();

        assert!(
            store
                .apply(EditCommand::SetTrackParams {
                    track,
                    gain: Some(1.5),
                    pan: None,
                    mute: None,
                    solo: None,
                    preset: None,
                })
                .is_err()
        );
        assert!(
            store
                .apply(EditCommand::SetTrackParams {
                    track,
                    gain: Some(1.0),
                    pan: None,
                    mute: None,
                    solo: None,
                    preset: None,
                })
                .is_ok()
        );
        assert_eq!(store.project().track(track).unwrap().gain, 1.0);
    }

    #[test]
    fn out_of_range_key_reports_the_requested_value() {
        let (mut store, _track, pattern, _clip) = fixture();
        let note = store
            .apply(EditCommand::AddNote {
                pattern,
                key: 60,
                velocity: 100,
                start: Ticks(0),
                length: Ticks(480),
            })
            .unwrap()
            .note
            .unwrap();

        let error = store
            .apply(EditCommand::MoveNotes {
                pattern,
                notes: vec![note],
                delta_ticks: 0,
                delta_key: -120,
            })
            .unwrap_err();

        assert_eq!(error, ModelError::InvalidKey(-60));
        assert!(error.to_string().contains("-60"));
    }

    #[test]
    fn resize_clip_validates_length_and_supports_undo() {
        let (mut store, track, _pattern, clip) = fixture();

        store
            .apply(EditCommand::ResizeClip {
                track,
                clip,
                length: Ticks(7680),
            })
            .unwrap();
        let length = store
            .project()
            .track(track)
            .unwrap()
            .clip(clip)
            .unwrap()
            .length;
        assert_eq!(length, Ticks(7680));

        assert!(
            store
                .apply(EditCommand::ResizeClip {
                    track,
                    clip,
                    length: Ticks::ZERO,
                })
                .is_err()
        );

        store.undo();
        let length = store
            .project()
            .track(track)
            .unwrap()
            .clip(clip)
            .unwrap()
            .length;
        assert_eq!(length, Ticks(3840));
    }

    #[test]
    fn move_track_reorders_and_clamps() {
        let mut project = Project::new("Test");
        let first = project.add_track("First").unwrap();
        let second = project.add_track("Second").unwrap();
        let third = project.add_track("Third").unwrap();
        let mut store = ProjectStore::new(project);

        store
            .apply(EditCommand::MoveTrack {
                track: first,
                to_index: 99,
            })
            .unwrap();
        let order = store
            .project()
            .tracks
            .iter()
            .map(|track| track.id)
            .collect::<Vec<_>>();
        assert_eq!(order, vec![second, third, first]);

        store.undo();
        let order = store
            .project()
            .tracks
            .iter()
            .map(|track| track.id)
            .collect::<Vec<_>>();
        assert_eq!(order, vec![first, second, third]);

        assert!(
            store
                .apply(EditCommand::MoveTrack {
                    track: first,
                    to_index: 1,
                })
                .is_ok()
        );
    }

    #[test]
    fn add_note_returns_allocated_id() {
        let (mut store, _track, pattern, _clip) = fixture();
        let outcome = store
            .apply(EditCommand::AddNote {
                pattern,
                key: 60,
                velocity: 100,
                start: Ticks(120),
                length: Ticks(480),
            })
            .unwrap();

        let note = outcome.note.unwrap();
        assert_eq!(outcome.pattern, Some(pattern));
        let stored = store.project().pattern(pattern).unwrap();
        assert!(stored.note(note).is_some());
    }

    #[test]
    fn move_notes_updates_all_targets_atomically() {
        let (mut store, _track, pattern, _clip) = fixture();
        let first = store
            .apply(EditCommand::AddNote {
                pattern,
                key: 60,
                velocity: 100,
                start: Ticks(0),
                length: Ticks(480),
            })
            .unwrap()
            .note
            .unwrap();
        let second = store
            .apply(EditCommand::AddNote {
                pattern,
                key: 64,
                velocity: 100,
                start: Ticks(960),
                length: Ticks(480),
            })
            .unwrap()
            .note
            .unwrap();

        store
            .apply(EditCommand::MoveNotes {
                pattern,
                notes: vec![first, second],
                delta_ticks: 240,
                delta_key: 2,
            })
            .unwrap();

        let stored = store.project().pattern(pattern).unwrap();
        assert_eq!(stored.note(first).unwrap().start, Ticks(240));
        assert_eq!(stored.note(first).unwrap().key, 62);
        assert_eq!(stored.note(second).unwrap().start, Ticks(1200));
        assert_eq!(stored.note(second).unwrap().key, 66);
    }

    #[test]
    fn failed_multi_note_move_leaves_notes_untouched() {
        let (mut store, _track, pattern, _clip) = fixture();
        let first = store
            .apply(EditCommand::AddNote {
                pattern,
                key: 60,
                velocity: 100,
                start: Ticks(0),
                length: Ticks(480),
            })
            .unwrap()
            .note
            .unwrap();
        let second = store
            .apply(EditCommand::AddNote {
                pattern,
                key: 64,
                velocity: 100,
                start: Ticks(960),
                length: Ticks(480),
            })
            .unwrap()
            .note
            .unwrap();
        let revision = store.revision();

        let result = store.apply(EditCommand::MoveNotes {
            pattern,
            notes: vec![first, second],
            delta_ticks: 0,
            delta_key: -120,
        });

        assert!(result.is_err());
        assert_eq!(store.revision(), revision);
        let stored = store.project().pattern(pattern).unwrap();
        assert_eq!(stored.note(first).unwrap().key, 60);
        assert_eq!(stored.note(second).unwrap().key, 64);
    }

    #[test]
    fn remove_notes_deduplicates_ids() {
        let (mut store, _track, pattern, _clip) = fixture();
        let note = store
            .apply(EditCommand::AddNote {
                pattern,
                key: 60,
                velocity: 100,
                start: Ticks(0),
                length: Ticks(480),
            })
            .unwrap()
            .note
            .unwrap();

        let outcome = store
            .apply(EditCommand::RemoveNotes {
                pattern,
                notes: vec![note, note],
            })
            .unwrap();

        assert_eq!(outcome.removed_notes, 1);
        assert!(store.project().pattern(pattern).unwrap().notes.is_empty());
    }

    #[test]
    fn resize_and_velocity_are_validated() {
        let (mut store, _track, pattern, _clip) = fixture();
        let note = store
            .apply(EditCommand::AddNote {
                pattern,
                key: 60,
                velocity: 100,
                start: Ticks(0),
                length: Ticks(480),
            })
            .unwrap()
            .note
            .unwrap();

        assert!(
            store
                .apply(EditCommand::ResizeNote {
                    pattern,
                    note,
                    start: Ticks(0),
                    length: Ticks::ZERO,
                })
                .is_err()
        );
        assert!(
            store
                .apply(EditCommand::SetNoteVelocity {
                    pattern,
                    note,
                    velocity: 0,
                })
                .is_err()
        );

        let note_ref: &Note = store
            .project()
            .pattern(pattern)
            .unwrap()
            .note(note)
            .unwrap();
        assert_eq!(note_ref.length, Ticks(480));
        assert_eq!(note_ref.velocity, 100);
    }

    #[test]
    fn removing_a_pattern_removes_its_placements() {
        let (mut store, track, pattern, clip) = fixture();
        let other = store
            .apply(EditCommand::AddPattern {
                name: "Chorus".into(),
                length: Ticks(1920),
            })
            .unwrap()
            .pattern
            .unwrap();
        let other_clip = store
            .apply(EditCommand::AddClip {
                track,
                pattern: other,
                start: Ticks::ZERO,
            })
            .unwrap()
            .clip
            .unwrap();

        store.apply(EditCommand::RemovePattern { pattern }).unwrap();

        let stored = store.project().track(track).unwrap();
        assert!(stored.clip(clip).is_none());
        assert!(stored.clip(other_clip).is_some());
        assert!(store.project().pattern(pattern).is_none());
        assert!(store.project().pattern(other).is_some());

        store.undo();
        assert!(store.project().pattern(pattern).is_some());
        assert!(store.project().track(track).unwrap().clip(clip).is_some());
    }

    #[test]
    fn cloned_patterns_are_independent() {
        let (mut store, _track, pattern, _clip) = fixture();
        store
            .apply(EditCommand::AddNote {
                pattern,
                key: 60,
                velocity: 100,
                start: Ticks(0),
                length: Ticks(480),
            })
            .unwrap();

        let clone = store
            .apply(EditCommand::ClonePattern { pattern })
            .unwrap()
            .pattern
            .unwrap();
        assert_ne!(clone, pattern);

        store
            .apply(EditCommand::AddNote {
                pattern: clone,
                key: 64,
                velocity: 100,
                start: Ticks(480),
                length: Ticks(480),
            })
            .unwrap();

        assert_eq!(store.project().pattern(pattern).unwrap().notes.len(), 1);
        assert_eq!(store.project().pattern(clone).unwrap().notes.len(), 2);
    }

    #[test]
    fn pattern_name_must_not_be_empty() {
        let (mut store, _track, _pattern, _clip) = fixture();
        assert!(
            store
                .apply(EditCommand::AddPattern {
                    name: "   ".into(),
                    length: Ticks(1920),
                })
                .is_err()
        );
        assert!(
            store
                .apply(EditCommand::AddPattern {
                    name: "Bass".into(),
                    length: Ticks::ZERO,
                })
                .is_err()
        );
    }
}
