use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::model::{
    ClipId, ModelError, NoteId, Preset, Project, TrackId, validate_note_geometry, validate_preset,
    validate_velocity,
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
    AddClip {
        track: TrackId,
        start: Ticks,
        length: Ticks,
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
        track: TrackId,
        clip: ClipId,
        key: u8,
        velocity: u8,
        start: Ticks,
        length: Ticks,
    },
    RemoveNotes {
        track: TrackId,
        clip: ClipId,
        notes: Vec<NoteId>,
    },
    MoveNotes {
        track: TrackId,
        clip: ClipId,
        notes: Vec<NoteId>,
        delta_ticks: i64,
        delta_key: i32,
    },
    ResizeNote {
        track: TrackId,
        clip: ClipId,
        note: NoteId,
        start: Ticks,
        length: Ticks,
    },
    SetNoteVelocity {
        track: TrackId,
        clip: ClipId,
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
                && (!gain.is_finite() || *gain < 0.0)
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
        EditCommand::AddClip {
            track,
            start,
            length,
        } => {
            let clip = project.add_clip(*track, *start, *length)?;
            outcome.clip = Some(clip);
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
            track,
            clip,
            key,
            velocity,
            start,
            length,
        } => {
            let note = project.add_note(*track, *clip, *key, *velocity, *start, *length)?;
            outcome.note = Some(note);
        }
        EditCommand::RemoveNotes { track, clip, notes } => {
            let (track_index, clip_index) = locate_clip(project, *track, *clip)?;
            let stored = &project.tracks[track_index].clips[clip_index];
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
            project.tracks[track_index].clips[clip_index]
                .notes
                .retain(|note| !unique.contains(&note.id));
            outcome.removed_notes =
                before - project.tracks[track_index].clips[clip_index].notes.len();
        }
        EditCommand::MoveNotes {
            track,
            clip,
            notes,
            delta_ticks,
            delta_key,
        } => {
            let (track_index, clip_index) = locate_clip(project, *track, *clip)?;
            let stored = &project.tracks[track_index].clips[clip_index];
            let indices = unique_note_indices(stored, notes)?;
            if indices.is_empty() {
                return Ok(outcome);
            }

            for index in &indices {
                let note = &stored.notes[*index];
                let new_start = note.start.0 as i128 + i128::from(*delta_ticks);
                let new_key = i128::from(note.key) + i128::from(*delta_key);
                if new_start < 0 || new_start > u64::MAX as i128 {
                    return Err(ModelError::NoteOutsideClip);
                }
                if !(0..=127).contains(&new_key) {
                    return Err(ModelError::InvalidKey(new_key as u8));
                }
                validate_note_geometry(Ticks(new_start as u64), note.length, stored.length)?;
            }

            let stored = &mut project.tracks[track_index].clips[clip_index];
            for index in indices {
                let note = &mut stored.notes[index];
                note.start = Ticks((note.start.0 as i128 + i128::from(*delta_ticks)) as u64);
                note.key = (i128::from(note.key) + i128::from(*delta_key)) as u8;
            }
        }
        EditCommand::ResizeNote {
            track,
            clip,
            note,
            start,
            length,
        } => {
            let (track_index, clip_index) = locate_clip(project, *track, *clip)?;
            let stored = &project.tracks[track_index].clips[clip_index];
            let note_index = stored
                .note_index(*note)
                .ok_or(ModelError::NoteNotFound(*note))?;
            validate_note_geometry(*start, *length, stored.length)?;

            let stored = &mut project.tracks[track_index].clips[clip_index];
            stored.notes[note_index].start = *start;
            stored.notes[note_index].length = *length;
            outcome.note = Some(*note);
        }
        EditCommand::SetNoteVelocity {
            track,
            clip,
            note,
            velocity,
        } => {
            validate_velocity(*velocity)?;
            let (track_index, clip_index) = locate_clip(project, *track, *clip)?;
            let stored = &mut project.tracks[track_index].clips[clip_index];
            let note_index = stored
                .note_index(*note)
                .ok_or(ModelError::NoteNotFound(*note))?;
            stored.notes[note_index].velocity = *velocity;
            outcome.note = Some(*note);
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
    clip: &crate::model::MidiClip,
    notes: &[NoteId],
) -> Result<Vec<usize>, ModelError> {
    let unique: HashSet<NoteId> = notes.iter().copied().collect();
    let mut indices = Vec::with_capacity(unique.len());
    for note in unique {
        let index = clip
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

    fn fixture() -> (ProjectStore, TrackId, ClipId) {
        let mut project = Project::new("Test");
        let track = project.add_track("Piano").unwrap();
        let clip = project.add_clip(track, Ticks::ZERO, Ticks(3840)).unwrap();
        (ProjectStore::new(project), track, clip)
    }

    #[test]
    fn resize_clip_validates_length_and_supports_undo() {
        let (mut store, track, clip) = fixture();

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
        let (mut store, track, clip) = fixture();
        let outcome = store
            .apply(EditCommand::AddNote {
                track,
                clip,
                key: 60,
                velocity: 100,
                start: Ticks(120),
                length: Ticks(480),
            })
            .unwrap();

        let note = outcome.note.unwrap();
        let stored = store.project().track(track).unwrap().clip(clip).unwrap();
        assert!(stored.note(note).is_some());
    }

    #[test]
    fn move_notes_updates_all_targets_atomically() {
        let (mut store, track, clip) = fixture();
        let first = store
            .apply(EditCommand::AddNote {
                track,
                clip,
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
                track,
                clip,
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
                track,
                clip,
                notes: vec![first, second],
                delta_ticks: 240,
                delta_key: 2,
            })
            .unwrap();

        let stored = store.project().track(track).unwrap().clip(clip).unwrap();
        assert_eq!(stored.note(first).unwrap().start, Ticks(240));
        assert_eq!(stored.note(first).unwrap().key, 62);
        assert_eq!(stored.note(second).unwrap().start, Ticks(1200));
        assert_eq!(stored.note(second).unwrap().key, 66);
    }

    #[test]
    fn failed_multi_note_move_leaves_notes_untouched() {
        let (mut store, track, clip) = fixture();
        let first = store
            .apply(EditCommand::AddNote {
                track,
                clip,
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
                track,
                clip,
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
            track,
            clip,
            notes: vec![first, second],
            delta_ticks: 0,
            delta_key: -120,
        });

        assert!(result.is_err());
        assert_eq!(store.revision(), revision);
        let stored = store.project().track(track).unwrap().clip(clip).unwrap();
        assert_eq!(stored.note(first).unwrap().key, 60);
        assert_eq!(stored.note(second).unwrap().key, 64);
    }

    #[test]
    fn remove_notes_deduplicates_ids() {
        let (mut store, track, clip) = fixture();
        let note = store
            .apply(EditCommand::AddNote {
                track,
                clip,
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
                track,
                clip,
                notes: vec![note, note],
            })
            .unwrap();

        assert_eq!(outcome.removed_notes, 1);
        assert!(
            store
                .project()
                .track(track)
                .unwrap()
                .clip(clip)
                .unwrap()
                .notes
                .is_empty()
        );
    }

    #[test]
    fn resize_and_velocity_are_validated() {
        let (mut store, track, clip) = fixture();
        let note = store
            .apply(EditCommand::AddNote {
                track,
                clip,
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
                    track,
                    clip,
                    note,
                    start: Ticks(0),
                    length: Ticks::ZERO,
                })
                .is_err()
        );
        assert!(
            store
                .apply(EditCommand::SetNoteVelocity {
                    track,
                    clip,
                    note,
                    velocity: 0,
                })
                .is_err()
        );

        let note_ref: &Note = store
            .project()
            .track(track)
            .unwrap()
            .clip(clip)
            .unwrap()
            .note(note)
            .unwrap();
        assert_eq!(note_ref.length, Ticks(480));
        assert_eq!(note_ref.velocity, 100);
    }
}
