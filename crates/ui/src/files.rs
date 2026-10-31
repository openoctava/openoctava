//! Sample browser backed by a folder of WAV files.
//!
//! The application layer picks and scans the folder; this module only draws
//! the list and emits actions. Samples can be auditioned, placed at the
//! playhead or dragged onto a timeline track.

use std::path::PathBuf;

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Rect, RichText, Sense, Stroke, StrokeKind,
    pos2, vec2,
};
use octava_core::{EditCommand, Project, Ticks};

use crate::state::{FilesView, PlaybackView, SampleDragPayload, SampleEntry, UiAction, UiState};
use crate::theme;

pub(crate) fn show(
    ui: &mut egui::Ui,
    project: &Project,
    playback: &PlaybackView,
    files: &FilesView,
    state: &mut UiState,
    actions: &mut Vec<UiAction>,
) {
    egui::Frame::NONE
        .fill(theme::BACKGROUND)
        .inner_margin(egui::Margin::symmetric(6, 5))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("FILES")
                        .size(9.5)
                        .strong()
                        .color(theme::TEXT_FAINT),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button("Refresh")
                        .on_hover_text("Re-scan the folder and reload changed samples")
                        .clicked()
                    {
                        actions.push(UiAction::RefreshSamples);
                    }
                    if ui.small_button("Open Folder…").clicked() {
                        actions.push(UiAction::OpenSampleFolder);
                    }
                });
            });
            ui.add_space(3.0);

            let mut folder = files.folder.clone().unwrap_or_default();
            let load = ui
                .horizontal(|ui| {
                    let width = (ui.available_width() - 54.0).max(80.0);
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut folder)
                            .desired_width(width)
                            .hint_text("Path to a folder of WAV samples"),
                    );
                    let submitted = response.lost_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter));
                    let clicked = ui.small_button("Load").clicked();
                    (submitted || clicked).then(|| folder.trim().to_owned())
                })
                .inner;
            if let Some(folder) = load
                && !folder.is_empty()
            {
                actions.push(UiAction::SetSampleFolder(PathBuf::from(folder)));
            }

            if let Some(error) = &files.error {
                ui.add_space(2.0);
                ui.label(
                    RichText::new(error)
                        .size(9.5)
                        .color(Color32::from_rgb(222, 112, 102)),
                );
            }

            ui.add_space(2.0);
            if files.entries.is_empty() {
                ui.add_space(12.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("Open a folder to browse samples")
                            .size(12.0)
                            .color(theme::TEXT_DIM),
                    );
                    ui.label(
                        RichText::new("WAV files can be dragged onto a track")
                            .size(9.5)
                            .color(theme::TEXT_FAINT),
                    );
                });
                return;
            }

            ui.label(
                RichText::new(format!(
                    "{} samples  ·  double-click to place at the playhead",
                    files.entries.len()
                ))
                .size(9.0)
                .color(theme::TEXT_FAINT),
            );
            ui.add_space(2.0);

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for entry in &files.entries {
                        let response = sample_card(ui, entry);
                        if response.double_clicked() {
                            place_at_playhead(project, playback, state, actions, entry);
                        }
                        if response.drag_started() {
                            response.ctx.set_cursor_icon(CursorIcon::Grabbing);
                        }
                        response.dnd_set_drag_payload(SampleDragPayload {
                            name: entry.name.clone(),
                            path: entry.path.clone(),
                            duration_seconds: entry.duration_seconds,
                        });
                        response.context_menu(|ui| {
                            let enabled = state.selected_track.is_some();
                            if ui
                                .add_enabled(enabled, egui::Button::new("Place at Playhead"))
                                .on_disabled_hover_text("Select a track first")
                                .clicked()
                            {
                                place_at_playhead(project, playback, state, actions, entry);
                                ui.close();
                            }
                            if ui.button("Audition").clicked() {
                                actions.push(UiAction::AuditionSample(entry.path.clone()));
                                ui.close();
                            }
                        });
                        ui.add_space(4.0);
                    }
                });
        });
}

fn place_at_playhead(
    project: &Project,
    playback: &PlaybackView,
    state: &UiState,
    actions: &mut Vec<UiAction>,
    entry: &SampleEntry,
) {
    let Some(track) = state.selected_track else {
        return;
    };
    let start = Ticks(
        state
            .snap
            .snap_tick(playback.playhead.0 as f64, project.tempo_map.ppq()),
    );
    actions.push(UiAction::Edit(EditCommand::AddSample {
        track,
        name: entry.name.clone(),
        path: entry.path.clone(),
        start,
        duration: entry.duration_seconds,
        gain: 1.0,
    }));
}

fn sample_card(ui: &mut egui::Ui, entry: &SampleEntry) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width().max(80.0), 40.0),
        Sense::click_and_drag(),
    );
    let painter = ui.painter();
    let fill = if response.hovered() {
        theme::PANEL_RAISED
    } else {
        theme::PANEL
    };
    painter.rect_filled(rect.shrink2(vec2(1.0, 1.0)), CornerRadius::same(3), fill);
    painter.rect_stroke(
        rect.shrink2(vec2(1.0, 1.0)),
        CornerRadius::same(3),
        Stroke::new(1.0, theme::GRID_LINE),
        StrokeKind::Inside,
    );
    painter.rect_filled(
        Rect::from_min_max(
            pos2(rect.left() + 1.0, rect.top() + 1.0),
            pos2(rect.left() + 4.0, rect.bottom() - 1.0),
        ),
        CornerRadius {
            nw: 3,
            sw: 3,
            ne: 0,
            se: 0,
        },
        Color32::from_rgb(86, 176, 150),
    );
    painter.text(
        pos2(rect.left() + 10.0, rect.top() + 12.0),
        Align2::LEFT_CENTER,
        &entry.name,
        FontId::proportional(11.0),
        theme::TEXT_DIM,
    );
    painter.text(
        pos2(rect.left() + 10.0, rect.top() + 28.0),
        Align2::LEFT_CENTER,
        format!(
            "{:.2} s  ·  {:.1} kHz  ·  {}",
            entry.duration_seconds,
            f64::from(entry.sample_rate) / 1000.0,
            channel_label(entry.channels)
        ),
        FontId::proportional(9.0),
        theme::TEXT_FAINT,
    );

    response
        .on_hover_cursor(CursorIcon::Grab)
        .on_hover_text("Double-click to place · drag into Tracks · right-click for more")
}

fn channel_label(channels: u16) -> String {
    match channels {
        1 => "mono".to_owned(),
        2 => "stereo".to_owned(),
        other => format!("{other} ch"),
    }
}
