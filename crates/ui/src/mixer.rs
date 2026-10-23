use egui::{
    Color32, CornerRadius, CursorIcon, Rect, Response, RichText, Sense, Stroke, StrokeKind, pos2,
    vec2,
};
use octava_core::{EditCommand, Project, Track, TrackId};

use crate::state::{MixerDrag, MixerParam, PlaybackView, UiAction, UiState};
use crate::theme;

const STRIP_WIDTH: f32 = 132.0;
const MIN_FADER_HEIGHT: f32 = 84.0;
/// Height reserved for the caption pinned below the fader.
const STRIP_FOOTER: f32 = 18.0;

pub(crate) fn show(
    ui: &mut egui::Ui,
    project: &Project,
    playback: &PlaybackView,
    state: &mut UiState,
    actions: &mut Vec<UiAction>,
) {
    let mut add_track = false;
    egui::Frame::NONE
        .fill(theme::BACKGROUND)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("MIXER")
                        .size(9.5)
                        .strong()
                        .color(theme::TEXT_FAINT),
                );
                ui.label(
                    RichText::new(format!(
                        "{} tracks  ·  {} Hz",
                        project.tracks.len(),
                        playback.audio_sample_rate.unwrap_or(0)
                    ))
                    .size(9.5)
                    .color(theme::TEXT_FAINT),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("+ Track").clicked() {
                        add_track = true;
                    }
                });
            });
            ui.add_space(4.0);

            if project.tracks.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        RichText::new("Add a track to open the mixer")
                            .size(13.0)
                            .color(theme::TEXT_DIM),
                    );
                });
                return;
            }

            let height = ui.available_height().max(180.0);
            egui::ScrollArea::horizontal()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_min_height(height);
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        for (index, track) in project.tracks.iter().enumerate() {
                            strip(ui, project, track, index, height, state, actions);
                        }
                    });
                });
        });

    if add_track {
        actions.push(UiAction::Edit(EditCommand::AddTrack {
            name: format!("Track {}", project.tracks.len() + 1),
        }));
    }

    if ui.input(|input| input.pointer.any_released()) {
        finish_mixer_drag(project, state, actions);
    }
}

fn strip(
    ui: &mut egui::Ui,
    project: &Project,
    track: &Track,
    index: usize,
    height: f32,
    state: &mut UiState,
    actions: &mut Vec<UiAction>,
) {
    let color = theme::track_color(index);
    let selected = state.selected_track == Some(track.id);
    let fill = if selected {
        theme::ACCENT_SOFT
    } else {
        theme::PANEL
    };
    let content_height = (height - 12.0).max(140.0);

    egui::Frame::NONE
        .fill(fill)
        .stroke(Stroke::new(
            1.0,
            if selected {
                theme::ACCENT
            } else {
                theme::GRID_LINE
            },
        ))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(STRIP_WIDTH);
            ui.set_height(content_height);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(7.0, 7.0), Sense::hover());
                    ui.painter().circle_filled(rect.center(), 3.5, color);
                    if ui
                        .selectable_label(
                            selected,
                            RichText::new(&track.name).size(11.5).color(if selected {
                                theme::TEXT
                            } else {
                                theme::TEXT_DIM
                            }),
                        )
                        .clicked()
                    {
                        state.select_track(project, track.id);
                    }
                });
                ui.label(
                    RichText::new(format!(
                        "Ch {}  ·  program {}",
                        u16::from(track.channel) + 1,
                        track.preset.program
                    ))
                    .size(9.0)
                    .color(theme::TEXT_FAINT),
                );
                ui.add_space(3.0);

                let mut mute = track.mute;
                let mut solo = track.solo;
                ui.horizontal(|ui| {
                    if ui
                        .add_sized(
                            [30.0, 20.0],
                            egui::Button::new(RichText::new("M").size(10.0)).fill(if mute {
                                Color32::from_rgb(120, 52, 48)
                            } else {
                                theme::PANEL_RAISED
                            }),
                        )
                        .on_hover_text("Mute")
                        .clicked()
                    {
                        mute = !mute;
                        actions.push(track_params(track.id, None, None, Some(mute), None));
                    }
                    if ui
                        .add_sized(
                            [30.0, 20.0],
                            egui::Button::new(RichText::new("S").size(10.0)).fill(if solo {
                                theme::ACCENT_SOFT
                            } else {
                                theme::PANEL_RAISED
                            }),
                        )
                        .on_hover_text("Solo")
                        .clicked()
                    {
                        solo = !solo;
                        actions.push(track_params(track.id, None, None, None, Some(solo)));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{} notes", project.track_note_count(track.id)))
                                .size(8.5)
                                .color(theme::TEXT_FAINT),
                        );
                    });
                });

                ui.add_space(6.0);
                let fader_height = (ui.available_height() - STRIP_FOOTER).max(MIN_FADER_HEIGHT);
                ui.horizontal_top(|ui| {
                    let gain = current_value(track.id, MixerParam::Gain, track.gain, state);
                    let (response, new_gain) = fader(ui, gain, color, fader_height);
                    if response.drag_started() {
                        begin_drag(state, track.id, MixerParam::Gain, gain);
                    } else if response.dragged() {
                        update_drag(state, MixerParam::Gain, new_gain);
                    }
                    response.on_hover_text("Gain");
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        let pan = current_value(track.id, MixerParam::Pan, track.pan, state);
                        ui.label(
                            RichText::new(gain_label(gain))
                                .monospace()
                                .size(10.0)
                                .color(theme::TEXT),
                        );
                        ui.add_space(2.0);
                        let (response, new_pan) = pan_slider(ui, pan, color);
                        if response.drag_started() {
                            begin_drag(state, track.id, MixerParam::Pan, pan);
                        } else if response.dragged() {
                            update_drag(state, MixerParam::Pan, new_pan);
                        }
                        response.on_hover_text("Pan");
                        ui.label(
                            RichText::new(pan_label(pan))
                                .monospace()
                                .size(9.0)
                                .color(theme::TEXT_DIM),
                        );
                    });
                });

                ui.add_space((ui.available_height() - STRIP_FOOTER).max(0.0));
                ui.label(
                    RichText::new(format!("{:.1} bars", clip_span_bars(project, track)))
                        .size(8.5)
                        .color(theme::TEXT_FAINT),
                );
            });
        });
}

fn current_value(track: TrackId, param: MixerParam, stored: f32, state: &UiState) -> f32 {
    state
        .mixer_drag
        .filter(|drag| drag.track == track && drag.param == param)
        .map_or(stored, |drag| drag.preview)
}

fn begin_drag(state: &mut UiState, track: TrackId, param: MixerParam, value: f32) {
    state.mixer_drag = Some(MixerDrag {
        track,
        param,
        original: value,
        preview: value,
    });
}

fn update_drag(state: &mut UiState, param: MixerParam, value: f32) {
    let Some(drag) = state.mixer_drag else {
        return;
    };
    if drag.param != param {
        return;
    }
    state.mixer_drag = Some(MixerDrag {
        preview: value,
        ..drag
    });
}

fn finish_mixer_drag(project: &Project, state: &mut UiState, actions: &mut Vec<UiAction>) {
    let Some(drag) = state.mixer_drag.take() else {
        return;
    };
    if project.track(drag.track).is_none() || (drag.preview - drag.original).abs() < 0.001 {
        return;
    }

    match drag.param {
        MixerParam::Gain => actions.push(track_params(
            drag.track,
            Some(drag.preview.clamp(0.0, 1.0)),
            None,
            None,
            None,
        )),
        MixerParam::Pan => actions.push(track_params(
            drag.track,
            None,
            Some(drag.preview.clamp(-1.0, 1.0)),
            None,
            None,
        )),
    }
}

fn track_params(
    track: TrackId,
    gain: Option<f32>,
    pan: Option<f32>,
    mute: Option<bool>,
    solo: Option<bool>,
) -> UiAction {
    UiAction::Edit(EditCommand::SetTrackParams {
        track,
        gain,
        pan,
        mute,
        solo,
        preset: None,
    })
}

fn fader(ui: &mut egui::Ui, value: f32, color: Color32, height: f32) -> (Response, f32) {
    let (rect, response) = ui.allocate_exact_size(vec2(24.0, height), Sense::click_and_drag());
    let painter = ui.painter();
    let rail = Rect::from_center_size(rect.center(), vec2(5.0, rect.height() - 8.0));
    painter.rect_filled(rail, CornerRadius::same(2), theme::BACKGROUND);
    painter.rect_stroke(
        rail,
        CornerRadius::same(2),
        Stroke::new(1.0, theme::GRID_LINE),
        StrokeKind::Inside,
    );

    let normalized = value.clamp(0.0, 1.0).powf(0.65);
    let y = rail.bottom() - normalized * rail.height();
    painter.rect_filled(
        Rect::from_min_max(rail.min, pos2(rail.right(), y)),
        CornerRadius::same(2),
        color.gamma_multiply(0.45),
    );
    let handle = Rect::from_center_size(pos2(rail.center().x, y), vec2(rect.width() - 2.0, 8.0));
    painter.rect_filled(handle, CornerRadius::same(2), color);
    painter.rect_stroke(
        handle,
        CornerRadius::same(2),
        Stroke::new(1.0, theme::TEXT),
        StrokeKind::Inside,
    );
    let next = if response.dragged() {
        let pointer_delta = ui.input(|input| input.pointer.delta()).y;
        let current = ui
            .data(|data| data.get_temp::<f32>(ui.id().with("fader-value")))
            .unwrap_or(value);
        let next = (current - pointer_delta / (rail.height() * 0.65)).clamp(0.0, 1.0);
        ui.data_mut(|data| data.insert_temp(ui.id().with("fader-value"), next));
        next
    } else {
        ui.data_mut(|data| data.remove::<f32>(ui.id().with("fader-value")));
        value
    };

    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
    }
    (response, next)
}

fn pan_slider(ui: &mut egui::Ui, value: f32, color: Color32) -> (Response, f32) {
    let (rect, response) = ui.allocate_exact_size(vec2(76.0, 18.0), Sense::click_and_drag());
    let painter = ui.painter();
    let rail = Rect::from_center_size(rect.center(), vec2(rect.width() - 4.0, 4.0));
    painter.rect_filled(rail, CornerRadius::same(2), theme::BACKGROUND);
    let center_x = rail.center().x;
    let handle_x = center_x + value.clamp(-1.0, 1.0) * rail.width() * 0.5;
    painter.line_segment(
        [
            pos2(center_x, rail.top() - 2.0),
            pos2(center_x, rail.bottom() + 2.0),
        ],
        Stroke::new(1.0, theme::TEXT_FAINT),
    );
    painter.line_segment(
        [
            pos2(center_x, rail.center().y),
            pos2(handle_x, rail.center().y),
        ],
        Stroke::new(2.0, color),
    );
    painter.circle_filled(pos2(handle_x, rail.center().y), 4.0, color);

    let next = if response.dragged() {
        ui.input(|input| input.pointer.interact_pos())
            .map_or(value, |pointer| {
                ((pointer.x - center_x) / (rail.width() * 0.5)).clamp(-1.0, 1.0)
            })
    } else {
        value
    };

    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
    (response, next)
}

fn gain_label(gain: f32) -> String {
    if gain <= 0.001 {
        "-inf dB".to_owned()
    } else {
        format!("{:.1} dB", 20.0 * gain.log10())
    }
}

fn pan_label(pan: f32) -> String {
    if pan.abs() < 0.005 {
        "C".to_owned()
    } else if pan < 0.0 {
        format!("L{:.0}", -pan * 100.0)
    } else {
        format!("R{:.0}", pan * 100.0)
    }
}

fn clip_span_bars(project: &Project, track: &Track) -> f32 {
    let bar_ticks = project
        .time_signature
        .ticks_per_bar(project.tempo_map.ppq())
        .max(1) as f32;
    let end = track
        .clips
        .iter()
        .map(octava_core::Clip::end)
        .max()
        .unwrap_or_default();
    end.0 as f32 / bar_ticks
}
