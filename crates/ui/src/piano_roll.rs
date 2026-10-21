use egui::{
    Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, StrokeKind, Vec2, pos2, vec2,
};
use octava_core::{ClipId, MidiClip, Project, Ticks, Track};

use crate::state::{PlaybackView, UiAction, UiState};
use crate::theme;

const GUTTER_WIDTH: f32 = 58.0;
const RULER_HEIGHT: f32 = 24.0;
const CLIP_HEADER_HEIGHT: f32 = 17.0;
const MIN_PX_PER_BEAT: f32 = 26.0;
const MAX_PX_PER_BEAT: f32 = 1200.0;

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

    handle_input(ui, project, state, &response, grid_left, grid_rect.height());

    let grid = Grid {
        rect: grid_rect,
        left_tick: state.piano_roll.left_tick,
        px_per_tick: f64::from(state.piano_roll.px_per_beat) / f64::from(project.tempo_map.ppq()),
        scroll_y: state.piano_roll.scroll_y,
        key_height: state.piano_roll.key_height,
    };
    let track_index = project.track_index(track.id).unwrap_or(0);

    draw_key_rows(ui, &grid, rect);
    draw_clips(ui, &grid, track, track_index, state.selected_clip);
    draw_grid_lines(ui, project, &grid, rect);
    draw_notes(ui, &grid, track, track_index);
    draw_ruler(ui, project, &grid, rect);
    draw_playhead(ui, &grid, playback);

    if track.clips.is_empty() {
        ui.painter_at(grid.rect).text(
            grid.rect.center(),
            Align2::CENTER_CENTER,
            "No clips on this track",
            FontId::proportional(13.0),
            theme::TEXT_FAINT,
        );
    }

    ui.painter_at(rect).text(
        pos2(rect.right() - 8.0, rect.bottom() - 6.0),
        Align2::RIGHT_BOTTOM,
        "Ctrl+wheel: zoom   wheel: scroll   click ruler: seek",
        FontId::proportional(9.5),
        theme::TEXT_FAINT,
    );

    if response.clicked()
        && let Some(pointer) = response.interact_pointer_pos()
        && pointer.y < grid_top
        && pointer.x > grid_left
    {
        let tick = grid.x_to_tick(pointer.x).max(0.0);
        let beat = project
            .time_signature
            .ticks_per_beat(project.tempo_map.ppq()) as f64;
        let step = (beat / 4.0).max(1.0);
        let snapped = ((tick / step).round() * step) as u64;
        actions.push(UiAction::Seek(Ticks(snapped)));
    }
}

fn handle_input(
    ui: &egui::Ui,
    project: &Project,
    state: &mut UiState,
    response: &egui::Response,
    grid_left: f32,
    grid_height: f32,
) {
    let key_height = state.piano_roll.key_height;
    let max_scroll = (128.0 * key_height - grid_height).max(0.0);
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

            if let Some(pointer) = pointer.filter(|pointer| pointer.x >= grid_left) {
                let old_px_per_tick = old_px_per_beat / ppq;
                let new_px_per_tick = new_px_per_beat / ppq;
                let anchor_tick = state.piano_roll.left_tick
                    + f64::from((pointer.x - grid_left) / old_px_per_tick);
                state.piano_roll.left_tick =
                    anchor_tick - f64::from((pointer.x - grid_left) / new_px_per_tick);
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
    selected_clip: Option<ClipId>,
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

fn draw_grid_lines(ui: &egui::Ui, project: &Project, grid: &Grid, panel: Rect) {
    let painter = ui.painter_at(grid.rect);
    let bar_ticks = project
        .time_signature
        .ticks_per_bar(project.tempo_map.ppq()) as f64;
    let beat_ticks = project
        .time_signature
        .ticks_per_beat(project.tempo_map.ppq()) as f64;
    let visible_start = grid.left_tick.max(0.0);
    let visible_end = grid.x_to_tick(panel.right());

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

fn draw_notes(ui: &egui::Ui, grid: &Grid, track: &Track, track_index: usize) {
    let painter = ui.painter_at(grid.rect);
    let color = theme::track_color(track_index);

    for clip in &track.clips {
        for note in &clip.notes {
            let clipped_end = note.start.saturating_add(note.length).min(clip.length);
            if clipped_end <= note.start {
                continue;
            }

            let start = clip.start.0 as f64 + note.start.0 as f64;
            let end = clip.start.0 as f64 + clipped_end.0 as f64;
            let x0 = grid.tick_to_x(start);
            let x1 = grid.tick_to_x(end);
            let y = grid.key_y(note.key);

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
            let fill = color.gamma_multiply(0.58 + 0.42 * velocity);
            painter.rect_filled(note_rect, CornerRadius::same(3), fill);
            painter.rect_stroke(
                note_rect,
                CornerRadius::same(3),
                Stroke::new(1.0, fill.gamma_multiply(0.55)),
                StrokeKind::Inside,
            );
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
