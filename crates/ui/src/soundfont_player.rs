use std::path::Path;

use egui::{RichText, ScrollArea};
use octava_core::{EditCommand, Preset, Project};

use crate::state::{SoundfontView, UiAction, UiState};
use crate::theme;

/// Draws the SoundFont player, if it is open.
pub(crate) fn show(
    ctx: &egui::Context,
    project: &Project,
    soundfont: &SoundfontView,
    state: &mut UiState,
    actions: &mut Vec<UiAction>,
) {
    player_window(ctx, project, soundfont, state, actions);
}

fn player_window(
    ctx: &egui::Context,
    project: &Project,
    soundfont: &SoundfontView,
    state: &mut UiState,
    actions: &mut Vec<UiAction>,
) {
    let Some(pattern_id) = state.soundfont_player else {
        return;
    };
    let Some(pattern) = project.pattern(pattern_id) else {
        state.soundfont_player = None;
        return;
    };

    let mut open = true;
    egui::Window::new("SoundFont Player")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(420.0)
        .min_width(320.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Instrument")
                        .size(10.5)
                        .color(theme::TEXT_DIM),
                );
                ui.label(
                    RichText::new("SoundFont Player")
                        .size(11.5)
                        .strong()
                        .color(theme::ACCENT),
                );
            });
            ui.label(
                RichText::new(format!("Pattern · {}", pattern.name))
                    .size(10.5)
                    .color(theme::TEXT_DIM),
            );
            ui.label(
                RichText::new(format!(
                    "Sound · {}",
                    preset_label(soundfont, pattern.preset)
                ))
                .size(10.5)
                .color(theme::TEXT_DIM),
            );
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("SoundFont")
                        .size(10.5)
                        .color(theme::TEXT_DIM),
                );
                let label = soundfont.path.as_deref().map_or_else(
                    || "Bundled SoundFont".to_owned(),
                    |path| {
                        Path::new(path)
                            .file_name()
                            .map_or_else(|| path.to_owned(), |name| name.to_string_lossy().into())
                    },
                );
                ui.label(RichText::new(label).size(11.0).color(theme::TEXT))
                    .on_hover_text(
                        soundfont
                            .path
                            .clone()
                            .unwrap_or_else(|| "assets/soundfonts".to_owned()),
                    );
                if ui.small_button("Choose…").clicked() {
                    actions.push(UiAction::ChooseSoundfont);
                }
                if soundfont.path.is_some() && ui.small_button("Bundled").clicked() {
                    actions.push(UiAction::Edit(EditCommand::SetSoundfont { path: None }));
                }
            });

            if let Some(error) = &soundfont.error {
                ui.label(
                    RichText::new(error)
                        .size(10.0)
                        .color(egui::Color32::from_rgb(222, 112, 102)),
                );
            }

            ui.add_space(6.0);
            ui.label(
                RichText::new("SOUNDS")
                    .size(9.5)
                    .strong()
                    .color(theme::TEXT_FAINT),
            );

            if soundfont.presets.is_empty() {
                ui.label(
                    RichText::new("No sounds in this SoundFont")
                        .size(10.5)
                        .color(theme::TEXT_FAINT),
                );
                return;
            }

            ScrollArea::vertical()
                .max_height(260.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for preset in &soundfont.presets {
                        let selected =
                            pattern.preset.bank == preset.bank
                                && pattern.preset.program == preset.program;
                        let label = format!(
                            "{:03}:{:03}  {}",
                            preset.bank, preset.program, preset.name
                        );
                        if ui
                            .add(egui::Button::new(RichText::new(label).size(11.0)).selected(selected))
                            .clicked()
                        {
                            actions.push(UiAction::Edit(EditCommand::SetPatternPreset {
                                pattern: pattern_id,
                                preset: Preset::new(preset.bank, preset.program),
                            }));
                        }
                        ui.add_space(1.0);
                    }
                });
            ui.add_space(4.0);
            ui.label(
                RichText::new("The selected sound is stored on the clip; every pattern can use a different one.")
                    .size(9.0)
                    .color(theme::TEXT_FAINT),
            );
        });
    if !open {
        state.soundfont_player = None;
    }
}

/// Small helper shared with the clip list so that the same preset renders the
/// same way everywhere.
pub(crate) fn preset_label(soundfont: &SoundfontView, preset: Preset) -> String {
    soundfont.preset_name(preset).map_or_else(
        || format!("bank {} · program {}", preset.bank, preset.program),
        str::to_owned,
    )
}
