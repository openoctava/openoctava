use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Rect, Response, RichText, Sense, Stroke,
    StrokeKind, Vec2, pos2, vec2,
};
use octava_core::{EditCommand, MidiClip, Project, Ticks, Track};

use crate::state::{
    NoteDrag, NoteDragKind, NoteRef, PlaybackView, SnapDivision, SnapSettings, UiAction, UiState,
};
use crate::theme;

const GUTTER_WIDTH: f32 = 58.0;
const RULER_HEIGHT: f32 = 24.0;
const CLIP_HEADER_HEIGHT: f32 = 17.0;
const TOOLBAR_HEIGHT: f32 = 28.0;
const MIN_PX_PER_BEAT: f32 = 26.0;
const MAX_PX_PER_BEAT: f32 = 1200.0;

pub(crate) fn show(
    ui: &mut egui::Ui,
    project: &Project,
    playback: &PlaybackView,
    state: &mut UiState,
    actions: &mut Vec<UiAction>,
) {
    toolbar(ui, state, actions);
    ui.add_space(2.0);

    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    ui.painter_at(rect)
        .rect_filled(rect, CornerRadius::ZERO, theme::BACKGROUND);

    if rect.width() < GUTTER_WIDTH + 80.0 || rect.height() < RULER_HEIGHT + 80.0 {
        return;
    }

    let Some(track) = state.selected_track(project) else {
        ui.painter_at(rect).text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Add a track to open the piano roll",
            FontId::proportional(14.0),
            theme::TEXT_DIM,
        );
        return;
    };

    let grid_top = rect.top() + RULER_HEIGHT;
    let grid_left = rect.left() + GUTTER_WIDTH;
    let grid_rect = Rect::from_min_max(pos2(grid_left, grid_top), rect.max);

    let grid = Grid {
        rect: grid_rect,
        left_tick: state.piano_roll.left_tick,
        px_per_tick: f64::from(state.piano_roll.px_per_beat) / f64::from(project.tempo_map.ppq()),
        scroll_y: state.piano_roll.scroll_y,
        key_height: state.piano_roll.key_height,
    };
    let track_index = project.track_index(track.id).unwrap_or(0);

    handle_input(ui, project, track, state, &response, &grid, actions);

    draw_key_rows(ui, &grid, rect);
    draw_clips(ui, &grid, track, track_index, state.selected_clip);
    draw_grid_lines(ui, project, &grid, rect, state.snap);
    draw_notes(ui, &grid, track, track_index, state);
    draw_ruler(ui, project, &grid, rect);
    draw_playhead(ui, &grid, playback);

    if track.clips.is_empty() {
        ui.painter_at(grid.rect).text(
            grid.rect.center(),
            Align2::CENTER_CENTER,
            "Add a clip in the timeline to start composing",
            FontId::proportional(13.0),
            theme::TEXT_FAINT,
        );
    }

    ui.painter_at(rect).text(
        pos2(rect.right() - 8.0, rect.bottom() - 6.0),
        Align2::RIGHT_BOTTOM,
        "click: add note  ·  drag: move  ·  drag edge: resize  ·  Del: delete  ·  Ctrl+wheel: zoom",
        FontId::proportional(9.5),
        theme::TEXT_FAINT,
    );

    if ui.input(|input| input.pointer.any_released()) {
        finish_note_drag(state, actions);
    }
}

fn toolbar(ui: &mut egui::Ui, state: &mut UiState, actions: &mut Vec<UiAction>) {
    egui::Frame::NONE
        .fill(theme::PANEL)
        .inner_margin(egui::Margin::symmetric(8, 3))
        .stroke(Stroke::new(1.0, theme::GRID_LINE))
        .show(ui, |ui| {
            ui.set_height(TOOLBAR_HEIGHT - 6.0);
            ui.horizontal_centered(|ui| {
                let magnet = tool_icon_button(
                    ui,
                    ToolIcon::Magnet,
                    state.snap.enabled,
                    true,
                    "Snap notes and clips to the grid",
                );
                if magnet.clicked() {
                    state.snap.enabled = !state.snap.enabled;
                }

                ui.add_space(4.0);
                ui.label(
                    RichText::new("GRID")
                        .size(9.0)
                        .strong()
                        .color(theme::TEXT_FAINT),
                );
                egui::ComboBox::from_id_salt("piano-roll-snap")
                    .selected_text(state.snap.division.label())
                    .width(58.0)
                    .show_ui(ui, |ui| {
                        for division in SnapDivision::ALL {
                            ui.selectable_value(
                                &mut state.snap.division,
                                division,
                                division.label(),
                            );
                        }
                    });

                ui.separator();
                let eraser = tool_icon_button(
                    ui,
                    ToolIcon::Eraser,
                    false,
                    state.selected_note.is_some(),
                    "Delete the selected note",
                );
                if eraser.clicked() {
                    delete_selected_note(state, actions);
                }
                tool_icon_button(
                    ui,
                    ToolIcon::Quantize,
                    false,
                    false,
                    "Quantize (placeholder)",
                );
                tool_icon_button(
                    ui,
                    ToolIcon::Velocity,
                    false,
                    false,
                    "Velocity editing (placeholder)",
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new("Click grid to draw  ·  drag notes to move")
                            .size(9.5)
                            .color(theme::TEXT_FAINT),
                    );
                });
            });
        });
}

#[allow(clippy::too_many_arguments)]
fn handle_input(
    ui: &egui::Ui,
    project: &Project,
    track: &Track,
    state: &mut UiState,
    response: &Response,
    grid: &Grid,
    actions: &mut Vec<UiAction>,
) {
    let key_height = state.piano_roll.key_height;
    let max_scroll = (128.0 * key_height - grid.rect.height()).max(0.0);
    let bar_ticks = project
        .time_signature
        .ticks_per_bar(project.tempo_map.ppq()) as f64;
    let max_left = project.length_ticks().0 as f64 + bar_ticks * 16.0;

    if response.hovered() {
        let (scroll, zoom, shift, pointer) = ui.input(|input| {
            (
                input.smooth_scroll_delta,
                input.zoom_delta(),
                input.modifiers.shift,
                input.pointer.hover_pos(),
            )
        });

        if (zoom - 1.0).abs() > f32::EPSILON {
            let old_px_per_beat = state.piano_roll.px_per_beat;
            let new_px_per_beat = (old_px_per_beat * zoom).clamp(MIN_PX_PER_BEAT, MAX_PX_PER_BEAT);
            let ppq = f32::from(project.tempo_map.ppq());

            if let Some(pointer) = pointer.filter(|pointer| pointer.x >= grid.rect.left()) {
                let old_px_per_tick = old_px_per_beat / ppq;
                let new_px_per_tick = new_px_per_beat / ppq;
                let anchor_tick = state.piano_roll.left_tick
                    + f64::from((pointer.x - grid.rect.left()) / old_px_per_tick);
                state.piano_roll.left_tick =
                    anchor_tick - f64::from((pointer.x - grid.rect.left()) / new_px_per_tick);
            }
            state.piano_roll.px_per_beat = new_px_per_beat;
        }

        if scroll != Vec2::ZERO {
            let px_per_tick = (f64::from(state.piano_roll.px_per_beat)
                / f64::from(project.tempo_map.ppq())) as f32;
            if shift {
                state.piano_roll.left_tick -= f64::from(scroll.y / px_per_tick);
            } else {
                state.piano_roll.left_tick -= f64::from(scroll.x / px_per_tick);
                state.piano_roll.scroll_y -= scroll.y;
            }
        }
    }

    state.piano_roll.left_tick = state.piano_roll.left_tick.clamp(0.0, max_left);
    state.piano_roll.scroll_y = state.piano_roll.scroll_y.clamp(0.0, max_scroll);

    if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
        state.note_drag = None;
    }

    if response.drag_started()
        && let Some(origin) = ui.input(|input| input.pointer.press_origin())
        && grid.rect.contains(origin)
        && let Some(hit) = hit_test_note(track, grid, origin)
    {
        let edge_zone = (hit.rect.width() * 0.25).clamp(4.0, 10.0);
        let kind = if hit.rect.width() > 12.0 && origin.x >= hit.rect.right() - edge_zone {
            NoteDragKind::Resize
        } else {
            NoteDragKind::Move
        };
        state.select_clip(hit.note.track, hit.note.clip);
        state.selected_note = Some(hit.note);
        state.note_drag = Some(NoteDrag {
            note: hit.note,
            kind,
            pointer_start: origin,
            original_start: Ticks(hit.start),
            original_length: Ticks(hit.length),
            original_key: hit.key,
            preview_start: Ticks(hit.start),
            preview_length: Ticks(hit.length),
            preview_key: hit.key,
        });
    }

    if state.note_drag.is_some()
        && let Some(pointer) = ui.input(|input| input.pointer.latest_pos())
    {
        update_note_drag(project, grid, state, pointer);
    }

    if response.hovered()
        && let Some(pointer) = ui.input(|input| input.pointer.hover_pos())
    {
        if let Some(drag) = state.note_drag {
            ui.ctx().set_cursor_icon(match drag.kind {
                NoteDragKind::Resize => CursorIcon::ResizeHorizontal,
                NoteDragKind::Move => CursorIcon::Grabbing,
            });
        } else if grid.rect.contains(pointer)
            && let Some(hit) = hit_test_note(track, grid, pointer)
        {
            let edge_zone = (hit.rect.width() * 0.25).clamp(4.0, 10.0);
            let cursor = if hit.rect.width() > 12.0 && pointer.x >= hit.rect.right() - edge_zone {
                CursorIcon::ResizeHorizontal
            } else {
                CursorIcon::Grab
            };
            ui.ctx().set_cursor_icon(cursor);
        }
    }

    if response.clicked()
        && let Some(pointer) = response.interact_pointer_pos()
    {
        if pointer.y < grid.rect.top() && pointer.x > grid.rect.left() {
            let tick = grid.x_to_tick(pointer.x).max(0.0);
            actions.push(UiAction::Seek(Ticks(
                state.snap.snap_tick(tick, project.tempo_map.ppq()),
            )));
            return;
        }

        if !grid.rect.contains(pointer) {
            return;
        }

        if let Some(hit) = hit_test_note(track, grid, pointer) {
            state.select_clip(hit.note.track, hit.note.clip);
            state.selected_note = Some(hit.note);
            return;
        }

        if response.double_clicked() {
            return;
        }

        let tick = state
            .snap
            .snap_tick(grid.x_to_tick(pointer.x), project.tempo_map.ppq());
        if let Some(clip) = clip_at_tick(track, tick)
            && tick >= clip.start.0
            && tick < clip.end().0
        {
            let start = tick - clip.start.0;
            let available = clip.length.0.saturating_sub(start);
            let length = state
                .snap
                .division
                .ticks(project.tempo_map.ppq())
                .min(available);
            if length > 0 {
                let key = key_at_y(grid, pointer.y);
                state.select_clip(track.id, clip.id);
                state.selected_note = None;
                actions.push(UiAction::Edit(EditCommand::AddNote {
                    track: track.id,
                    clip: clip.id,
                    key,
                    velocity: 100,
                    start: Ticks(start),
                    length: Ticks(length),
                }));
            }
        }
    }

    if response.hovered()
        && !ui.ctx().egui_wants_keyboard_input()
        && ui.input(|input| {
            input.key_pressed(egui::Key::Delete) || input.key_pressed(egui::Key::Backspace)
        })
    {
        delete_selected_note(state, actions);
    }
}

fn update_note_drag(project: &Project, grid: &Grid, state: &mut UiState, pointer: egui::Pos2) {
    let Some(drag) = state.note_drag else {
        return;
    };
    let Some(track) = project.track(drag.note.track) else {
        state.note_drag = None;
        return;
    };
    let Some(clip) = track.clip(drag.note.clip) else {
        state.note_drag = None;
        return;
    };
    if clip.note(drag.note.note).is_none() {
        state.note_drag = None;
        return;
    }

    let delta_ticks = f64::from(pointer.x - drag.pointer_start.x) / grid.px_per_tick;
    let clip_start = clip.start.0;
    let clip_end = clip.end().0;

    let mut next = drag;
    match drag.kind {
        NoteDragKind::Move => {
            let original_global = clip_start as f64 + drag.original_start.0 as f64;
            let max_start = clip_end
                .saturating_sub(drag.original_length.0)
                .max(clip_start);
            let global_start = state
                .snap
                .snap_tick(original_global + delta_ticks, project.tempo_map.ppq())
                .clamp(clip_start, max_start);
            next.preview_start = Ticks(global_start - clip_start);
            next.preview_length = drag.original_length;

            let delta_key = (-(pointer.y - drag.pointer_start.y) / grid.key_height).round() as i32;
            next.preview_key = (i32::from(drag.original_key) + delta_key).clamp(0, 127) as u8;
        }
        NoteDragKind::Resize => {
            let start_global = clip_start + drag.original_start.0;
            let min_end = start_global.saturating_add(1);
            let max_end = clip_end.max(min_end);
            let original_end = start_global + drag.original_length.0;
            let end = state
                .snap
                .snap_tick(original_end as f64 + delta_ticks, project.tempo_map.ppq())
                .clamp(min_end, max_end);
            next.preview_start = drag.original_start;
            next.preview_length = Ticks(end - start_global);
            next.preview_key = drag.original_key;
        }
    }

    state.note_drag = Some(next);
}

fn finish_note_drag(state: &mut UiState, actions: &mut Vec<UiAction>) {
    let Some(drag) = state.note_drag.take() else {
        return;
    };

    match drag.kind {
        NoteDragKind::Move => {
            let delta_ticks = drag.preview_start.0 as i64 - drag.original_start.0 as i64;
            let delta_key = i32::from(drag.preview_key) - i32::from(drag.original_key);
            if delta_ticks != 0 || delta_key != 0 {
                actions.push(UiAction::Edit(EditCommand::MoveNotes {
                    track: drag.note.track,
                    clip: drag.note.clip,
                    notes: vec![drag.note.note],
                    delta_ticks,
                    delta_key,
                }));
            }
        }
        NoteDragKind::Resize => {
            if drag.preview_length != drag.original_length {
                actions.push(UiAction::Edit(EditCommand::ResizeNote {
                    track: drag.note.track,
                    clip: drag.note.clip,
                    note: drag.note.note,
                    start: drag.preview_start,
                    length: drag.preview_length,
                }));
            }
        }
    }
}

fn delete_selected_note(state: &mut UiState, actions: &mut Vec<UiAction>) {
    let Some(selection) = state.selected_note.take() else {
        return;
    };
    actions.push(UiAction::Edit(EditCommand::RemoveNotes {
        track: selection.track,
        clip: selection.clip,
        notes: vec![selection.note],
    }));
}

fn clip_at_tick(track: &Track, tick: u64) -> Option<&MidiClip> {
    track
        .clips
        .iter()
        .find(|clip| clip.start.0 <= tick && tick < clip.end().0)
}

fn key_at_y(grid: &Grid, y: f32) -> u8 {
    let key = 127.0 - (y - grid.rect.top() + grid.scroll_y) / grid.key_height;
    key.floor().clamp(0.0, 127.0) as u8
}

struct NoteHit {
    note: NoteRef,
    start: u64,
    length: u64,
    key: u8,
    rect: Rect,
}

fn hit_test_note(track: &Track, grid: &Grid, pointer: egui::Pos2) -> Option<NoteHit> {
    let mut hit = None;
    for clip in &track.clips {
        for note in &clip.notes {
            let start = clip.start.0.saturating_add(note.start.0);
            let length = note
                .length
                .0
                .min(clip.length.0.saturating_sub(note.start.0));
            if length == 0 {
                continue;
            }
            let rect = note_screen_rect(grid, start, length, note.key);
            if rect.contains(pointer) {
                hit = Some(NoteHit {
                    note: NoteRef {
                        track: track.id,
                        clip: clip.id,
                        note: note.id,
                    },
                    start: note.start.0,
                    length: note.length.0,
                    key: note.key,
                    rect,
                });
            }
        }
    }
    hit
}

fn note_screen_rect(grid: &Grid, start: u64, length: u64, key: u8) -> Rect {
    let x0 = grid.tick_to_x(start as f64);
    let x1 = grid.tick_to_x(start.saturating_add(length) as f64);
    let y = grid.key_y(key);
    Rect::from_min_size(
        pos2(x0, y + 1.0),
        vec2((x1 - x0).max(2.0), (grid.key_height - 2.0).max(2.0)),
    )
}

struct Grid {
    rect: Rect,
    left_tick: f64,
    px_per_tick: f64,
    scroll_y: f32,
    key_height: f32,
}

impl Grid {
    fn tick_to_x(&self, tick: f64) -> f32 {
        self.rect.left() + ((tick - self.left_tick) * self.px_per_tick) as f32
    }

    fn x_to_tick(&self, x: f32) -> f64 {
        self.left_tick + f64::from(x - self.rect.left()) / self.px_per_tick
    }

    fn key_y(&self, key: u8) -> f32 {
        self.rect.top() + (127.0 - f32::from(key)) * self.key_height - self.scroll_y
    }
}

fn draw_key_rows(ui: &egui::Ui, grid: &Grid, panel: Rect) {
    let gutter_rect = Rect::from_min_max(
        pos2(panel.left(), panel.top() + RULER_HEIGHT),
        pos2(panel.left() + GUTTER_WIDTH, panel.bottom()),
    );
    let gutter_painter = ui.painter_at(gutter_rect);
    let grid_painter = ui.painter_at(grid.rect);
    let key_height = grid.key_height;

    for key in 0..=127u8 {
        let y = grid.key_y(key);
        if y + key_height < grid.rect.top() || y > panel.bottom() {
            continue;
        }

        let top = y.max(grid.rect.top());
        let bottom = (y + key_height).min(panel.bottom());
        if bottom <= top {
            continue;
        }

        let black = is_black_key(key);
        let row = Rect::from_min_max(pos2(grid.rect.left(), top), pos2(grid.rect.right(), bottom));
        grid_painter.rect_filled(
            row,
            CornerRadius::ZERO,
            if black {
                theme::GRID_ROW_BLACK
            } else {
                theme::GRID_ROW_WHITE
            },
        );
        grid_painter.line_segment(
            [
                pos2(grid.rect.left(), y + key_height),
                pos2(grid.rect.right(), y + key_height),
            ],
            Stroke::new(1.0, theme::GRID_LINE),
        );

        let key_rect = Rect::from_min_max(
            pos2(panel.left(), top),
            pos2(panel.left() + GUTTER_WIDTH, bottom),
        );
        gutter_painter.rect_filled(
            key_rect,
            CornerRadius::ZERO,
            if black {
                theme::KEY_BLACK
            } else {
                theme::KEY_WHITE
            },
        );
        gutter_painter.line_segment(
            [
                pos2(panel.left(), y + key_height),
                pos2(panel.left() + GUTTER_WIDTH, y + key_height),
            ],
            Stroke::new(1.0, Color32::from_black_alpha(60)),
        );

        if key % 12 == 0 {
            gutter_painter.text(
                pos2(panel.left() + GUTTER_WIDTH - 6.0, y + key_height * 0.5),
                Align2::RIGHT_CENTER,
                note_name(key),
                FontId::proportional(9.5),
                if black {
                    theme::TEXT
                } else {
                    Color32::from_rgb(45, 48, 54)
                },
            );
        }
    }

    ui.painter_at(panel).line_segment(
        [
            pos2(panel.left() + GUTTER_WIDTH, panel.top() + RULER_HEIGHT),
            pos2(panel.left() + GUTTER_WIDTH, panel.bottom()),
        ],
        Stroke::new(1.0, theme::GRID_BAR),
    );
}

fn draw_clips(
    ui: &egui::Ui,
    grid: &Grid,
    track: &Track,
    track_index: usize,
    selected_clip: Option<octava_core::ClipId>,
) {
    let painter = ui.painter_at(grid.rect);
    let color = theme::track_color(track_index);

    for clip in &track.clips {
        let x0 = grid.tick_to_x(clip.start.0 as f64);
        let x1 = grid.tick_to_x(clip.end().0 as f64);
        if x1 < grid.rect.left() || x0 > grid.rect.right() {
            continue;
        }

        let left = x0.max(grid.rect.left());
        let right = x1.min(grid.rect.right());
        let clip_rect =
            Rect::from_min_max(pos2(left, grid.rect.top()), pos2(right, grid.rect.bottom()));
        painter.rect_filled(clip_rect, CornerRadius::ZERO, color.gamma_multiply(0.07));
        painter.rect_filled(
            Rect::from_min_max(
                pos2(left, grid.rect.top()),
                pos2(right, grid.rect.top() + CLIP_HEADER_HEIGHT),
            ),
            CornerRadius::ZERO,
            color.gamma_multiply(0.24),
        );
        painter.text(
            pos2(left + 6.0, grid.rect.top() + CLIP_HEADER_HEIGHT * 0.5),
            Align2::LEFT_CENTER,
            clip_title(clip),
            FontId::proportional(9.5),
            color,
        );

        if selected_clip == Some(clip.id) {
            painter.rect_stroke(
                clip_rect,
                CornerRadius::ZERO,
                Stroke::new(1.0, color),
                StrokeKind::Inside,
            );
        }
    }
}

fn draw_grid_lines(ui: &egui::Ui, project: &Project, grid: &Grid, panel: Rect, snap: SnapSettings) {
    let painter = ui.painter_at(grid.rect);
    let bar_ticks = project
        .time_signature
        .ticks_per_bar(project.tempo_map.ppq()) as f64;
    let beat_ticks = project
        .time_signature
        .ticks_per_beat(project.tempo_map.ppq()) as f64;
    let snap_ticks = snap.division.ticks(project.tempo_map.ppq()) as f64;
    let visible_start = grid.left_tick.max(0.0);
    let visible_end = grid.x_to_tick(panel.right());

    let first_snap = (visible_start / snap_ticks).floor() as u64;
    let last_snap = (visible_end / snap_ticks).ceil() as u64;
    if snap_ticks < beat_ticks && snap_ticks * grid.px_per_tick >= 6.0 {
        let mut step = first_snap;
        while step <= last_snap {
            let tick = step as f64 * snap_ticks;
            if !(tick as u64).is_multiple_of(beat_ticks as u64) {
                let x = grid.tick_to_x(tick);
                if x >= grid.rect.left() && x <= panel.right() {
                    painter.line_segment(
                        [pos2(x, grid.rect.top()), pos2(x, grid.rect.bottom())],
                        Stroke::new(1.0, theme::GRID_LINE.gamma_multiply(0.5)),
                    );
                }
            }
            step += 1;
        }
    }

    let first_bar = (visible_start / bar_ticks).floor() as u64;
    let last_bar = (visible_end / bar_ticks).ceil() as u64;
    let beats_per_bar = u64::from(project.time_signature.numerator);
    let px_per_beat = grid.px_per_tick * f64::from(project.tempo_map.ppq());

    for bar in first_bar..=last_bar {
        let bar_start = bar as f64 * bar_ticks;
        let x = grid.tick_to_x(bar_start);
        if x < grid.rect.left() - 1.0 || x > panel.right() {
            continue;
        }

        if px_per_beat as f32 >= 34.0 {
            for beat in 1..beats_per_bar {
                let beat_x = grid.tick_to_x(bar_start + beat as f64 * beat_ticks);
                if beat_x >= grid.rect.left() && beat_x <= panel.right() {
                    painter.line_segment(
                        [
                            pos2(beat_x, grid.rect.top()),
                            pos2(beat_x, grid.rect.bottom()),
                        ],
                        Stroke::new(1.0, theme::GRID_LINE),
                    );
                }
            }
        }

        ui.painter_at(panel).line_segment(
            [pos2(x, panel.top()), pos2(x, grid.rect.bottom())],
            Stroke::new(1.0, theme::GRID_BAR),
        );
    }
}

fn draw_notes(ui: &egui::Ui, grid: &Grid, track: &Track, track_index: usize, state: &UiState) {
    let painter = ui.painter_at(grid.rect);
    let color = theme::track_color(track_index);
    let drag = state.note_drag;

    for clip in &track.clips {
        for note in &clip.notes {
            let clipped_end = note.start.saturating_add(note.length).min(clip.length);
            if clipped_end <= note.start {
                continue;
            }

            let note_ref = NoteRef {
                track: track.id,
                clip: clip.id,
                note: note.id,
            };
            let active_drag = drag.filter(|drag| drag.note == note_ref);
            let (relative_start, length, key) = active_drag.map_or(
                (note.start.0, clipped_end.0 - note.start.0, note.key),
                |drag| {
                    (
                        drag.preview_start.0,
                        drag.preview_length.0,
                        drag.preview_key,
                    )
                },
            );
            let start = clip.start.0 as f64 + relative_start as f64;
            let end = start + length as f64;
            let x0 = grid.tick_to_x(start);
            let x1 = grid.tick_to_x(end);
            let y = grid.key_y(key);

            if x1 < grid.rect.left()
                || x0 > grid.rect.right()
                || y + grid.key_height < grid.rect.top()
                || y > grid.rect.bottom()
            {
                continue;
            }

            let left = x0.max(grid.rect.left()) + 1.0;
            let right = x1.min(grid.rect.right());
            let width = (right - left).max(2.0);
            let note_rect = Rect::from_min_size(
                pos2(left, y + 1.0),
                vec2(width, (grid.key_height - 2.0).max(2.0)),
            )
            .intersect(grid.rect);
            if note_rect.width() <= 0.0 || note_rect.height() <= 0.0 {
                continue;
            }

            let velocity = f32::from(note.velocity) / 127.0;
            let selected = state.selected_note == Some(note_ref);
            let fill = if selected || active_drag.is_some() {
                color
            } else {
                color.gamma_multiply(0.58 + 0.42 * velocity)
            };
            painter.rect_filled(note_rect, CornerRadius::same(3), fill);
            painter.rect_stroke(
                note_rect,
                CornerRadius::same(3),
                Stroke::new(1.0, fill.gamma_multiply(0.55)),
                StrokeKind::Inside,
            );

            if selected || active_drag.is_some() {
                painter.rect_stroke(
                    note_rect,
                    CornerRadius::same(3),
                    Stroke::new(1.0, theme::TEXT),
                    StrokeKind::Inside,
                );
                painter.rect_filled(
                    Rect::from_min_max(
                        pos2(note_rect.right() - 2.5, note_rect.top() + 2.0),
                        pos2(note_rect.right() - 0.5, note_rect.bottom() - 2.0),
                    ),
                    CornerRadius::same(1),
                    theme::TEXT,
                );
            }
        }
    }
}

fn draw_ruler(ui: &egui::Ui, project: &Project, grid: &Grid, panel: Rect) {
    let ruler_rect = Rect::from_min_max(
        pos2(panel.left(), panel.top()),
        pos2(panel.right(), panel.top() + RULER_HEIGHT),
    );
    let painter = ui.painter_at(ruler_rect);
    painter.rect_filled(ruler_rect, CornerRadius::ZERO, theme::RULER);

    let bar_ticks = project
        .time_signature
        .ticks_per_bar(project.tempo_map.ppq()) as f64;
    let visible_end = grid.x_to_tick(panel.right());
    let first_bar = (grid.left_tick.max(0.0) / bar_ticks).floor() as u64;
    let last_bar = (visible_end / bar_ticks).ceil() as u64;

    for bar in first_bar..=last_bar {
        let x = grid.tick_to_x(bar as f64 * bar_ticks);
        if x < grid.rect.left() - 1.0 || x > panel.right() {
            continue;
        }
        painter.line_segment(
            [
                pos2(x, panel.top() + RULER_HEIGHT - 6.0),
                pos2(x, panel.top() + RULER_HEIGHT),
            ],
            Stroke::new(1.0, theme::TEXT_FAINT),
        );
        painter.text(
            pos2(x + 4.0, panel.top() + 4.0),
            Align2::LEFT_TOP,
            format!("{}", bar + 1),
            FontId::proportional(10.0),
            theme::TEXT_DIM,
        );
    }

    painter.text(
        pos2(panel.left() + 8.0, panel.top() + RULER_HEIGHT * 0.5),
        Align2::LEFT_CENTER,
        "KEYS",
        FontId::proportional(9.0),
        theme::TEXT_FAINT,
    );
    painter.line_segment(
        [
            pos2(panel.left(), panel.top() + RULER_HEIGHT),
            pos2(panel.right(), panel.top() + RULER_HEIGHT),
        ],
        Stroke::new(1.0, theme::GRID_BAR),
    );
}

fn draw_playhead(ui: &egui::Ui, grid: &Grid, playback: &PlaybackView) {
    let x = grid.tick_to_x(playback.playhead.0 as f64);
    if x < grid.rect.left() || x > grid.rect.right() {
        return;
    }

    let painter = ui.painter_at(grid.rect.expand2(vec2(0.0, RULER_HEIGHT)));
    painter.line_segment(
        [
            pos2(x, grid.rect.top() - RULER_HEIGHT),
            pos2(x, grid.rect.bottom()),
        ],
        Stroke::new(1.5, theme::ACCENT),
    );
    painter.add(egui::Shape::convex_polygon(
        vec![
            pos2(x - 4.0, grid.rect.top() - RULER_HEIGHT + 2.0),
            pos2(x + 4.0, grid.rect.top() - RULER_HEIGHT + 2.0),
            pos2(x, grid.rect.top() - RULER_HEIGHT + 9.0),
        ],
        theme::ACCENT,
        Stroke::NONE,
    ));
}

fn clip_title(clip: &MidiClip) -> String {
    format!("Clip {}  ·  {} notes", clip.id.get(), clip.notes.len())
}

fn is_black_key(key: u8) -> bool {
    matches!(key % 12, 1 | 3 | 6 | 8 | 10)
}

fn note_name(key: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let octave = i32::from(key) / 12 - 1;
    format!("{}{}", NAMES[usize::from(key % 12)], octave)
}

#[derive(Clone, Copy)]
enum ToolIcon {
    Magnet,
    Eraser,
    Quantize,
    Velocity,
}

fn tool_icon_button(
    ui: &mut egui::Ui,
    icon: ToolIcon,
    active: bool,
    enabled: bool,
    tooltip: &str,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(26.0, 22.0), Sense::click());
    let hovered = response.hovered() && enabled;
    let fill = if active {
        theme::ACCENT_SOFT
    } else if hovered {
        theme::PANEL_RAISED
    } else {
        theme::BACKGROUND
    };
    let stroke = if active {
        theme::ACCENT
    } else {
        theme::GRID_LINE
    };
    let color = if active {
        theme::ACCENT
    } else if enabled {
        theme::TEXT_DIM
    } else {
        theme::TEXT_FAINT
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(3), fill);
    painter.rect_stroke(
        rect,
        CornerRadius::same(3),
        Stroke::new(1.0, stroke),
        StrokeKind::Inside,
    );
    draw_tool_icon(painter, rect.shrink(6.0), icon, color);

    let response = if enabled {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response
    };
    response.on_hover_text(tooltip)
}

fn draw_tool_icon(painter: &egui::Painter, rect: Rect, icon: ToolIcon, color: Color32) {
    let stroke = Stroke::new(1.6, color);
    match icon {
        ToolIcon::Magnet => {
            let left = rect.left();
            let right = rect.right();
            let top = rect.top();
            let bottom = rect.bottom();
            painter.add(egui::Shape::line(
                vec![
                    pos2(left, top),
                    pos2(left, bottom - 2.0),
                    pos2(left + 2.0, bottom),
                    pos2(right - 2.0, bottom),
                    pos2(right, bottom - 2.0),
                    pos2(right, top),
                ],
                stroke,
            ));
            painter.line_segment(
                [pos2(left, top), pos2(left, top + 3.0)],
                Stroke::new(2.4, color),
            );
            painter.line_segment(
                [pos2(right, top), pos2(right, top + 3.0)],
                Stroke::new(2.4, color),
            );
        }
        ToolIcon::Eraser => {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    pos2(rect.left() + 2.0, rect.bottom() - 2.0),
                    pos2(rect.left() + 6.0, rect.bottom() - 6.0),
                    pos2(rect.right() - 2.0, rect.top() + 2.0),
                    pos2(rect.right() - 6.0, rect.top() + 6.0),
                ],
                color.gamma_multiply(0.35),
                stroke,
            ));
        }
        ToolIcon::Quantize => {
            for (index, x) in [rect.left(), rect.center().x, rect.right()]
                .iter()
                .enumerate()
            {
                let height = [5.0, 8.0, 11.0][index];
                painter.line_segment(
                    [pos2(*x, rect.bottom()), pos2(*x, rect.bottom() - height)],
                    stroke,
                );
            }
        }
        ToolIcon::Velocity => {
            for (index, x) in [rect.left(), rect.center().x, rect.right()]
                .iter()
                .enumerate()
            {
                let height = [4.0, 7.0, 10.0][index];
                painter.rect_filled(
                    Rect::from_min_max(
                        pos2(*x - 1.0, rect.bottom() - height),
                        pos2(*x + 1.0, rect.bottom()),
                    ),
                    CornerRadius::same(1),
                    color,
                );
            }
        }
    }
}
