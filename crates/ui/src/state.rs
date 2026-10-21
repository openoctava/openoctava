use egui::Pos2;
use octava_core::{ClipId, EditCommand, NoteId, Project, Ticks, Track, TrackId};

/// Read-only transport state supplied by the application layer.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlaybackView {
    pub playing: bool,
    pub playhead: Ticks,
    pub peak_left: f32,
    pub peak_right: f32,
    pub audio_sample_rate: Option<u32>,
}

/// Everything the UI wants the application layer to do.
#[derive(Clone, Debug, PartialEq)]
pub enum UiAction {
    Play,
    Pause,
    Stop,
    Seek(Ticks),
    Edit(EditCommand),
    Undo,
    Redo,
    NewProject,
    ExportWav,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SnapDivision {
    Whole,
    Half,
    #[default]
    Quarter,
    Eighth,
    Sixteenth,
    ThirtySecond,
    SixtyFourth,
}

impl SnapDivision {
    pub const ALL: [Self; 7] = [
        Self::Whole,
        Self::Half,
        Self::Quarter,
        Self::Eighth,
        Self::Sixteenth,
        Self::ThirtySecond,
        Self::SixtyFourth,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Whole => "1/1",
            Self::Half => "1/2",
            Self::Quarter => "1/4",
            Self::Eighth => "1/8",
            Self::Sixteenth => "1/16",
            Self::ThirtySecond => "1/32",
            Self::SixtyFourth => "1/64",
        }
    }

    pub fn ticks(self, ppq: u16) -> u64 {
        let quarter = u64::from(ppq);
        let ticks = match self {
            Self::Whole => quarter.saturating_mul(4),
            Self::Half => quarter.saturating_mul(2),
            Self::Quarter => quarter,
            Self::Eighth => quarter / 2,
            Self::Sixteenth => quarter / 4,
            Self::ThirtySecond => quarter / 8,
            Self::SixtyFourth => quarter / 16,
        };
        ticks.max(1)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SnapSettings {
    pub enabled: bool,
    pub division: SnapDivision,
}

impl Default for SnapSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            division: SnapDivision::Sixteenth,
        }
    }
}

impl SnapSettings {
    pub fn snap_tick(self, tick: f64, ppq: u16) -> u64 {
        let tick = tick.max(0.0);
        if self.enabled {
            let step = self.division.ticks(ppq) as f64;
            ((tick / step).round() * step) as u64
        } else {
            tick.round() as u64
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NoteRef {
    pub track: TrackId,
    pub clip: ClipId,
    pub note: NoteId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NoteDragKind {
    Move,
    Resize,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct NoteDrag {
    pub note: NoteRef,
    pub kind: NoteDragKind,
    pub pointer_start: Pos2,
    pub original_start: Ticks,
    pub original_length: Ticks,
    pub original_key: u8,
    pub preview_start: Ticks,
    pub preview_length: Ticks,
    pub preview_key: u8,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ClipDrag {
    pub track: TrackId,
    pub clip: ClipId,
    pub pointer_start_x: f32,
    pub original_start: Ticks,
    pub preview_start: Ticks,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TrackDrag {
    pub track: TrackId,
    pub original_index: usize,
    pub target_index: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct PianoRollState {
    pub px_per_beat: f32,
    pub left_tick: f64,
    pub key_height: f32,
    pub scroll_y: f32,
}

impl Default for PianoRollState {
    fn default() -> Self {
        Self {
            px_per_beat: 112.0,
            left_tick: 0.0,
            key_height: 14.0,
            scroll_y: 520.0,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct UiState {
    pub selected_track: Option<TrackId>,
    pub selected_clip: Option<ClipId>,
    pub selected_note: Option<NoteRef>,
    pub snap: SnapSettings,
    pub piano_roll: PianoRollState,
    pub note_drag: Option<NoteDrag>,
    pub clip_drag: Option<ClipDrag>,
    pub track_drag: Option<TrackDrag>,
}

impl UiState {
    pub fn ensure_valid_selection(&mut self, project: &Project) {
        if self
            .selected_track
            .is_none_or(|id| project.track(id).is_none())
        {
            self.selected_track = project.tracks.first().map(|track| track.id);
        }

        let clips = self
            .selected_track
            .and_then(|id| project.track(id))
            .map(|track| track.clips.as_slice())
            .unwrap_or(&[]);

        if self
            .selected_clip
            .is_none_or(|id| !clips.iter().any(|clip| clip.id == id))
        {
            self.selected_clip = clips.first().map(|clip| clip.id);
        }

        let selected_note_is_valid = self.selected_note.is_some_and(|selection| {
            self.selected_track == Some(selection.track)
                && self.selected_clip == Some(selection.clip)
                && project
                    .track(selection.track)
                    .and_then(|track| track.clip(selection.clip))
                    .and_then(|clip| clip.note(selection.note))
                    .is_some()
        });
        if !selected_note_is_valid {
            self.selected_note = None;
        }

        let note_drag_is_valid = self.note_drag.is_some_and(|drag| {
            project
                .track(drag.note.track)
                .and_then(|track| track.clip(drag.note.clip))
                .and_then(|clip| clip.note(drag.note.note))
                .is_some()
        });
        if !note_drag_is_valid {
            self.note_drag = None;
        }

        let clip_drag_is_valid = self.clip_drag.is_some_and(|drag| {
            project
                .track(drag.track)
                .and_then(|track| track.clip(drag.clip))
                .is_some()
        });
        if !clip_drag_is_valid {
            self.clip_drag = None;
        }

        let track_drag_is_valid = self.track_drag.is_some_and(|drag| {
            project.track(drag.track).is_some()
                && drag.original_index < project.tracks.len()
                && drag.target_index < project.tracks.len()
        });
        if !track_drag_is_valid {
            self.track_drag = None;
        }
    }

    pub fn select_track(&mut self, project: &Project, id: TrackId) {
        self.selected_track = Some(id);
        self.selected_clip = project
            .track(id)
            .and_then(|track| track.clips.first())
            .map(|clip| clip.id);
        self.selected_note = None;
    }

    pub fn select_clip(&mut self, track: TrackId, clip: ClipId) {
        if self.selected_track != Some(track) || self.selected_clip != Some(clip) {
            self.selected_note = None;
        }
        self.selected_track = Some(track);
        self.selected_clip = Some(clip);
    }

    pub fn selected_track<'a>(&self, project: &'a Project) -> Option<&'a Track> {
        self.selected_track.and_then(|id| project.track(id))
    }
}
