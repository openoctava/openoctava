use std::collections::BTreeSet;
use std::path::PathBuf;

use egui::Pos2;
use octava_core::{
    ClipId, EditCommand, NoteId, PatternId, Preset, Project, SampleId, Ticks, TrackId,
};

/// Read-only transport state supplied by the application layer.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlaybackView {
    pub playing: bool,
    pub playhead: Ticks,
    pub peak_left: f32,
    pub peak_right: f32,
    pub audio_sample_rate: Option<u32>,
}

/// One instrument exposed by the loaded SoundFont.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoundfontPreset {
    pub name: String,
    pub bank: u8,
    pub program: u8,
}

/// SoundFont information supplied by the application layer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SoundfontView {
    /// Path of the loaded SoundFont, `None` when the bundled bank is used.
    pub path: Option<String>,
    pub presets: Vec<SoundfontPreset>,
    pub error: Option<String>,
}

impl SoundfontView {
    #[must_use]
    pub fn preset_name(&self, preset: Preset) -> Option<&str> {
        self.presets
            .iter()
            .find(|candidate| candidate.bank == preset.bank && candidate.program == preset.program)
            .map(|candidate| candidate.name.as_str())
    }
}

/// State of the project file shown in the title bar.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocumentView {
    pub path: Option<String>,
    pub dirty: bool,
    pub recent_projects: Vec<PathBuf>,
}

impl DocumentView {
    #[must_use]
    pub fn file_name(&self) -> Option<&str> {
        self.path.as_deref().and_then(|path| {
            std::path::Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
        })
    }
}

/// One WAV file found in the sample browser folder.
#[derive(Clone, Debug, PartialEq)]
pub struct SampleEntry {
    pub name: String,
    pub path: PathBuf,
    pub duration_seconds: f64,
    pub sample_rate: u32,
    pub channels: u16,
}

/// Sample browser contents supplied by the application layer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FilesView {
    pub folder: Option<String>,
    pub entries: Vec<SampleEntry>,
    pub error: Option<String>,
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
    OpenProject,
    OpenRecentProject(PathBuf),
    ClearRecentProjects,
    SaveProject,
    SaveProjectAs,
    ExportWav,
    /// Ask the app shell to open the desktop's native SoundFont picker.
    ChooseSoundfont,
    /// Ask the app shell to open a folder picker for the sample browser.
    OpenSampleFolder,
    /// Load a folder path typed into the Files panel.
    SetSampleFolder(PathBuf),
    /// Re-scan the current sample folder and drop cached decodes.
    RefreshSamples,
    /// Audition a sample through the audio engine while stopped.
    AuditionSample(PathBuf),
    AuditionNote {
        track: TrackId,
        preset: Preset,
        key: u8,
        velocity: u8,
    },
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
    /// Snap to the closest grid line. Dragging and resizing use this so the
    /// edited edge follows the cursor without lagging behind a whole cell.
    pub fn snap_tick(self, tick: f64, ppq: u16) -> u64 {
        let tick = tick.max(0.0);
        if self.enabled {
            let step = self.division.ticks(ppq) as f64;
            ((tick / step).round() * step) as u64
        } else {
            tick.round() as u64
        }
    }

    /// Snap down to the grid line at or before `tick`. Note creation uses this
    /// so a click starts the note in the cell under the cursor: snapping to
    /// the closest line made clicks in the right half of a cell jump one cell
    /// to the right.
    pub fn snap_tick_floor(self, tick: f64, ppq: u16) -> u64 {
        let tick = tick.max(0.0);
        if self.enabled {
            let step = self.division.ticks(ppq) as f64;
            ((tick / step).floor() * step) as u64
        } else {
            tick.round() as u64
        }
    }
}

/// Identifies a clip placement on a track.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ClipRef {
    pub track: TrackId,
    pub clip: ClipId,
}

/// A note inside a pattern. Notes are shared by every placement of a pattern,
/// so the pattern id is part of the reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct NoteRef {
    pub pattern: PatternId,
    pub note: NoteId,
}

/// Payload dragged from the clips library onto the timeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PatternDragPayload {
    pub pattern: PatternId,
}

/// Clipboard entry for Ctrl+C / Ctrl+V on the timeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ClipboardClip {
    pub pattern: PatternId,
    pub length: Ticks,
}

/// A clip placement the UI just asked the app layer to create. Once the
/// command lands, the next frame selects it so that repeated Ctrl+D chains.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PendingClip {
    pub track: TrackId,
    pub pattern: PatternId,
    pub start: Ticks,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NoteDragKind {
    Move,
    Resize,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct NoteDragSnapshot {
    pub note: NoteRef,
    pub original_start: Ticks,
    pub original_length: Ticks,
    pub original_key: u8,
    pub preview_start: Ticks,
    pub preview_length: Ticks,
    pub preview_key: u8,
}

#[derive(Clone, Debug)]
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
    pub notes: Vec<NoteDragSnapshot>,
}

#[derive(Clone, Debug)]
pub(crate) struct BoxSelect {
    pub start: Pos2,
    pub current: Pos2,
    pub initial_selected: BTreeSet<NoteRef>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClipDragKind {
    Move,
    ResizeEnd,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ClipDrag {
    pub track: TrackId,
    pub target_track: TrackId,
    pub clip: ClipId,
    pub kind: ClipDragKind,
    pub pointer_start_x: f32,
    pub original_start: Ticks,
    pub original_length: Ticks,
    pub preview_start: Ticks,
    pub preview_length: Ticks,
}

/// Payload dragged from the Files browser onto the timeline.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SampleDragPayload {
    pub name: String,
    pub path: PathBuf,
    pub duration_seconds: f64,
}

/// Identifies a sample placement on a track.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SampleRef {
    pub track: TrackId,
    pub sample: SampleId,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SampleDrag {
    pub track: TrackId,
    pub sample: SampleId,
    pub pointer_start_x: f32,
    pub original_start: Ticks,
    pub preview_start: Ticks,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MixerParam {
    Gain,
    Pan,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MixerDrag {
    pub track: TrackId,
    pub param: MixerParam,
    pub original: f32,
    pub preview: f32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TrackDrag {
    pub track: TrackId,
    pub original_index: usize,
    pub target_index: usize,
}

/// In-progress tempo edit, applied once the drag ends so that dragging the
/// BPM field does not flood the undo history with intermediate values.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TempoDrag {
    pub tick: Ticks,
    pub bpm: f64,
}

/// Object a rename dialog is editing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RenameTarget {
    Track(TrackId),
    Pattern(PatternId),
}

impl UiState {
    pub(crate) fn start_rename(&mut self, target: RenameTarget, current: &str) {
        self.rename = Some(RenameState {
            target,
            text: current.to_owned(),
            focus: true,
        });
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RenameState {
    pub target: RenameTarget,
    pub text: String,
    /// Set for the first frame so that the text field grabs the keyboard.
    pub focus: bool,
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

/// Viewport of the track timeline: a zoom factor over the fit-to-width scale,
/// the first visible tick and the vertical lane offset.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TimelineState {
    pub zoom: f32,
    pub left_tick: f64,
    pub scroll_y: f32,
}

impl Default for TimelineState {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            left_tick: 0.0,
            scroll_y: 0.0,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct UiState {
    pub selected_track: Option<TrackId>,
    /// Clip placement selected on the timeline.
    pub selected_clip: Option<ClipRef>,
    /// Pattern opened in the piano roll / focused in the clips library.
    pub selected_pattern: Option<PatternId>,
    pub selected_note: Option<NoteRef>,
    pub selected_notes: BTreeSet<NoteRef>,
    pub box_select: Option<BoxSelect>,
    /// Sample placement selected on the timeline.
    pub selected_sample: Option<SampleRef>,
    pub clipboard: Option<ClipboardClip>,
    pub pending_clip: Option<PendingClip>,
    pub snap: SnapSettings,
    pub piano_roll: PianoRollState,
    pub timeline: TimelineState,
    pub tempo_drag: Option<TempoDrag>,
    pub note_drag: Option<NoteDrag>,
    pub clip_drag: Option<ClipDrag>,
    pub sample_drag: Option<SampleDrag>,
    pub track_drag: Option<TrackDrag>,
    pub mixer_drag: Option<MixerDrag>,
    pub focus_piano_roll: bool,
    /// Pattern whose SoundFont player window is open.
    pub soundfont_player: Option<PatternId>,
    pub rename: Option<RenameState>,
}

impl UiState {
    pub fn ensure_valid_selection(&mut self, project: &Project) {
        let selected_track_is_valid = self
            .selected_track
            .is_some_and(|track| project.track(track).is_some());
        if !selected_track_is_valid {
            self.selected_track = project.tracks.first().map(|track| track.id);
        }

        let selected_clip_is_valid = self.selected_clip.is_some_and(|selection| {
            project
                .track(selection.track)
                .and_then(|track| track.clip(selection.clip))
                .is_some()
        });
        if !selected_clip_is_valid {
            self.selected_clip = None;
        }

        let selected_pattern_is_valid = self
            .selected_pattern
            .is_some_and(|pattern| project.pattern(pattern).is_some());
        if !selected_pattern_is_valid {
            self.selected_pattern = self
                .selected_clip
                .and_then(|selection| project.track(selection.track))
                .and_then(|track| track.clip(self.selected_clip?.clip))
                .map(|clip| clip.pattern)
                .or_else(|| project.patterns.first().map(|pattern| pattern.id));
        }

        self.selected_notes.retain(|selection| {
            project
                .pattern(selection.pattern)
                .and_then(|pattern| pattern.note(selection.note))
                .is_some()
        });
        if let Some(box_sel) = &mut self.box_select {
            box_sel.initial_selected.retain(|selection| {
                project
                    .pattern(selection.pattern)
                    .and_then(|pattern| pattern.note(selection.note))
                    .is_some()
            });
        }
        let selected_note_is_valid = self.selected_note.is_some_and(|selection| {
            project
                .pattern(selection.pattern)
                .and_then(|pattern| pattern.note(selection.note))
                .is_some()
        });
        if !selected_note_is_valid {
            self.selected_note = self.selected_notes.iter().copied().next();
        }

        let note_drag_is_valid = self.note_drag.as_ref().is_some_and(|drag| {
            project
                .pattern(drag.note.pattern)
                .and_then(|pattern| pattern.note(drag.note.note))
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

        let sample_drag_is_valid = self.sample_drag.is_some_and(|drag| {
            project
                .track(drag.track)
                .and_then(|track| track.sample(drag.sample))
                .is_some()
        });
        if !sample_drag_is_valid {
            self.sample_drag = None;
        }

        let soundfont_player_is_valid = self
            .soundfont_player
            .is_some_and(|pattern| project.pattern(pattern).is_some());
        if !soundfont_player_is_valid {
            self.soundfont_player = None;
        }
    }

    /// Selects a track and picks an appropriate pattern to show in the piano roll:
    /// - If the track has clips of only one pattern, selects that pattern.
    /// - If the track has multiple patterns, picks the one with the earliest start.
    /// - Otherwise leaves `selected_pattern` alone.
    pub fn select_track(&mut self, project: &Project, track: TrackId) {
        self.selected_track = Some(track);
        self.selected_clip = None;
        self.selected_sample = None;
        let Some(track_data) = project.track(track) else {
            return;
        };
        let mut patterns_in_track = track_data
            .clips
            .iter()
            .map(|clip| (clip.start, clip.pattern))
            .collect::<Vec<_>>();
        patterns_in_track.sort_by_key(|(start, _)| *start);
        if let Some((_, pattern)) = patterns_in_track.first() {
            self.selected_pattern = Some(*pattern);
        } else if let Some(pattern) = project.patterns.first().map(|p| p.id) {
            self.selected_pattern = Some(pattern);
        }
        self.selected_note = None;
        self.selected_notes.clear();
    }

    pub fn select_clip(&mut self, project: &Project, track: TrackId, clip: ClipId) {
        let selection = ClipRef { track, clip };
        if self.selected_clip != Some(selection) {
            self.selected_note = None;
            self.selected_notes.clear();
        }
        self.selected_track = Some(track);
        self.selected_sample = None;
        self.selected_clip = Some(selection);
        if let Some(track) = project.track(track)
            && let Some(clip) = track.clip(clip)
        {
            self.selected_pattern = Some(clip.pattern);
        }
    }

    pub fn select_sample(&mut self, track: TrackId, sample: SampleId) {
        let selection = SampleRef { track, sample };
        if self.selected_sample != Some(selection) {
            self.selected_clip = None;
            self.selected_note = None;
            self.selected_notes.clear();
        }
        self.selected_track = Some(track);
        self.selected_sample = Some(selection);
    }

    /// Focuses a pattern in the piano roll. Note that an auditioned track
    /// pattern only sounds where it has been placed on a track.
    pub fn select_pattern(&mut self, pattern: PatternId) {
        if self.selected_pattern != Some(pattern) {
            self.deselect_all_notes();
        }
        self.selected_pattern = Some(pattern);
    }

    pub fn is_note_selected(&self, note: &NoteRef) -> bool {
        self.selected_notes.contains(note) || self.selected_note == Some(*note)
    }

    pub fn select_note(&mut self, note: NoteRef, add: bool) {
        if add {
            if self.selected_notes.contains(&note) {
                self.selected_notes.remove(&note);
                if self.selected_note == Some(note) {
                    self.selected_note = self.selected_notes.iter().copied().next();
                }
            } else {
                self.selected_notes.insert(note);
                self.selected_note = Some(note);
            }
        } else {
            self.selected_notes.clear();
            self.selected_notes.insert(note);
            self.selected_note = Some(note);
        }
    }

    pub fn deselect_all_notes(&mut self) {
        self.selected_note = None;
        self.selected_notes.clear();
        self.box_select = None;
    }
}
