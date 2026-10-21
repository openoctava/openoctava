use crate::model::{ClipId, MidiClip, Note, NoteId, Preset, Project, Track, TrackId};
use crate::time::{TempoMap, Ticks};

/// Immutable, playback-ready view of a project.
///
/// The audio thread only consumes this structure. It contains audible tracks and
/// note events sorted by tick, with note-offs ordered before note-ons at the same tick.
#[derive(Clone, Debug, PartialEq)]
pub struct PlaybackSnapshot {
    pub ppq: u16,
    pub tempo_map: TempoMap,
    pub length: Ticks,
    pub tracks: Vec<SnapshotTrack>,
    pub events: Vec<TimedEvent>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapshotTrack {
    pub id: TrackId,
    pub channel: u8,
    pub preset: Preset,
    pub gain: f32,
    pub pan: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimedEvent {
    pub tick: Ticks,
    pub track: TrackId,
    pub channel: u8,
    pub clip: ClipId,
    pub note: NoteId,
    pub kind: EventKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    NoteOn { key: u8, velocity: u8 },
    NoteOff { key: u8 },
}

impl EventKind {
    #[must_use]
    pub const fn key(self) -> u8 {
        match self {
            Self::NoteOn { key, .. } | Self::NoteOff { key } => key,
        }
    }

    const fn order(self) -> u8 {
        match self {
            Self::NoteOff { .. } => 0,
            Self::NoteOn { .. } => 1,
        }
    }
}

impl Project {
    #[must_use]
    pub fn compile_snapshot(&self) -> PlaybackSnapshot {
        let any_solo = self.any_solo();
        let mut tracks = Vec::new();
        let mut events = Vec::with_capacity(self.note_count() * 2);

        for track in &self.tracks {
            if !track.is_audible(any_solo) {
                continue;
            }

            tracks.push(SnapshotTrack {
                id: track.id,
                channel: track.channel,
                preset: track.preset,
                gain: track.gain,
                pan: track.pan,
            });

            for clip in &track.clips {
                push_clip_events(track, clip, &mut events);
            }
        }

        events.sort_by(|a, b| {
            a.tick
                .cmp(&b.tick)
                .then_with(|| a.kind.order().cmp(&b.kind.order()))
                .then_with(|| a.channel.cmp(&b.channel))
                .then_with(|| a.kind.key().cmp(&b.kind.key()))
                .then_with(|| a.track.0.cmp(&b.track.0))
                .then_with(|| a.clip.0.cmp(&b.clip.0))
                .then_with(|| a.note.0.cmp(&b.note.0))
        });

        PlaybackSnapshot {
            ppq: self.tempo_map.ppq(),
            tempo_map: self.tempo_map.clone(),
            length: self.length_ticks(),
            tracks,
            events,
        }
    }
}

fn push_clip_events(track: &Track, clip: &MidiClip, events: &mut Vec<TimedEvent>) {
    for note in &clip.notes {
        push_note_events(track, clip, note, events);
    }
}

fn push_note_events(track: &Track, clip: &MidiClip, note: &Note, events: &mut Vec<TimedEvent>) {
    if note.length == Ticks::ZERO || note.start >= clip.length {
        return;
    }

    let clipped_length = note.start.saturating_add(note.length).min(clip.length);
    let length = clipped_length.saturating_sub(note.start);
    if length == Ticks::ZERO {
        return;
    }

    let start = clip.start.saturating_add(note.start);
    let end = start.saturating_add(length);

    events.push(TimedEvent {
        tick: start,
        track: track.id,
        channel: track.channel,
        clip: clip.id,
        note: note.id,
        kind: EventKind::NoteOn {
            key: note.key,
            velocity: note.velocity,
        },
    });
    events.push(TimedEvent {
        tick: end,
        track: track.id,
        channel: track.channel,
        clip: clip.id,
        note: note.id,
        kind: EventKind::NoteOff { key: note.key },
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Project;

    #[test]
    fn events_are_sorted_and_clipped_to_clip_end() {
        let mut project = Project::new("Test");
        let track = project.add_track("Piano").unwrap();
        let clip = project.add_clip(track, Ticks::ZERO, Ticks(1920)).unwrap();

        project
            .add_note(track, clip, 60, 100, Ticks::ZERO, Ticks(480))
            .unwrap();
        project
            .add_note(track, clip, 65, 100, Ticks(480), Ticks(960))
            .unwrap();
        project
            .add_note(track, clip, 62, 100, Ticks(960), Ticks(1920))
            .unwrap();

        let snapshot = project.compile_snapshot();
        let summary: Vec<(u64, EventKind)> = snapshot
            .events
            .iter()
            .map(|event| (event.tick.0, event.kind))
            .collect();

        assert_eq!(
            summary,
            vec![
                (
                    0,
                    EventKind::NoteOn {
                        key: 60,
                        velocity: 100
                    }
                ),
                (480, EventKind::NoteOff { key: 60 }),
                (
                    480,
                    EventKind::NoteOn {
                        key: 65,
                        velocity: 100
                    }
                ),
                (
                    960,
                    EventKind::NoteOn {
                        key: 62,
                        velocity: 100
                    }
                ),
                (1440, EventKind::NoteOff { key: 65 }),
                (1920, EventKind::NoteOff { key: 62 }),
            ]
        );
        assert_eq!(snapshot.length, Ticks(1920));
    }

    #[test]
    fn mute_and_solo_filter_the_snapshot() {
        let mut project = Project::new("Test");
        let piano = project.add_track("Piano").unwrap();
        let bass = project.add_track("Bass").unwrap();
        let piano_clip = project.add_clip(piano, Ticks::ZERO, Ticks(960)).unwrap();
        let bass_clip = project.add_clip(bass, Ticks::ZERO, Ticks(960)).unwrap();
        project
            .add_note(piano, piano_clip, 60, 100, Ticks::ZERO, Ticks(480))
            .unwrap();
        project
            .add_note(bass, bass_clip, 36, 100, Ticks::ZERO, Ticks(480))
            .unwrap();

        project.track_mut(bass).unwrap().mute = true;
        let snapshot = project.compile_snapshot();
        assert_eq!(snapshot.tracks.len(), 1);
        assert_eq!(snapshot.tracks[0].id, piano);
        assert!(snapshot.events.iter().all(|event| event.track == piano));

        project.track_mut(piano).unwrap().solo = true;
        let snapshot = project.compile_snapshot();
        assert_eq!(snapshot.tracks.len(), 1);
        assert_eq!(snapshot.tracks[0].id, piano);

        project.track_mut(piano).unwrap().solo = false;
        project.track_mut(piano).unwrap().mute = true;
        project.track_mut(bass).unwrap().mute = false;
        project.track_mut(bass).unwrap().solo = true;
        let snapshot = project.compile_snapshot();
        assert_eq!(snapshot.tracks.len(), 1);
        assert_eq!(snapshot.tracks[0].id, bass);
        assert!(snapshot.events.iter().all(|event| event.track == bass));
    }

    #[test]
    fn clip_start_offsets_events() {
        let mut project = Project::new("Test");
        let track = project.add_track("Piano").unwrap();
        let clip = project.add_clip(track, Ticks(960), Ticks(1920)).unwrap();
        project
            .add_note(track, clip, 60, 100, Ticks(120), Ticks(240))
            .unwrap();

        let snapshot = project.compile_snapshot();
        assert_eq!(snapshot.events[0].tick, Ticks(1080));
        assert_eq!(snapshot.events[1].tick, Ticks(1320));
    }
}
