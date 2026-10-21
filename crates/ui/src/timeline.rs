use egui::{Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use octava_core::{Project, Ticks, Track};

use crate::state::{PlaybackView, UiAction, UiState};
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
    let beat_ticks = project
        .time_signature
        .ticks_per_beat(project.tempo_map.ppq()) as f64;
    let timeline_left = rect.left() + HEADER_WIDTH;
    let timeline_width = (rect.width() - HEADER_WIDTH).max(1.0);
    let project_bars = (project.length_ticks().0 as f64 / bar_ticks).ceil() as u64;
    let total_bars = project_bars.max(8);
    let px_per_tick = f64::from(timeline_width) / (total_bars as f64 * bar_ticks);
    let px_per_bar = px_per_tick * bar_ticks;

    let timeline_rect = Rect::from_min_max(pos2(timeline_left, rect.top()), rect.max);
    let max_lanes = ((rect.height() - RULER_HEIGHT) / LANE_HEIGHT).floor() as usize;

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

    let visible_tracks = project.tracks.len().min(max_lanes);
    for (index, track) in project.tracks.iter().take(visible_tracks).enumerate() {
        let lane_top = rect.top() + RULER_HEIGHT + index as f32 * LANE_HEIGHT;
        let lane_bottom = (lane_top + LANE_HEIGHT).min(rect.bottom());
        let header_rect = Rect::from_min_max(
            pos2(rect.left(), lane_top),
            pos2(timeline_left, lane_bottom),
        );
        let header_response = ui.interact(
            header_rect,
            ui.id().with(("timeline-track", track.id.0)),
            Sense::click(),
        );
        if header_response.clicked() {
            state.select_track(project, track.id);
        }
        let selected = state.selected_track == Some(track.id);
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

        draw_track_header(ui, track, header_rect, selected);
        draw_track_clips(
            ui,
            track,
            index,
            state,
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
        let step = (beat_ticks / 4.0).max(1.0);
        let snapped = ((tick / step).round() * step) as u64;
        actions.push(UiAction::Seek(Ticks(snapped)));
    }
}

fn draw_track_header(ui: &egui::Ui, track: &Track, rect: Rect, selected: bool) {
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
            track.note_count()
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
    track: &Track,
    track_index: usize,
    state: &mut UiState,
    timeline_left: f32,
    px_per_tick: f64,
    lane_top: f32,
    lane_bottom: f32,
) {
    let color = theme::track_color(track_index);
    let clip_painter = ui.painter_at(Rect::from_min_max(
        pos2(timeline_left, lane_top),
        pos2(ui.max_rect().right(), lane_bottom),
    ));

    for clip in &track.clips {
        let x0 = timeline_left + (clip.start.0 as f64 * px_per_tick) as f32;
        let x1 = timeline_left + (clip.end().0 as f64 * px_per_tick) as f32;
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
            Sense::click(),
        );
        if response.clicked() {
            state.selected_track = Some(track.id);
            state.selected_clip = Some(clip.id);
        }
        let selected =
            state.selected_track == Some(track.id) && state.selected_clip == Some(clip.id);

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
            format!("Clip {}", clip.id.get()),
            FontId::proportional(9.5),
            color,
        );
        clip_painter.text(
            pos2(clip_rect.left() + 4.0, clip_rect.bottom() - 8.0),
            Align2::LEFT_BOTTOM,
            format!("{} notes", clip.notes.len()),
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
