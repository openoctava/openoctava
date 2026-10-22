use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Rect, RichText, Sense, Stroke, StrokeKind,
    pos2, vec2,
};
use octava_core::{EditCommand, PatternId, Project, Ticks, Track};

use crate::state::{
    ClipDrag, ClipDragKind, ClipRef, ClipboardClip, PatternDragPayload, PendingClip, PlaybackView,
    TrackDrag, UiAction, UiState,
};
use crate::theme;

const HEADER_WIDTH: f32 = 168.0;
const RULER_HEIGHT: f32 = 26.0;
const LANE_HEIGHT: f32 = 58.0;

pub(crate) fn show(
    ui: &mut egui::Ui,
    project: &Project,
    playback: &PlaybackView,
    state: &mut UiState,
    actions: &mut Vec<UiAction>,
) {
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    ui.painter_at(rect)
        .rect_filled(rect, CornerRadius::ZERO, theme::BACKGROUND);

    if rect.width() < HEADER_WIDTH + 120.0 || rect.height() < RULER_HEIGHT + 40.0 {
        return;
    }

    let bar_ticks = project
        .time_signature
        .ticks_per_bar(project.tempo_map.ppq()) as f64;
    let timeline_left = rect.left() + HEADER_WIDTH;
    let timeline_width = (rect.width() - HEADER_WIDTH).max(1.0);
    let project_bars = (project.length_ticks().0 as f64 / bar_ticks).ceil() as u64;
    let total_bars = project_bars.max(8);
    let px_per_tick = f64::from(timeline_width) / (total_bars as f64 * bar_ticks);
    let px_per_bar = px_per_tick * bar_ticks;

    let timeline_rect = Rect::from_min_max(pos2(timeline_left, rect.top()), rect.max);
    let lanes_top = rect.top() + RULER_HEIGHT;
    let max_lanes = ((rect.height() - RULER_HEIGHT) / LANE_HEIGHT).floor() as usize;
    let visible_tracks = project.tracks.len().min(max_lanes);

    handle_shortcuts(ui, project, playback, state, actions);

    if let Some(pointer) = ui.input(|input| input.pointer.latest_pos()) {
        update_clip_drag(project, state, px_per_tick, pointer.x);
        update_track_drag(project, state, pointer.y, lanes_top, visible_tracks);
    }

    let painter = ui.painter_at(rect);
    painter.rect_filled(
        Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + RULER_HEIGHT)),
        CornerRadius::ZERO,
        theme::RULER,
    );

    let bar_step = if px_per_bar >= 52.0 {
        1
    } else {
        (52.0 / px_per_bar).ceil() as u64
    };
    let mut bar = 0;
    while bar <= total_bars {
        let x = timeline_left + (bar as f64 * bar_ticks * px_per_tick) as f32;
        if x >= timeline_left - 1.0 && x <= rect.right() {
            if bar % bar_step == 0 {
                painter.text(
                    pos2(x + 5.0, rect.top() + 5.0),
                    Align2::LEFT_TOP,
                    format!("{}", bar + 1),
                    FontId::proportional(10.0),
                    theme::TEXT_DIM,
                );
            }
            let line_color = if bar % u64::from(project.time_signature.numerator) == 0 {
                theme::GRID_BAR
            } else {
                theme::GRID_LINE
            };
            painter.line_segment(
                [pos2(x, rect.top() + RULER_HEIGHT), pos2(x, rect.bottom())],
                Stroke::new(1.0, line_color),
            );
        }
        bar += 1;
    }

    if project.tracks.is_empty() {
        painter.text(
            timeline_rect.center(),
            Align2::CENTER_CENTER,
            "No tracks yet",
            FontId::proportional(14.0),
            theme::TEXT_DIM,
        );
    }

    for (index, track) in project.tracks.iter().take(visible_tracks).enumerate() {
        let lane_top = lanes_top + index as f32 * LANE_HEIGHT;
        let lane_bottom = (lane_top + LANE_HEIGHT).min(rect.bottom());
        let header_rect = Rect::from_min_max(
            pos2(rect.left(), lane_top),
            pos2(timeline_left, lane_bottom),
        );
        let header_response = ui.interact(
            header_rect,
            ui.id().with(("timeline-track", track.id.0)),
            Sense::click_and_drag(),
        );
        if header_response.clicked() {
            state.select_track(project, track.id);
        }
        if header_response.drag_started() {
            state.select_track(project, track.id);
            state.clip_drag = None;
            state.track_drag = Some(TrackDrag {
                track: track.id,
                original_index: index,
                target_index: index,
            });
        }
        header_response.context_menu(|ui| {
            let pattern = target_pattern(project, state);
            if ui
                .add_enabled(pattern.is_some(), egui::Button::new("Add Clip at Playhead"))
                .on_hover_text("Place the selected clip from the Clips panel")
                .clicked()
                && let (Some(pattern), Some(track)) = (pattern, Some(track.id))
            {
                place_clip(state, actions, track, pattern, playback.playhead);
                ui.close();
            }
            ui.separator();
            if ui
                .button(if track.mute {
                    "Unmute Track"
                } else {
                    "Mute Track"
                })
                .clicked()
            {
                actions.push(UiAction::Edit(EditCommand::SetTrackParams {
                    track: track.id,
                    gain: None,
                    pan: None,
                    mute: Some(!track.mute),
                    solo: None,
                    preset: None,
                }));
                ui.close();
            }
            if ui
                .button(if track.solo {
                    "Unsolo Track"
                } else {
                    "Solo Track"
                })
                .clicked()
            {
                actions.push(UiAction::Edit(EditCommand::SetTrackParams {
                    track: track.id,
                    gain: None,
                    pan: None,
                    mute: None,
                    solo: Some(!track.solo),
                    preset: None,
                }));
                ui.close();
            }
            ui.separator();
            if ui
                .button(RichText::new("Delete Track").color(Color32::from_rgb(222, 112, 102)))
                .clicked()
            {
                actions.push(UiAction::Edit(EditCommand::RemoveTrack { track: track.id }));
                ui.close();
            }
        });
        if state.track_drag.is_some_and(|drag| drag.track == track.id) {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        } else if header_response.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
        }
        let selected = state.selected_track == Some(track.id);
        let drop_target = state
            .track_drag
            .is_some_and(|drag| drag.target_index == index && drag.original_index != index);
        let color = theme::track_color(index);

        let row_fill = if selected {
            theme::ACCENT_SOFT
        } else if header_response.hovered() {
            theme::PANEL_RAISED
        } else if index % 2 == 0 {
            theme::PANEL
        } else {
            theme::BACKGROUND
        };
        painter.rect_filled(
            Rect::from_min_max(header_rect.min, pos2(timeline_left, lane_bottom)),
            CornerRadius::ZERO,
            row_fill,
        );
        painter.rect_filled(
            Rect::from_min_max(
                pos2(rect.left(), lane_top),
                pos2(rect.left() + 3.0, lane_bottom),
            ),
            CornerRadius::ZERO,
            color,
        );
        painter.line_segment(
            [
                pos2(rect.left(), lane_bottom),
                pos2(rect.right(), lane_bottom),
            ],
            Stroke::new(1.0, theme::GRID_LINE),
        );
        if drop_target {
            painter.rect_stroke(
                Rect::from_min_max(header_rect.min, pos2(header_rect.right(), lane_bottom)),
                CornerRadius::ZERO,
                Stroke::new(2.0, theme::ACCENT),
                StrokeKind::Inside,
            );
        }

        draw_track_header(ui, project, track, header_rect, selected);
        draw_track_clips(
            ui,
            project,
            track,
            state,
            actions,
            timeline_left,
            px_per_tick,
            lane_top,
            lane_bottom,
        );
    }

    if project.tracks.len() > visible_tracks {
        painter.text(
            pos2(rect.left() + 10.0, rect.bottom() - 6.0),
            Align2::LEFT_BOTTOM,
            format!(
                "+{} more tracks (resize the panel to show them)",
                project.tracks.len() - visible_tracks
            ),
            FontId::proportional(9.5),
            theme::TEXT_FAINT,
        );
    }

    let empty_header_rect = Rect::from_min_max(
        pos2(
            rect.left(),
            rect.top() + RULER_HEIGHT + visible_tracks as f32 * LANE_HEIGHT,
        ),
        pos2(timeline_left, rect.bottom()),
    );
    if empty_header_rect.height() > 2.0 {
        let empty_response = ui.interact(
            empty_header_rect,
            ui.id().with("timeline-empty-header"),
            Sense::click(),
        );
        empty_response.context_menu(|ui| {
            if ui.button("Add Track").clicked() {
                actions.push(UiAction::Edit(EditCommand::AddTrack {
                    name: format!("Track {}", project.tracks.len() + 1),
                }));
                ui.close();
            }
        });
    }

    let empty_ruler_rect = Rect::from_min_max(
        pos2(rect.left(), rect.top()),
        pos2(timeline_left, rect.top() + RULER_HEIGHT),
    );
    ui.interact(
        empty_ruler_rect,
        ui.id().with("timeline-empty-ruler"),
        Sense::click(),
    )
    .context_menu(|ui| {
        if ui.button("Add Track").clicked() {
            actions.push(UiAction::Edit(EditCommand::AddTrack {
                name: format!("Track {}", project.tracks.len() + 1),
            }));
            ui.close();
        }
    });

    painter.line_segment(
        [
            pos2(timeline_left, rect.top()),
            pos2(timeline_left, rect.bottom()),
        ],
        Stroke::new(1.0, theme::GRID_BAR),
    );
    painter.line_segment(
        [
            pos2(rect.left(), rect.top() + RULER_HEIGHT),
            pos2(rect.right(), rect.top() + RULER_HEIGHT),
        ],
        Stroke::new(1.0, theme::GRID_BAR),
    );

    let playhead_x = timeline_left + (playback.playhead.0 as f64 * px_per_tick) as f32;
    if playhead_x >= timeline_left && playhead_x <= rect.right() {
        painter.line_segment(
            [
                pos2(playhead_x, rect.top()),
                pos2(playhead_x, rect.bottom()),
            ],
            Stroke::new(1.5, theme::ACCENT),
        );
    }

    painter.text(
        pos2(rect.left() + 10.0, rect.top() + RULER_HEIGHT * 0.5),
        Align2::LEFT_CENTER,
        "TRACKS",
        FontId::proportional(9.0),
        theme::TEXT_FAINT,
    );

    if response.clicked()
        && let Some(pointer) = response.interact_pointer_pos()
        && pointer.y < rect.top() + RULER_HEIGHT
        && pointer.x >= timeline_left
    {
        let tick = ((pointer.x - timeline_left) as f64 / px_per_tick).max(0.0);
        actions.push(UiAction::Seek(Ticks(
            state.snap.snap_tick(tick, project.tempo_map.ppq()),
        )));
    }

    // Drop target for clips dragged out of the clips library.
    handle_pattern_drop(
        ui,
        project,
        state,
        actions,
        &response,
        rect,
        timeline_left,
        lanes_top,
        visible_tracks,
        px_per_tick,
    );

    if ui.input(|input| input.pointer.any_released()) {
        finish_clip_drag(state, actions);
        finish_track_drag(state, actions);
    }
}

fn target_pattern(project: &Project, state: &UiState) -> Option<PatternId> {
    state
        .selected_pattern
        .filter(|id| project.pattern(*id).is_some())
        .or_else(|| project.patterns.first().map(|pattern| pattern.id))
}

fn place_clip(
    state: &mut UiState,
    actions: &mut Vec<UiAction>,
    track: octava_core::TrackId,
    pattern: PatternId,
    start: Ticks,
) {
    state.pending_clip = Some(PendingClip {
        track,
        pattern,
        start,
    });
    actions.push(UiAction::Edit(EditCommand::AddClip {
        track,
        pattern,
        start,
    }));
}

fn handle_shortcuts(
    ui: &egui::Ui,
    project: &Project,
    playback: &PlaybackView,
    state: &mut UiState,
    actions: &mut Vec<UiAction>,
) {
    if ui.ctx().egui_wants_keyboard_input() {
        return;
    }

    let (copy, paste, duplicate) = ui.input(|input| {
        let command = input.modifiers.command;
        (
            command && input.key_pressed(egui::Key::C),
            command && input.key_pressed(egui::Key::V),
            command && input.key_pressed(egui::Key::D),
        )
    });

    if copy
        && let Some(selection) = state.selected_clip
        && let Some(clip) = project
            .track(selection.track)
            .and_then(|track| track.clip(selection.clip))
    {
        state.clipboard = Some(ClipboardClip {
            pattern: clip.pattern,
            length: clip.length,
        });
    }

    if paste
        && let Some(entry) = state.clipboard
        && let Some(track) = state.selected_track
    {
        // Paste at the playhead so that repeated Ctrl+V lays clips out in a row.
        let start = Ticks(
            state
                .snap
                .snap_tick(playback.playhead.0 as f64, project.tempo_map.ppq()),
        );
        place_clip(state, actions, track, entry.pattern, start);
    }

    if duplicate
        && let Some(selection) = state.selected_clip
        && let Some(clip) = project
            .track(selection.track)
            .and_then(|track| track.clip(selection.clip))
    {
        // Duplicate lands immediately after the original.
        let start = clip.end();
        place_clip(state, actions, selection.track, clip.pattern, start);
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_pattern_drop(
    ui: &egui::Ui,
    project: &Project,
    state: &mut UiState,
    actions: &mut Vec<UiAction>,
    response: &egui::Response,
    rect: Rect,
    timeline_left: f32,
    lanes_top: f32,
    visible_tracks: usize,
    px_per_tick: f64,
) {
    // `dnd_release_payload` consumes the payload, so it may only be read once;
    // `dnd_hover_payload` is the non-destructive variant used for the ghost.
    let released = response.dnd_release_payload::<PatternDragPayload>();
    let hovered = response.dnd_hover_payload::<PatternDragPayload>();
    let Some(payload) = released.as_ref().or(hovered.as_ref()) else {
        return;
    };
    let Some(pattern) = project.pattern(payload.pattern) else {
        return;
    };
    let Some(pointer) = ui.input(|input| input.pointer.interact_pos()) else {
        return;
    };
    if pointer.x < timeline_left || pointer.y < lanes_top {
        return;
    }

    let lane = ((pointer.y - lanes_top) / LANE_HEIGHT).floor();
    if lane < 0.0 || lane as usize >= visible_tracks {
        return;
    }
    let Some(track) = project.tracks.get(lane as usize) else {
        return;
    };
    let tick = ((pointer.x - timeline_left) as f64 / px_per_tick).max(0.0);
    let start = Ticks(state.snap.snap_tick(tick, project.tempo_map.ppq()));

    // Ghost preview of the clip that would be created.
    let x0 = timeline_left + (start.0 as f64 * px_per_tick) as f32;
    let x1 = timeline_left + ((start.0 + pattern.length.0) as f64 * px_per_tick) as f32;
    let lane_top = lanes_top + lane * LANE_HEIGHT;
    let ghost = Rect::from_min_max(
        pos2(x0, lane_top + 7.0),
        pos2(x1.max(x0 + 4.0), lane_top + LANE_HEIGHT - 7.0),
    );
    let clip_painter = ui.painter_at(Rect::from_min_max(
        rect.min,
        pos2(rect.right(), rect.bottom()),
    ));
    clip_painter.rect_filled(
        ghost,
        CornerRadius::same(3),
        theme::ACCENT.gamma_multiply(0.18),
    );
    clip_painter.rect_stroke(
        ghost,
        CornerRadius::same(3),
        Stroke::new(1.5, theme::ACCENT),
        StrokeKind::Inside,
    );
    clip_painter.text(
        pos2(ghost.left() + 6.0, ghost.top() + 8.0),
        Align2::LEFT_CENTER,
        &pattern.name,
        FontId::proportional(9.5),
        theme::ACCENT,
    );

    if released.is_some() {
        place_clip(state, actions, track.id, pattern.id, start);
    }
}

fn update_track_drag(
    project: &Project,
    state: &mut UiState,
    pointer_y: f32,
    lanes_top: f32,
    visible_tracks: usize,
) {
    let Some(drag) = state.track_drag else {
        return;
    };
    if project.track(drag.track).is_none() || visible_tracks == 0 {
        state.track_drag = None;
        return;
    }

    let lane = ((pointer_y - lanes_top) / LANE_HEIGHT).floor();
    let target = lane.clamp(0.0, (visible_tracks - 1) as f32) as usize;
    state.track_drag = Some(TrackDrag {
        target_index: target,
        ..drag
    });
}

fn finish_track_drag(state: &mut UiState, actions: &mut Vec<UiAction>) {
    let Some(drag) = state.track_drag.take() else {
        return;
    };
    if drag.target_index != drag.original_index {
        actions.push(UiAction::Edit(EditCommand::MoveTrack {
            track: drag.track,
            to_index: drag.target_index,
        }));
    }
}

fn update_clip_drag(project: &Project, state: &mut UiState, px_per_tick: f64, pointer_x: f32) {
    let Some(drag) = state.clip_drag else {
        return;
    };
    let exists = project
        .track(drag.track)
        .and_then(|track| track.clip(drag.clip))
        .is_some();
    if !exists {
        state.clip_drag = None;
        return;
    }

    let delta = (pointer_x - drag.pointer_start_x) as f64 / px_per_tick;
    let next = match drag.kind {
        ClipDragKind::Move => {
            let start = state.snap.snap_tick(
                drag.original_start.0 as f64 + delta,
                project.tempo_map.ppq(),
            );
            ClipDrag {
                preview_start: Ticks(start),
                preview_length: drag.original_length,
                ..drag
            }
        }
        ClipDragKind::ResizeEnd => {
            let original_end = drag.original_start.0 + drag.original_length.0;
            let min_end = drag.original_start.0.saturating_add(1);
            let end = state
                .snap
                .snap_tick(original_end as f64 + delta, project.tempo_map.ppq())
                .max(min_end);
            ClipDrag {
                preview_start: drag.original_start,
                preview_length: Ticks(end - drag.original_start.0),
                ..drag
            }
        }
    };
    state.clip_drag = Some(next);
}

fn finish_clip_drag(state: &mut UiState, actions: &mut Vec<UiAction>) {
    let Some(drag) = state.clip_drag.take() else {
        return;
    };
    match drag.kind {
        ClipDragKind::Move => {
            if drag.preview_start != drag.original_start {
                actions.push(UiAction::Edit(EditCommand::MoveClip {
                    track: drag.track,
                    clip: drag.clip,
                    start: drag.preview_start,
                }));
            }
        }
        ClipDragKind::ResizeEnd => {
            if drag.preview_length != drag.original_length {
                actions.push(UiAction::Edit(EditCommand::ResizeClip {
                    track: drag.track,
                    clip: drag.clip,
                    length: drag.preview_length,
                }));
            }
        }
    }
}

fn draw_track_header(ui: &egui::Ui, project: &Project, track: &Track, rect: Rect, selected: bool) {
    let painter = ui.painter_at(rect);
    painter.text(
        pos2(rect.left() + 12.0, rect.top() + 9.0),
        Align2::LEFT_TOP,
        &track.name,
        FontId::proportional(12.0),
        if selected {
            theme::TEXT
        } else {
            theme::TEXT_DIM
        },
    );
    painter.text(
        pos2(rect.left() + 12.0, rect.top() + 28.0),
        Align2::LEFT_TOP,
        format!(
            "Ch {}  ·  {} notes",
            u16::from(track.channel) + 1,
            project.track_note_count(track.id)
        ),
        FontId::proportional(9.5),
        theme::TEXT_FAINT,
    );

    let mut badge_x = rect.right() - 10.0;
    if track.solo {
        draw_badge(&painter, &mut badge_x, rect.top() + 8.0, "S", theme::ACCENT);
    }
    if track.mute {
        draw_badge(
            &painter,
            &mut badge_x,
            rect.top() + 8.0,
            "M",
            Color32::from_rgb(220, 100, 90),
        );
    }
}

fn draw_badge(painter: &egui::Painter, x: &mut f32, y: f32, text: &str, color: Color32) {
    let rect = Rect::from_min_size(pos2(*x - 16.0, y), vec2(16.0, 13.0));
    painter.rect_filled(rect, CornerRadius::same(2), color.gamma_multiply(0.25));
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::monospace(9.0),
        color,
    );
    *x -= 20.0;
}

#[allow(clippy::too_many_arguments)]
fn draw_track_clips(
    ui: &egui::Ui,
    project: &Project,
    track: &Track,
    state: &mut UiState,
    actions: &mut Vec<UiAction>,
    timeline_left: f32,
    px_per_tick: f64,
    lane_top: f32,
    lane_bottom: f32,
) {
    let clip_painter = ui.painter_at(Rect::from_min_max(
        pos2(timeline_left, lane_top),
        pos2(ui.max_rect().right(), lane_bottom),
    ));

    for clip in &track.clips {
        let Some(pattern) = project.pattern(clip.pattern) else {
            continue;
        };
        let color = theme::pattern_color(project.pattern_index(pattern.id).unwrap_or(0));
        let dragging = state
            .clip_drag
            .filter(|drag| drag.track == track.id && drag.clip == clip.id);
        let display_start = dragging.map_or(clip.start.0, |drag| drag.preview_start.0);
        let display_length = dragging.map_or(clip.length.0, |drag| drag.preview_length.0);
        let x0 = timeline_left + (display_start as f64 * px_per_tick) as f32;
        let x1 = timeline_left + ((display_start + display_length) as f64 * px_per_tick) as f32;
        let clip_rect = Rect::from_min_max(
            pos2(x0.max(timeline_left), lane_top + 7.0),
            pos2(x1.max(x0 + 2.0), lane_bottom - 7.0),
        );
        if clip_rect.right() < timeline_left {
            continue;
        }

        let response = ui.interact(
            clip_rect,
            ui.id().with(("timeline-clip", clip.id.0)),
            Sense::click_and_drag(),
        );
        if response.clicked() {
            state.select_clip(project, track.id, clip.id);
        }
        if response.drag_started() {
            let origin = ui
                .input(|input| input.pointer.press_origin())
                .unwrap_or_else(|| clip_rect.center());
            let edge_zone = (clip_rect.width() * 0.12).clamp(5.0, 9.0);
            let kind = if clip_rect.width() > 16.0 && origin.x >= clip_rect.right() - edge_zone {
                ClipDragKind::ResizeEnd
            } else {
                ClipDragKind::Move
            };
            state.select_clip(project, track.id, clip.id);
            state.track_drag = None;
            state.clip_drag = Some(ClipDrag {
                track: track.id,
                clip: clip.id,
                kind,
                pointer_start_x: origin.x,
                original_start: clip.start,
                original_length: clip.length,
                preview_start: clip.start,
                preview_length: clip.length,
            });
        }
        if let Some(drag) = dragging {
            ui.ctx().set_cursor_icon(match drag.kind {
                ClipDragKind::Move => CursorIcon::Grabbing,
                ClipDragKind::ResizeEnd => CursorIcon::ResizeHorizontal,
            });
        } else if response.hovered() {
            let edge_zone = (clip_rect.width() * 0.12).clamp(5.0, 9.0);
            let near_end = clip_rect.width() > 16.0
                && ui
                    .input(|input| input.pointer.hover_pos())
                    .is_some_and(|pointer| pointer.x >= clip_rect.right() - edge_zone);
            ui.ctx().set_cursor_icon(if near_end {
                CursorIcon::ResizeHorizontal
            } else {
                CursorIcon::Grab
            });
        }

        let selection = ClipRef {
            track: track.id,
            clip: clip.id,
        };
        let selected = state.selected_clip == Some(selection);

        response.context_menu(|ui| {
            if ui.button("Copy").on_hover_text("Ctrl+C").clicked() {
                state.clipboard = Some(ClipboardClip {
                    pattern: clip.pattern,
                    length: clip.length,
                });
                ui.close();
            }
            if ui
                .button("Duplicate")
                .on_hover_text("Insert a copy right after this clip (Ctrl+D)")
                .clicked()
            {
                let start = clip.end();
                place_clip(state, actions, track.id, clip.pattern, start);
                ui.close();
            }
            ui.separator();
            if ui
                .button(RichText::new("Delete").color(Color32::from_rgb(222, 112, 102)))
                .clicked()
            {
                actions.push(UiAction::Edit(EditCommand::RemoveClip {
                    track: track.id,
                    clip: clip.id,
                }));
                ui.close();
            }
        });

        clip_painter.rect_filled(clip_rect, CornerRadius::same(3), color.gamma_multiply(0.24));
        clip_painter.rect_filled(
            Rect::from_min_max(
                clip_rect.min,
                pos2(clip_rect.right(), clip_rect.top() + 16.0),
            ),
            CornerRadius {
                nw: 3,
                ne: 3,
                sw: 0,
                se: 0,
            },
            color.gamma_multiply(0.52),
        );
        clip_painter.text(
            pos2(clip_rect.left() + 6.0, clip_rect.top() + 8.0),
            Align2::LEFT_CENTER,
            &pattern.name,
            FontId::proportional(9.5),
            color,
        );
        clip_painter.text(
            pos2(clip_rect.left() + 4.0, clip_rect.bottom() - 8.0),
            Align2::LEFT_BOTTOM,
            format!("{} notes", pattern.notes.len()),
            FontId::proportional(9.0),
            theme::TEXT_DIM,
        );

        if selected {
            clip_painter.rect_stroke(
                clip_rect,
                CornerRadius::same(3),
                Stroke::new(1.0, theme::ACCENT),
                StrokeKind::Inside,
            );
        }
    }
}
