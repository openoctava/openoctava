use std::path::{Path, PathBuf};

use egui::{RichText, ScrollArea};
use octava_core::{EditCommand, Preset, Project};

use crate::state::{PickerEntry, SoundfontPicker, SoundfontView, UiAction, UiState};
use crate::theme;

/// Draws the SoundFont player and its file browser, if they are open.
pub(crate) fn show(
    ctx: &egui::Context,
    project: &Project,
    soundfont: &SoundfontView,
    state: &mut UiState,
    actions: &mut Vec<UiAction>,
) {
    player_window(ctx, project, soundfont, state, actions);
    picker_window(ctx, state, actions);
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
                    let start = soundfont
                        .path
                        .as_deref()
                        .map(PathBuf::from)
                        .and_then(|path| path.parent().map(Path::to_path_buf))
                        .or_else(|| std::env::current_dir().ok())
                        .unwrap_or_else(|| PathBuf::from("."));
                    state.soundfont_picker = Some(read_directory(&start));
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

fn picker_window(ctx: &egui::Context, state: &mut UiState, actions: &mut Vec<UiAction>) {
    let Some(picker) = state.soundfont_picker.as_mut() else {
        return;
    };

    let mut open = true;
    let mut navigate_to = None;
    let mut chosen = None;
    let mut cancel = false;

    egui::Window::new("Choose SoundFont")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(460.0)
        .min_width(360.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("↑ Up").clicked()
                    && let Some(parent) = picker.directory.parent()
                {
                    navigate_to = Some(parent.to_path_buf());
                }
                ui.label(
                    RichText::new(picker.directory.display().to_string())
                        .size(10.0)
                        .color(theme::TEXT_DIM),
                );
            });
            if let Some(error) = &picker.error {
                ui.label(
                    RichText::new(error)
                        .size(10.0)
                        .color(egui::Color32::from_rgb(222, 112, 102)),
                );
            }
            ui.add_space(4.0);

            ScrollArea::vertical()
                .max_height(300.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if picker.entries.is_empty() {
                        ui.label(
                            RichText::new("No folders or .sf2 files here")
                                .size(10.5)
                                .color(theme::TEXT_FAINT),
                        );
                    }
                    for entry in &picker.entries {
                        match entry {
                            PickerEntry::Directory(path) => {
                                let name = path.file_name().map_or_else(
                                    || path.display().to_string(),
                                    |name| name.to_string_lossy().into(),
                                );
                                if ui.button(format!("📁 {name}")).clicked() {
                                    navigate_to = Some(path.clone());
                                }
                            }
                            PickerEntry::Soundfont(path) => {
                                let name = path.file_name().map_or_else(
                                    || path.display().to_string(),
                                    |name| name.to_string_lossy().into(),
                                );
                                if ui.button(format!("🎹 {name}")).clicked() {
                                    chosen = Some(path.clone());
                                }
                            }
                        }
                    }
                });

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Bundled SoundFont").clicked() {
                    actions.push(UiAction::Edit(EditCommand::SetSoundfont { path: None }));
                    cancel = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        });

    if let Some(path) = navigate_to {
        *picker = read_directory(&path);
    } else if let Some(path) = chosen {
        actions.push(UiAction::Edit(EditCommand::SetSoundfont {
            path: Some(path),
        }));
        state.soundfont_picker = None;
    } else if cancel || !open {
        state.soundfont_picker = None;
    }
}

fn read_directory(path: &Path) -> SoundfontPicker {
    let mut entries = Vec::new();
    let mut error = None;
    let mut directories = Vec::new();
    let mut soundfonts = Vec::new();

    match std::fs::read_dir(path) {
        Ok(reader) => {
            for entry in reader.flatten() {
                let Ok(file_type) = entry.file_type() else {
                    continue;
                };
                let entry_path = entry.path();
                if file_type.is_dir() {
                    directories.push(entry_path);
                } else if is_soundfont(&entry_path) {
                    soundfonts.push(entry_path);
                }
            }
        }
        Err(source) => error = Some(format!("{path:?}: {source}")),
    }

    directories.sort_by_key(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
    });
    soundfonts.sort_by_key(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
    });

    entries.extend(directories.into_iter().map(PickerEntry::Directory));
    entries.extend(soundfonts.into_iter().map(PickerEntry::Soundfont));

    SoundfontPicker {
        directory: path.to_path_buf(),
        entries,
        error,
    }
}

fn is_soundfont(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("sf2") || extension.eq_ignore_ascii_case("sf3")
        })
}

/// Small helper shared with the clip list so that the same preset renders the
/// same way everywhere.
pub(crate) fn preset_label(soundfont: &SoundfontView, preset: Preset) -> String {
    soundfont.preset_name(preset).map_or_else(
        || format!("bank {} · program {}", preset.bank, preset.program),
        str::to_owned,
    )
}
