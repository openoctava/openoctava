use std::sync::Arc;

use crate::model::{Clip, ClipId, Note, NoteId, Pattern, Preset, Project, Track, TrackId};
use crate::sample::{SampleCache, SampleData};
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
    /// Decoded audio samples sorted by start tick.
    pub samples: Vec<TimedSample>,
}

/// A decoded audio clip scheduled on the arrangement.
#[derive(Clone, Debug, PartialEq)]
pub struct TimedSample {
    pub start: Ticks,
    pub track: TrackId,
    /// Combined track and clip gain.
    pub gain: f32,
    pub pan: f32,
    pub data: Arc<SampleData>,
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
    /// Preset of the pattern this note belongs to, so the synthesizer can
    /// switch sounds per pattern instead of per track.
    pub preset: Preset,
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
        self.compile_snapshot_with(&mut SampleCache::default())
    }

    /// Compiles a playback snapshot, decoding sample files through `cache`.
    ///
    /// The application keeps a cache alive between edits so that a WAV is only
    /// read when it is first used.
    #[must_use]
    pub fn compile_snapshot_with(&self, cache: &mut SampleCache) -> PlaybackSnapshot {
        let any_solo = self.any_solo();
        let mut tracks = Vec::new();
        let mut events = Vec::with_capacity(self.note_count() * 2);
        let mut samples = Vec::new();

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
                let Some(pattern) = self.pattern(clip.pattern) else {
                    continue;
                };
                push_clip_events(track, clip, pattern, &mut events);
            }

            for sample in &track.samples {
                let Some(data) = cache.load(&sample.path) else {
                    continue;
                };
                samples.push(TimedSample {
                    start: sample.start,
                    track: track.id,
                    gain: (track.gain * sample.gain).clamp(0.0, 1.0),
                    pan: track.pan,
                    data,
                });
            }
        }

        samples.sort_by(|a, b| {
            a.start
                .cmp(&b.start)
                .then_with(|| a.track.0.cmp(&b.track.0))
        });

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
            samples,
        }
    }
}

fn push_clip_events(track: &Track, clip: &Clip, pattern: &Pattern, events: &mut Vec<TimedEvent>) {
    // A placement may be trimmed shorter than the pattern it plays.
    let limit = clip.length.min(pattern.length);
    for note in &pattern.notes {
        push_note_events(track, clip, pattern, note, limit, events);
    }
}

fn push_note_events(
    track: &Track,
    clip: &Clip,
    pattern: &Pattern,
    note: &Note,
    limit: Ticks,
    events: &mut Vec<TimedEvent>,
) {
    if note.length == Ticks::ZERO || note.start >= limit {
        return;
    }

    let clipped_length = note.start.saturating_add(note.length).min(limit);
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
        preset: pattern.preset,
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
        preset: pattern.preset,
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
        let pattern = project.add_pattern("Verse", Ticks(1920)).unwrap();
        project.add_clip(track, pattern, Ticks::ZERO).unwrap();

        project
            .add_note(pattern, 60, 100, Ticks::ZERO, Ticks(480))
            .unwrap();
        project
            .add_note(pattern, 65, 100, Ticks(480), Ticks(960))
            .unwrap();
        project
            .add_note(pattern, 62, 100, Ticks(960), Ticks(1920))
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
        let piano_pattern = project.add_pattern("Piano part", Ticks(960)).unwrap();
        let bass_pattern = project.add_pattern("Bass part", Ticks(960)).unwrap();
        project.add_clip(piano, piano_pattern, Ticks::ZERO).unwrap();
        project.add_clip(bass, bass_pattern, Ticks::ZERO).unwrap();
        project
            .add_note(piano_pattern, 60, 100, Ticks::ZERO, Ticks(480))
            .unwrap();
        project
            .add_note(bass_pattern, 36, 100, Ticks::ZERO, Ticks(480))
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
        let pattern = project.add_pattern("Verse", Ticks(1920)).unwrap();
        project.add_clip(track, pattern, Ticks(960)).unwrap();
        project
            .add_note(pattern, 60, 100, Ticks(120), Ticks(240))
            .unwrap();

        let snapshot = project.compile_snapshot();
        assert_eq!(snapshot.events[0].tick, Ticks(1080));
        assert_eq!(snapshot.events[1].tick, Ticks(1320));
    }

    #[test]
    fn trimmed_placement_clips_its_notes() {
        let mut project = Project::new("Test");
        let track = project.add_track("Piano").unwrap();
        let pattern = project.add_pattern("Verse", Ticks(1920)).unwrap();
        let clip = project.add_clip(track, pattern, Ticks::ZERO).unwrap();
        project
            .add_note(pattern, 60, 100, Ticks(960), Ticks(960))
            .unwrap();

        project
            .apply(&crate::command::EditCommand::ResizeClip {
                track,
                clip,
                length: Ticks(1200),
            })
            .unwrap();

        let snapshot = project.compile_snapshot();
        assert_eq!(snapshot.events[0].tick, Ticks(960));
        assert_eq!(snapshot.events[1].tick, Ticks(1200));
        assert_eq!(snapshot.length, Ticks(1200));
    }

    #[test]
    fn sample_clips_are_decoded_and_scheduled() {
        let path =
            std::env::temp_dir().join(format!("octava-snapshot-sample-{}.wav", std::process::id()));
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for _ in 0..24_000 {
            writer.write_sample(16_384_i16).unwrap();
        }
        writer.finalize().unwrap();

        let mut project = Project::new("Samples");
        let track = project.add_track("Audio").unwrap();
        let sample = project
            .add_sample(track, "hit", path.clone(), Ticks(960), 0.5, 0.8)
            .unwrap();
        assert_eq!(project.length_ticks(), Ticks(1920));
        assert_eq!(
            project.sample_length_ticks(project.track(track).unwrap().sample(sample).unwrap()),
            Ticks(960)
        );

        let snapshot = project.compile_snapshot();
        assert_eq!(snapshot.samples.len(), 1);
        assert_eq!(snapshot.samples[0].start, Ticks(960));
        assert_eq!(snapshot.samples[0].track, track);
        assert!((snapshot.samples[0].gain - 0.8).abs() < 1.0e-6);
        assert_eq!(snapshot.samples[0].data.frame_count(), 24_000);

        project.track_mut(track).unwrap().mute = true;
        assert!(project.compile_snapshot().samples.is_empty());

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn every_placement_of_a_pattern_sounds() {
        let mut project = Project::new("Test");
        let track = project.add_track("Piano").unwrap();
        let pattern = project.add_pattern("Hook", Ticks(960)).unwrap();
        project.add_clip(track, pattern, Ticks::ZERO).unwrap();
        project.add_clip(track, pattern, Ticks(1920)).unwrap();
        project
            .add_note(pattern, 60, 100, Ticks::ZERO, Ticks(480))
            .unwrap();

        let snapshot = project.compile_snapshot();
        let onsets: Vec<u64> = snapshot
            .events
            .iter()
            .filter(|event| matches!(event.kind, EventKind::NoteOn { .. }))
            .map(|event| event.tick.0)
            .collect();
        assert_eq!(onsets, vec![0, 1920]);
        assert_eq!(snapshot.length, Ticks(2880));
    }
}
