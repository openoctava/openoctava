use octava_core::{ClipId, EditCommand, Project, Ticks, Track, TrackId};

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

#[derive(Debug, Default)]
pub(crate) struct UiState {
    pub selected_track: Option<TrackId>,
    pub selected_clip: Option<ClipId>,
    pub piano_roll: PianoRollState,
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
    }

    pub fn select_track(&mut self, project: &Project, id: TrackId) {
        self.selected_track = Some(id);
        self.selected_clip = project
            .track(id)
            .and_then(|track| track.clips.first())
            .map(|clip| clip.id);
    }

    pub fn selected_track<'a>(&self, project: &'a Project) -> Option<&'a Track> {
        self.selected_track.and_then(|id| project.track(id))
    }
}
