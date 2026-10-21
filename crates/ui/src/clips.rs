use egui::{
    Align2, CornerRadius, CursorIcon, FontId, Rect, RichText, Sense, Stroke, StrokeKind, pos2, vec2,
};
use octava_core::{EditCommand, Project, Ticks};

use crate::state::{PlaybackView, UiAction, UiState};
use crate::theme;

pub(crate) fn show(
    ui: &mut egui::Ui,
    project: &Project,
    playback: &PlaybackView,
    state: &mut UiState,
    actions: &mut Vec<UiAction>,
) {
    let bar_ticks = project
        .time_signature
        .ticks_per_bar(project.tempo_map.ppq())
        .max(1);

    egui::Frame::NONE
        .fill(theme::BACKGROUND)
        .inner_margin(egui::Margin::symmetric(6, 5))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("CLIPS")
                        .size(9.5)
                        .strong()
                        .color(theme::TEXT_FAINT),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(
                            state.selected_track.is_some(),
                            egui::Button::new(RichText::new("+ Clip").size(9.5)),
                        )
                        .on_hover_text("Add a 4-bar clip at the playhead")
                        .clicked()
                        && let Some(track) = state.selected_track
                    {
                        actions.push(UiAction::Edit(EditCommand::AddClip {
                            track,
                            start: playback.playhead,
                            length: Ticks(bar_ticks.saturating_mul(4).max(1)),
                        }));
                    }
                });
            });
            ui.add_space(3.0);

            if project.tracks.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        RichText::new("No tracks yet")
                            .size(12.0)
                            .color(theme::TEXT_DIM),
                    );
                });
                return;
            }

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (index, track) in project.tracks.iter().enumerate() {
                        let selected_track = state.selected_track == Some(track.id);
                        let color = theme::track_color(index);

                        ui.horizontal(|ui| {
                            let (rect, _) = ui.allocate_exact_size(vec2(7.0, 7.0), Sense::hover());
                            ui.painter().circle_filled(rect.center(), 3.5, color);
                            if ui
                                .selectable_label(
                                    selected_track,
                                    RichText::new(&track.name).size(11.5).color(
                                        if selected_track {
                                            theme::TEXT
                                        } else {
                                            theme::TEXT_DIM
                                        },
                                    ),
                                )
                                .clicked()
                            {
                                state.select_track(project, track.id);
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new(track.clips.len().to_string())
                                            .size(9.0)
                                            .color(theme::TEXT_FAINT),
                                    );
                                },
                            );
                        });

                        if track.clips.is_empty() {
                            ui.label(
                                RichText::new("  No clips")
                                    .size(10.0)
                                    .color(theme::TEXT_FAINT),
                            );
                        }

                        for clip in &track.clips {
                            let selected = state.selected_clip == Some(clip.id)
                                && state.selected_track == Some(track.id);
                            let response = clip_card(
                                ui,
                                clip.id.get(),
                                clip.notes.len(),
                                clip.start.0,
                                clip.end().0,
                                bar_ticks,
                                color,
                                selected,
                            );
                            if response.clicked() {
                                state.select_clip(track.id, clip.id);
                            }
                            if response.double_clicked() {
                                state.select_clip(track.id, clip.id);
                                state.focus_piano_roll = true;
                            }
                        }

                        ui.add_space(5.0);
                    }
                });
        });
}

#[allow(clippy::too_many_arguments)]
fn clip_card(
    ui: &mut egui::Ui,
    clip_id: u64,
    notes: usize,
    start: u64,
    end: u64,
    bar_ticks: u64,
    color: egui::Color32,
    selected: bool,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width().max(80.0), 42.0), Sense::click());
    let painter = ui.painter();
    let fill = if selected {
        theme::ACCENT_SOFT
    } else if response.hovered() {
        theme::PANEL_RAISED
    } else {
        theme::PANEL
    };
    painter.rect_filled(rect.shrink2(vec2(1.0, 1.0)), CornerRadius::same(3), fill);
    painter.rect_stroke(
        rect.shrink2(vec2(1.0, 1.0)),
        CornerRadius::same(3),
        Stroke::new(
            1.0,
            if selected {
                theme::ACCENT
            } else {
                theme::GRID_LINE
            },
        ),
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
        color,
    );

    let bar_start = start as f64 / bar_ticks as f64 + 1.0;
    let bar_end = end as f64 / bar_ticks as f64 + 1.0;
    painter.text(
        pos2(rect.left() + 10.0, rect.top() + 10.0),
        Align2::LEFT_CENTER,
        format!("Clip {clip_id}"),
        FontId::proportional(11.0),
        if selected {
            theme::TEXT
        } else {
            theme::TEXT_DIM
        },
    );
    painter.text(
        pos2(rect.left() + 10.0, rect.top() + 28.0),
        Align2::LEFT_CENTER,
        format!("{bar_start:.1}–{bar_end:.1}  ·  {notes} notes"),
        FontId::proportional(9.0),
        theme::TEXT_FAINT,
    );

    response
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text("Click to edit · double-click for Piano Roll")
}
