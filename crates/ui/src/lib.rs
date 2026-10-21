//! egui user interface for Octava.
//!
//! The UI never touches audio APIs. It reads [`octava_core::Project`], renders
//! widgets and emits [`UiAction`] values for the application layer to execute.

mod inspector;
mod piano_roll;
mod state;
mod theme;
mod timeline;

use egui::{CornerRadius, RichText, Stroke, StrokeKind, WidgetText, vec2};
use egui_dock::{DockArea, DockState, NodeIndex, TabViewer};
use octava_core::Project;

use state::UiState;
pub use state::{PlaybackView, UiAction};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Timeline,
    PianoRoll,
    Inspector,
}

impl Tab {
    fn title(self) -> &'static str {
        match self {
            Self::Timeline => "Timeline",
            Self::PianoRoll => "Piano Roll",
            Self::Inspector => "Inspector",
        }
    }
}

pub struct OctavaUi {
    dock: DockState<Tab>,
    state: UiState,
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
        dock.main_surface_mut()
            .split_right(NodeIndex::root(), 0.76, vec![Tab::Inspector]);
        dock.main_surface_mut()
            .split_below(NodeIndex::root(), 0.34, vec![Tab::PianoRoll]);

        Self {
            dock,
            state: UiState::default(),
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

        transport_bar(ui, project, playback, &mut actions);
        status_bar(ui, project, playback);

        let Self { dock, state } = self;
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

        actions
    }
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
            Tab::Timeline => {
                timeline::show(ui, self.project, self.playback, self.state, self.actions)
            }
            Tab::PianoRoll => {
                piano_roll::show(ui, self.project, self.playback, self.state, self.actions)
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
