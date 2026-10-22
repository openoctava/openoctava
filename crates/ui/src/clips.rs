use egui::{
    Align2, CornerRadius, CursorIcon, FontId, Rect, RichText, Sense, Stroke, StrokeKind, pos2, vec2,
};
use octava_core::{EditCommand, PatternId, Preset, Project};

use crate::state::{PatternDragPayload, PlaybackView, UiAction, UiState};
use crate::theme;

/// A small, deliberately incomplete GM program list used by the per-pattern
/// instrument chooser. The selection is stored on the pattern but is not routed
/// to the synthesizer yet: a track owns one MIDI channel and therefore one
/// program.
pub(crate) const INSTRUMENTS: [(&str, u8); 12] = [
    ("Acoustic Piano", 0),
    ("Electric Piano", 4),
    ("Drawbar Organ", 16),
    ("Nylon Guitar", 24),
    ("Electric Bass", 33),
    ("Strings", 48),
    ("Choir", 52),
    ("Brass", 61),
    ("Alto Sax", 65),
    ("Square Lead", 80),
    ("Warm Pad", 89),
    ("Marimba", 12),
];

#[must_use]
pub(crate) fn instrument_name(preset: Preset) -> &'static str {
    INSTRUMENTS
        .iter()
        .find(|(_, program)| *program == preset.program)
        .map_or("Program", |(name, _)| name)
}

pub(crate) fn show(
    ui: &mut egui::Ui,
    project: &Project,
    _playback: &PlaybackView,
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
                        .add(egui::Button::new(RichText::new("+ Clip").size(9.5)))
                        .on_hover_text("Create a new 4-bar pattern")
                        .clicked()
                    {
                        let number = project.patterns.len() + 1;
                        actions.push(UiAction::Edit(EditCommand::AddPattern {
                            name: format!("Clip {number}"),
                            length: octava_core::Ticks(bar_ticks.saturating_mul(4).max(1)),
                        }));
                    }
                });
            });
            ui.add_space(2.0);
            ui.label(
                RichText::new("Drag a clip into Tracks to place it")
                    .size(9.0)
                    .color(theme::TEXT_FAINT),
            );
            ui.add_space(3.0);

            if project.patterns.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        RichText::new("No clips yet")
                            .size(12.0)
                            .color(theme::TEXT_DIM),
                    );
                });
                return;
            }

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (index, pattern) in project.patterns.iter().enumerate() {
                        let selected = state.selected_pattern == Some(pattern.id);
                        let color = theme::pattern_color(index);
                        let response = pattern_card(
                            ui,
                            &pattern.name,
                            pattern.notes.len(),
                            pattern.length.0,
                            pattern.preset,
                            project.pattern_use_count(pattern.id),
                            bar_ticks,
                            color,
                            selected,
                        );

                        if response.clicked() {
                            state.select_pattern(pattern.id);
                        }
                        if response.double_clicked() {
                            state.select_pattern(pattern.id);
                            state.focus_piano_roll = true;
                        }

                        // Drag a pattern from the library onto the timeline.
                        if response.drag_started() {
                            response.ctx.set_cursor_icon(CursorIcon::Grabbing);
                        }
                        response.dnd_set_drag_payload(PatternDragPayload {
                            pattern: pattern.id,
                        });

                        response.context_menu(|ui| {
                            pattern_menu(ui, pattern.id, actions);
                        });

                        ui.add_space(4.0);
                    }
                });
        });
}

fn pattern_menu(ui: &mut egui::Ui, pattern: PatternId, actions: &mut Vec<UiAction>) {
    if ui
        .button("Clone")
        .on_hover_text("Copy this clip into a new, independent clip")
        .clicked()
    {
        actions.push(UiAction::Edit(EditCommand::ClonePattern { pattern }));
        ui.close();
    }

    ui.menu_button("Instrument", |ui| {
        ui.label(
            RichText::new("Placeholder: not routed to the synth yet")
                .size(9.0)
                .color(theme::TEXT_FAINT),
        );
        ui.separator();
        for (name, program) in INSTRUMENTS {
            if ui
                .button(name)
                .on_hover_text(format!("GM program {program}"))
                .clicked()
            {
                actions.push(UiAction::Edit(EditCommand::SetPatternPreset {
                    pattern,
                    preset: Preset::new(0, program),
                }));
                ui.close();
            }
        }
    });

    ui.separator();
    if ui
        .button("Delete")
        .on_hover_text("Remove this clip and every placement of it")
        .clicked()
    {
        actions.push(UiAction::Edit(EditCommand::RemovePattern { pattern }));
        ui.close();
    }
}

#[allow(clippy::too_many_arguments)]
fn pattern_card(
    ui: &mut egui::Ui,
    name: &str,
    notes: usize,
    length: u64,
    preset: Preset,
    uses: usize,
    bar_ticks: u64,
    color: egui::Color32,
    selected: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width().max(80.0), 46.0),
        Sense::click_and_drag(),
    );
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

    let text_color = if selected {
        theme::TEXT
    } else {
        theme::TEXT_DIM
    };
    painter.text(
        pos2(rect.left() + 10.0, rect.top() + 12.0),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(11.5),
        text_color,
    );
    painter.text(
        pos2(rect.left() + 10.0, rect.top() + 31.0),
        Align2::LEFT_CENTER,
        format!(
            "{}  ·  {:.0} bars  ·  {notes} notes  ·  used {uses}×",
            instrument_name(preset),
            length as f64 / bar_ticks as f64
        ),
        FontId::proportional(9.0),
        theme::TEXT_FAINT,
    );

    response
        .on_hover_cursor(CursorIcon::Grab)
        .on_hover_text("Click to edit · double-click for Piano Roll · drag into Tracks")
}
