//! egui user interface for Octava.
//!
//! The UI never touches audio APIs. It reads [`octava_core::Project`], renders
//! widgets and emits [`UiAction`] values for the application layer to execute.

mod clips;
mod files;
mod mixer;
mod piano_roll;
mod soundfont_player;
mod state;
mod theme;
mod timeline;

use egui::{
    Align2, CornerRadius, CursorIcon, FontId, RichText, Sense, Stroke, StrokeKind, ViewportCommand,
    WidgetText, vec2,
};
use egui_dock::{DockArea, DockState, NodeIndex, TabViewer};
use octava_core::{EditCommand, Project, Ticks};

pub use state::{
    DocumentView, FilesView, PlaybackView, SampleEntry, SoundfontPreset, SoundfontView, UiAction,
};
use state::{RenameTarget, UiState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Clips,
    Files,
    Timeline,
    PianoRoll,
    Mixer,
}

impl Tab {
    const ALL: [Self; 5] = [
        Self::Clips,
        Self::Files,
        Self::Timeline,
        Self::PianoRoll,
        Self::Mixer,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::Clips => "Clips",
            Self::Files => "Files",
            Self::Timeline => "Timeline",
            Self::PianoRoll => "Piano Roll",
            Self::Mixer => "Mixer",
        }
    }
}

const TITLEBAR_HEIGHT: f32 = 34.0;

pub struct OctavaUi {
    dock: DockState<Tab>,
    state: UiState,
    show_about: bool,
}

impl Default for OctavaUi {
    fn default() -> Self {
        Self::new()
    }
}

impl OctavaUi {
    #[must_use]
    pub fn new() -> Self {
        let mut dock = DockState::new(vec![Tab::Timeline]);
        let [timeline, _library] = dock.main_surface_mut().split_left(
            NodeIndex::root(),
            0.18,
            vec![Tab::Clips, Tab::Files],
        );
        let _ =
            dock.main_surface_mut()
                .split_below(timeline, 0.62, vec![Tab::PianoRoll, Tab::Mixer]);

        Self {
            dock,
            state: UiState::default(),
            show_about: false,
        }
    }

    /// Installs the dark Octava theme into an egui context.
    pub fn install_theme(ctx: &egui::Context) {
        theme::install(ctx);
    }

    /// Drops selections and in-flight edits that referred to the previous
    /// project. Called after opening a file or creating a new project.
    pub fn project_replaced(&mut self) {
        let state = &mut self.state;
        state.selected_track = None;
        state.selected_clip = None;
        state.selected_pattern = None;
        state.selected_note = None;
        state.selected_sample = None;
        state.clipboard = None;
        state.pending_clip = None;
        state.note_drag = None;
        state.clip_drag = None;
        state.sample_drag = None;
        state.track_drag = None;
        state.mixer_drag = None;
        state.soundfont_player = None;
        state.rename = None;
        state.focus_piano_roll = false;
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        project: &Project,
        playback: &PlaybackView,
        soundfont: &SoundfontView,
        files: &FilesView,
        document: &DocumentView,
    ) -> Vec<UiAction> {
        self.state.ensure_valid_selection(project);
        let mut actions = Vec::new();

        self.title_bar(ui, project, playback, soundfont, document, &mut actions);
        transport_bar(ui, project, playback, &mut self.state, &mut actions);
        status_bar(ui, project, playback, document);

        let Self {
            dock,
            state,
            show_about,
        } = self;
        let mut viewer = WorkspaceViewer {
            project,
            playback,
            soundfont,
            files,
            state,
            actions: &mut actions,
        };

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(theme::BACKGROUND)
                    .inner_margin(egui::Margin::same(5)),
            )
            .show(ui, |ui| {
                let mut style = egui_dock::Style::from_egui(ui.style().as_ref());
                style.main_surface_border_stroke = Stroke::NONE;
                style.dock_area_padding = Some(egui::Margin::ZERO);

                DockArea::new(dock)
                    .style(style)
                    .show_close_buttons(true)
                    .show_add_buttons(false)
                    .show_leaf_close_all_buttons(false)
                    .show_leaf_collapse_buttons(false)
                    .show_secondary_button_hint(false)
                    .show_inside(ui, &mut viewer);
            });

        if self.state.focus_piano_roll {
            self.state.focus_piano_roll = false;
            if self.dock.find_tab(&Tab::PianoRoll).is_none() {
                self.dock.push_to_focused_leaf(Tab::PianoRoll);
            }
            if let Some(path) = self.dock.find_tab(&Tab::PianoRoll) {
                let _ = self.dock.set_active_tab(path);
            }
        }

        if *show_about {
            about_window(ui.ctx(), show_about);
        }

        soundfont_player::show(ui.ctx(), project, soundfont, &mut self.state, &mut actions);
        rename_window(ui.ctx(), &mut self.state, &mut actions);

        actions
    }

    fn title_bar(
        &mut self,
        ui: &mut egui::Ui,
        project: &Project,
        playback: &PlaybackView,
        soundfont: &SoundfontView,
        document: &DocumentView,
        actions: &mut Vec<UiAction>,
    ) {
        let mut show_about = false;
        let mut reset_layout = false;
        let mut toggle_tab = None;
        let selected_track = self.state.selected_track;
        let selected_pattern = self.state.selected_pattern;
        let selected_note = self.state.selected_note;
        let open_tabs = Tab::ALL.map(|tab| self.dock.find_tab(&tab).is_some());

        egui::Panel::top("octava-titlebar")
            .exact_size(TITLEBAR_HEIGHT)
            .frame(
                egui::Frame::NONE
                    .fill(theme::TITLEBAR)
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .stroke(Stroke::new(1.0, theme::GRID_LINE)),
            )
            .show(ui, |ui| {
                let drag_response = ui.interact(
                    ui.max_rect(),
                    ui.id().with("titlebar-drag"),
                    Sense::click_and_drag(),
                );
                if drag_response.drag_started() {
                    ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
                }
                if drag_response.double_clicked() {
                    toggle_maximize(ui.ctx());
                }

                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;

                    ui.menu_button(
                        RichText::new("OpenOctava").strong().color(theme::ACCENT),
                        |ui| {
                            if ui.button("About OpenOctava").clicked() {
                                show_about = true;
                                ui.close();
                            }
                            ui.separator();
                            if ui.button("Quit").clicked() {
                                ui.ctx().send_viewport_cmd(ViewportCommand::Close);
                                ui.close();
                            }
                        },
                    );

                    ui.menu_button("File", |ui| {
                        if ui.button("New Project").clicked() {
                            actions.push(UiAction::NewProject);
                            ui.close();
                        }
                        if ui.button("Open Project…    Ctrl+O").clicked() {
                            actions.push(UiAction::OpenProject);
                            ui.close();
                        }
                        ui.menu_button("Open Recent", |ui| {
                            if document.recent_projects.is_empty() {
                                ui.add_enabled(false, egui::Button::new("No recent projects"));
                            } else {
                                for path in &document.recent_projects {
                                    let label = path
                                        .file_name()
                                        .and_then(|name| name.to_str())
                                        .unwrap_or("project");
                                    if ui
                                        .button(label)
                                        .on_hover_text(path.display().to_string())
                                        .clicked()
                                    {
                                        actions.push(UiAction::OpenRecentProject(path.clone()));
                                        ui.close();
                                    }
                                }
                                ui.separator();
                                if ui.button("Clear Recent").clicked() {
                                    actions.push(UiAction::ClearRecentProjects);
                                    ui.close();
                                }
                            }
                        });
                        ui.separator();
                        if ui.button("Save Project    Ctrl+S").clicked() {
                            actions.push(UiAction::SaveProject);
                            ui.close();
                        }
                        if ui.button("Save Project As…    Ctrl+Shift+S").clicked() {
                            actions.push(UiAction::SaveProjectAs);
                            ui.close();
                        }
                    });

                    ui.menu_button("Edit", |ui| {
                        if ui.button("Undo    Ctrl+Z").clicked() {
                            actions.push(UiAction::Undo);
                            ui.close();
                        }
                        if ui.button("Redo    Ctrl+Shift+Z").clicked() {
                            actions.push(UiAction::Redo);
                            ui.close();
                        }
                        ui.separator();
                        if ui
                            .button(format!("Add Track {}", project.tracks.len() + 1))
                            .clicked()
                        {
                            actions.push(UiAction::Edit(EditCommand::AddTrack {
                                name: format!("Track {}", project.tracks.len() + 1),
                            }));
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                selected_track.is_some() && selected_pattern.is_some(),
                                egui::Button::new("Add Clip at Playhead"),
                            )
                            .on_hover_text("Place the selected clip from the Clips panel")
                            .clicked()
                            && let (Some(track), Some(pattern)) = (selected_track, selected_pattern)
                        {
                            actions.push(UiAction::Edit(EditCommand::AddClip {
                                track,
                                pattern,
                                start: playback.playhead,
                            }));
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                selected_note.is_some(),
                                egui::Button::new("Delete Selected Note"),
                            )
                            .clicked()
                            && let Some(selection) = selected_note
                        {
                            actions.push(UiAction::Edit(EditCommand::RemoveNotes {
                                pattern: selection.pattern,
                                notes: vec![selection.note],
                            }));
                            ui.close();
                        }
                    });

                    ui.menu_button("Windows", |ui| {
                        for (index, tab) in Tab::ALL.iter().enumerate() {
                            let open = open_tabs[index];
                            let label = if open {
                                format!("✓ {}", tab.title())
                            } else {
                                format!("   {}  (closed)", tab.title())
                            };
                            if ui.selectable_label(open, label).clicked() {
                                toggle_tab = Some(*tab);
                                ui.close();
                            }
                        }
                        ui.separator();
                        if ui.button("Reset Layout").clicked() {
                            reset_layout = true;
                            ui.close();
                        }
                    });

                    ui.menu_button("Export", |ui| {
                        if ui.button("Export WAV…").clicked() {
                            actions.push(UiAction::ExportWav);
                            ui.close();
                        }
                        ui.add_enabled(false, egui::Button::new("Export MP3…"));
                    });

                    ui.menu_button("Audio", |ui| {
                        let status = playback.audio_sample_rate.map_or_else(
                            || "Audio engine offline".to_owned(),
                            |rate| format!("Audio engine: {rate} Hz"),
                        );
                        ui.label(RichText::new(status).size(11.0).color(theme::TEXT_DIM));
                        let soundfont_label = soundfont.path.as_deref().map_or_else(
                            || "SoundFont: bundled bank".to_owned(),
                            |path| {
                                let name = std::path::Path::new(path).file_name().map_or_else(
                                    || path.to_owned(),
                                    |name| name.to_string_lossy().into_owned(),
                                );
                                format!("SoundFont: {name}")
                            },
                        );
                        ui.label(
                            RichText::new(soundfont_label)
                                .size(10.5)
                                .color(theme::TEXT_FAINT),
                        );
                        ui.separator();
                        ui.add_enabled(false, egui::Button::new("Audio Settings…"));
                    });

                    ui.add_space(8.0);
                    let document_label = document.file_name().map_or_else(
                        || project.name.clone(),
                        |name| format!("{}  ·  {name}", project.name),
                    );
                    ui.label(
                        RichText::new(document_label)
                            .size(10.5)
                            .color(theme::TEXT_FAINT),
                    );
                    if document.dirty {
                        ui.label(RichText::new("●").size(10.5).color(theme::ACCENT))
                            .on_hover_text("Unsaved changes");
                    }

                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        window_controls,
                    );
                });
            });

        if reset_layout {
            self.dock = Self::new().dock;
        }
        if let Some(tab) = toggle_tab {
            if let Some(path) = self.dock.find_tab(&tab) {
                let _ = self.dock.set_active_tab(path);
            } else {
                self.dock.push_to_focused_leaf(tab);
                if let Some(path) = self.dock.find_tab(&tab) {
                    let _ = self.dock.set_active_tab(path);
                }
            }
        }
        if show_about {
            self.show_about = true;
        }
    }
}

fn window_controls(ui: &mut egui::Ui) {
    if window_control(ui, "×", true).clicked() {
        ui.ctx().send_viewport_cmd(ViewportCommand::Close);
    }

    let maximized = ui.input(|input| input.viewport().maximized.unwrap_or(false));
    if window_control(ui, if maximized { "❐" } else { "□" }, false).clicked() {
        toggle_maximize(ui.ctx());
    }

    if window_control(ui, "—", false).clicked() {
        ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
    }
}

fn window_control(ui: &mut egui::Ui, glyph: &str, close: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(30.0, 24.0), Sense::click());
    let hovered = response.hovered();
    let fill = if hovered && close {
        egui::Color32::from_rgb(180, 62, 54)
    } else if hovered {
        theme::PANEL_RAISED
    } else {
        theme::TITLEBAR
    };
    let color = if hovered && close {
        egui::Color32::WHITE
    } else {
        theme::TEXT_DIM
    };
    ui.painter().rect_filled(rect, CornerRadius::same(3), fill);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(13.0),
        color,
    );
    response.on_hover_cursor(CursorIcon::PointingHand)
}

fn toggle_maximize(ctx: &egui::Context) {
    let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
    ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
}

fn about_window(ctx: &egui::Context, open: &mut bool) {
    if !*open {
        return;
    }

    egui::Window::new("About OpenOctava")
        .open(open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(
                RichText::new("OpenOctava")
                    .size(22.0)
                    .strong()
                    .color(theme::ACCENT),
            );
            ui.label(
                RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION")))
                    .size(11.0)
                    .color(theme::TEXT_DIM),
            );
            ui.add_space(8.0);
            ui.label(
                RichText::new(
                    "A lightweight native DAW for composing with MIDI clips and SoundFonts.",
                )
                .size(12.0),
            );
            ui.add_space(8.0);
            ui.label(
                RichText::new("MIT OR Apache-2.0  ·  FluidLite LGPL-2.1")
                    .size(10.0)
                    .color(theme::TEXT_FAINT),
            );
        });
}

fn rename_window(ctx: &egui::Context, state: &mut UiState, actions: &mut Vec<UiAction>) {
    let Some(rename) = state.rename.as_mut() else {
        return;
    };
    let title = match rename.target {
        RenameTarget::Track(_) => "Rename Track",
        RenameTarget::Pattern(_) => "Rename Clip",
    };
    let mut open = true;
    let mut submit = false;
    let mut cancel = false;

    egui::Window::new(title)
        .id(egui::Id::new("octava-rename"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, -40.0])
        .show(ctx, |ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut rename.text)
                    .desired_width(240.0)
                    .hint_text("Name"),
            );
            if rename.focus {
                response.request_focus();
                rename.focus = false;
            }
            if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                submit = true;
            }
            if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                cancel = true;
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!rename.text.trim().is_empty(), egui::Button::new("Rename"))
                    .clicked()
                {
                    submit = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });

    let target = rename.target;
    let text = rename.text.trim().to_owned();
    if submit && !text.is_empty() {
        actions.push(match target {
            RenameTarget::Track(track) => {
                UiAction::Edit(EditCommand::SetTrackName { track, name: text })
            }
            RenameTarget::Pattern(pattern) => UiAction::Edit(EditCommand::SetPatternName {
                pattern,
                name: text,
            }),
        });
        state.rename = None;
    } else if cancel || !open {
        state.rename = None;
    }
}

struct WorkspaceViewer<'a> {
    project: &'a Project,
    playback: &'a PlaybackView,
    soundfont: &'a SoundfontView,
    files: &'a FilesView,
    state: &'a mut UiState,
    actions: &'a mut Vec<UiAction>,
}

impl TabViewer for WorkspaceViewer<'_> {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Self::Tab) -> WidgetText {
        tab.title().into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab {
            Tab::Clips => {
                clips::show(ui, self.project, self.soundfont, self.state, self.actions);
            }
            Tab::Files => {
                files::show(
                    ui,
                    self.project,
                    self.playback,
                    self.files,
                    self.state,
                    self.actions,
                );
            }
            Tab::Timeline => {
                timeline::show(ui, self.project, self.playback, self.state, self.actions)
            }
            Tab::PianoRoll => {
                piano_roll::show(ui, self.project, self.playback, self.state, self.actions)
            }
            Tab::Mixer => {
                mixer::show(ui, self.project, self.playback, self.state, self.actions);
            }
        }
    }

    fn is_closeable(&self, _tab: &Self::Tab) -> bool {
        true
    }
}

fn transport_bar(
    ui: &mut egui::Ui,
    project: &Project,
    playback: &PlaybackView,
    state: &mut UiState,
    actions: &mut Vec<UiAction>,
) {
    egui::Panel::top("octava-transport")
        .exact_size(54.0)
        .frame(
            egui::Frame::NONE
                .fill(theme::PANEL)
                .inner_margin(egui::Margin::symmetric(14, 8))
                .stroke(Stroke::new(1.0, theme::GRID_LINE)),
        )
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(
                    RichText::new("OCTAVA")
                        .size(18.0)
                        .strong()
                        .color(theme::ACCENT),
                );
                ui.add_space(2.0);
                ui.label(
                    RichText::new(&project.name)
                        .size(12.0)
                        .color(theme::TEXT_DIM),
                );
                ui.add_space(6.0);
                ui.separator();

                let play_label = if playback.playing { "Pause" } else { "Play" };
                let play_button = egui::Button::new(
                    RichText::new(play_label)
                        .strong()
                        .color(egui::Color32::from_rgb(28, 22, 15)),
                )
                .fill(theme::ACCENT);
                if ui.add_sized([68.0, 28.0], play_button).clicked() {
                    actions.push(if playback.playing {
                        UiAction::Pause
                    } else {
                        UiAction::Play
                    });
                }

                if ui
                    .add_sized([56.0, 28.0], egui::Button::new("Stop"))
                    .clicked()
                {
                    actions.push(UiAction::Stop);
                }

                ui.separator();
                let ppq = project.tempo_map.ppq();
                let (bar, beat, tick) =
                    project.time_signature.bar_beat_tick(playback.playhead, ppq);
                ui.label(
                    RichText::new(format!("{:03}.{:02}.{:03}", bar + 1, beat + 1, tick))
                        .monospace()
                        .size(17.0)
                        .color(theme::TEXT),
                );
                ui.separator();
                let tempo_tick = project
                    .tempo_map
                    .segments()
                    .iter()
                    .rev()
                    .find(|segment| segment.start <= playback.playhead)
                    .map_or(Ticks::ZERO, |segment| segment.start);
                let stored_bpm = project.tempo_map.tempo_at(playback.playhead);
                let mut bpm = state
                    .tempo_drag
                    .filter(|drag| drag.tick == tempo_tick)
                    .map_or(stored_bpm, |drag| drag.bpm);
                let bpm_response = ui.add(
                    egui::DragValue::new(&mut bpm)
                        .range(20.0..=400.0)
                        .speed(0.25)
                        .fixed_decimals(1)
                        .suffix(" BPM"),
                );
                if bpm_response.changed() {
                    state.tempo_drag = Some(state::TempoDrag {
                        tick: tempo_tick,
                        bpm,
                    });
                }
                if (bpm_response.drag_stopped() || bpm_response.lost_focus())
                    && let Some(drag) = state.tempo_drag.take()
                    && (drag.bpm - project.tempo_map.tempo_at(drag.tick)).abs() > 0.000_1
                {
                    actions.push(UiAction::Edit(EditCommand::SetTempo {
                        tick: drag.tick,
                        bpm: drag.bpm,
                    }));
                }
                ui.label(
                    RichText::new(format!(
                        "{}/{}",
                        project.time_signature.numerator, project.time_signature.denominator
                    ))
                    .size(11.5)
                    .color(theme::TEXT_DIM),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    peak_meter(ui, "R", playback.peak_right);
                    peak_meter(ui, "L", playback.peak_left);
                    ui.label(RichText::new("OUT").size(9.0).color(theme::TEXT_FAINT));
                });
            });
        });
}

fn peak_meter(ui: &mut egui::Ui, label: &str, value: f32) {
    ui.label(
        RichText::new(label)
            .monospace()
            .size(9.0)
            .color(theme::TEXT_FAINT),
    );
    let (rect, _) = ui.allocate_exact_size(vec2(46.0, 7.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(2), theme::BACKGROUND);

    let value = value.clamp(0.0, 1.0);
    if value > 0.0 {
        let bar = egui::Rect::from_min_size(rect.min, vec2(rect.width() * value, rect.height()));
        let color = if value > 0.9 {
            egui::Color32::from_rgb(218, 92, 82)
        } else {
            theme::ACCENT
        };
        painter.rect_filled(bar, CornerRadius::same(2), color);
    }
    painter.rect_stroke(
        rect,
        CornerRadius::same(2),
        Stroke::new(1.0, theme::GRID_LINE),
        StrokeKind::Inside,
    );
}

fn status_bar(
    ui: &mut egui::Ui,
    project: &Project,
    playback: &PlaybackView,
    document: &DocumentView,
) {
    egui::Panel::bottom("octava-status")
        .exact_size(21.0)
        .frame(
            egui::Frame::NONE
                .fill(theme::PANEL)
                .inner_margin(egui::Margin::symmetric(12, 2))
                .stroke(Stroke::new(1.0, theme::GRID_LINE)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let sample_count: usize =
                    project.tracks.iter().map(|track| track.samples.len()).sum();
                ui.label(
                    RichText::new(format!(
                        "{} tracks  ·  {} notes  ·  {} samples  ·  {} PPQ",
                        project.tracks.len(),
                        project.note_count(),
                        sample_count,
                        project.tempo_map.ppq()
                    ))
                    .size(9.5)
                    .color(theme::TEXT_FAINT),
                );
                let audio_status = playback.audio_sample_rate.map_or_else(
                    || "preview transport · audio engine offline".to_owned(),
                    |rate| format!("audio engine · {rate} Hz"),
                );
                ui.label(
                    RichText::new(format!("· {audio_status}"))
                        .size(9.5)
                        .color(theme::TEXT_FAINT),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let state = if playback.playing {
                        "playing"
                    } else {
                        "stopped"
                    };
                    ui.label(
                        RichText::new(format!("Space: play/pause  ·  {state}"))
                            .size(9.5)
                            .color(theme::TEXT_FAINT),
                    );
                    if let Some(path) = &document.path {
                        ui.label(
                            RichText::new(if document.dirty { "unsaved" } else { "saved" })
                                .size(9.5)
                                .color(if document.dirty {
                                    theme::ACCENT
                                } else {
                                    theme::TEXT_FAINT
                                }),
                        );
                        ui.label(RichText::new(path).size(9.5).color(theme::TEXT_FAINT));
                    }
                });
            });
        });
}
