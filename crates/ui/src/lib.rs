//! egui user interface for Octava.
//!
//! The UI never touches audio APIs. It reads [`octava_core::Project`], renders
//! widgets and emits [`UiAction`] values for the application layer to execute.

mod clips;
mod inspector;
mod mixer;
mod piano_roll;
mod state;
mod theme;
mod timeline;

use egui::{
    Align2, CornerRadius, CursorIcon, FontId, RichText, Sense, Stroke, StrokeKind, ViewportCommand,
    WidgetText, vec2,
};
use egui_dock::{DockArea, DockState, NodeIndex, TabViewer};
use octava_core::{EditCommand, Project};

use state::UiState;
pub use state::{PlaybackView, UiAction};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Clips,
    Timeline,
    PianoRoll,
    Mixer,
    Inspector,
}

impl Tab {
    fn title(self) -> &'static str {
        match self {
            Self::Clips => "Clips",
            Self::Timeline => "Timeline",
            Self::PianoRoll => "Piano Roll",
            Self::Mixer => "Mixer",
            Self::Inspector => "Inspector",
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
        let [timeline, _clips] =
            dock.main_surface_mut()
                .split_left(NodeIndex::root(), 0.18, vec![Tab::Clips]);
        let [timeline, _inspector] =
            dock.main_surface_mut()
                .split_right(timeline, 0.76, vec![Tab::Inspector]);
        let _ =
            dock.main_surface_mut()
                .split_below(timeline, 0.66, vec![Tab::PianoRoll, Tab::Mixer]);

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

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        project: &Project,
        playback: &PlaybackView,
    ) -> Vec<UiAction> {
        self.state.ensure_valid_selection(project);
        let mut actions = Vec::new();

        self.title_bar(ui, project, playback, &mut actions);
        transport_bar(ui, project, playback, &mut actions);
        status_bar(ui, project, playback);

        let Self {
            dock,
            state,
            show_about,
        } = self;
        let mut viewer = WorkspaceViewer {
            project,
            playback,
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
                    .show_close_buttons(false)
                    .show_add_buttons(false)
                    .show_leaf_close_all_buttons(false)
                    .show_leaf_collapse_buttons(false)
                    .show_secondary_button_hint(false)
                    .show_inside(ui, &mut viewer);
            });

        if self.state.focus_piano_roll
            && let Some(path) = self.dock.find_tab(&Tab::PianoRoll)
        {
            self.state.focus_piano_roll = false;
            let _ = self.dock.set_active_tab(path);
        }

        if *show_about {
            about_window(ui.ctx(), show_about);
        }

        actions
    }

    fn title_bar(
        &mut self,
        ui: &mut egui::Ui,
        project: &Project,
        playback: &PlaybackView,
        actions: &mut Vec<UiAction>,
    ) {
        let mut show_about = false;
        let mut reset_layout = false;
        let mut focus_tab = None;
        let selected_track = self.state.selected_track;
        let selected_pattern = self.state.selected_pattern;
        let selected_note = self.state.selected_note;

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
                        ui.add_enabled(false, egui::Button::new("Open Project…"));
                        ui.separator();
                        ui.add_enabled(false, egui::Button::new("Save Project"));
                        ui.add_enabled(false, egui::Button::new("Save Project As…"));
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
                        if ui.button("Clips").clicked() {
                            focus_tab = Some(Tab::Clips);
                            ui.close();
                        }
                        if ui.button("Timeline").clicked() {
                            focus_tab = Some(Tab::Timeline);
                            ui.close();
                        }
                        if ui.button("Piano Roll").clicked() {
                            focus_tab = Some(Tab::PianoRoll);
                            ui.close();
                        }
                        if ui.button("Mixer").clicked() {
                            focus_tab = Some(Tab::Mixer);
                            ui.close();
                        }
                        if ui.button("Inspector").clicked() {
                            focus_tab = Some(Tab::Inspector);
                            ui.close();
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
                        ui.label(
                            RichText::new("SoundFont: Vintage Dreams Waves v2")
                                .size(10.5)
                                .color(theme::TEXT_FAINT),
                        );
                        ui.separator();
                        ui.add_enabled(false, egui::Button::new("Audio Settings…"));
                    });

                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(&project.name)
                            .size(10.5)
                            .color(theme::TEXT_FAINT),
                    );

                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        window_controls,
                    );
                });
            });

        if reset_layout {
            self.dock = Self::new().dock;
        }
        if let Some(tab) = focus_tab
            && let Some(path) = self.dock.find_tab(&tab)
        {
            let _ = self.dock.set_active_tab(path);
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

struct WorkspaceViewer<'a> {
    project: &'a Project,
    playback: &'a PlaybackView,
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
                clips::show(ui, self.project, self.playback, self.state, self.actions);
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
            Tab::Inspector => {
                inspector::show(ui, self.project, self.state, self.actions);
            }
        }
    }

    fn is_closeable(&self, _tab: &Self::Tab) -> bool {
        false
    }
}

fn transport_bar(
    ui: &mut egui::Ui,
    project: &Project,
    playback: &PlaybackView,
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
                ui.label(
                    RichText::new(format!(
                        "{:.1} BPM",
                        project.tempo_map.tempo_at(playback.playhead)
                    ))
                    .size(11.5)
                    .color(theme::TEXT_DIM),
                );
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

fn status_bar(ui: &mut egui::Ui, project: &Project, playback: &PlaybackView) {
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
                ui.label(
                    RichText::new(format!(
                        "{} tracks  ·  {} notes  ·  {} PPQ",
                        project.tracks.len(),
                        project.note_count(),
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
                });
            });
        });
}
