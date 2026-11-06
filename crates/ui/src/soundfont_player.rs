use std::path::Path;

use egui::{Color32, RichText, ScrollArea, Stroke, Vec2};
use octava_core::{EditCommand, Preset, Project, SoundParams};

use crate::state::{SoundfontPreset, SoundfontView, UiAction, UiState};
use crate::theme;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SoundCategory {
    #[default]
    All,
    Piano,
    ChromaticPercussion,
    Organ,
    Guitar,
    Bass,
    Strings,
    Brass,
    Reed,
    Pipe,
    SynthLead,
    SynthPad,
    SynthFx,
    Percussion,
    SoundEffects,
    Drums,
}

impl SoundCategory {
    pub const TABS: &'static [(Self, &'static str)] = &[
        (Self::All, "All"),
        (Self::Piano, "Piano"),
        (Self::ChromaticPercussion, "Chromatic"),
        (Self::Organ, "Organ"),
        (Self::Guitar, "Guitar"),
        (Self::Bass, "Bass"),
        (Self::Strings, "Strings"),
        (Self::Brass, "Brass"),
        (Self::Reed, "Reed"),
        (Self::Pipe, "Pipe"),
        (Self::SynthLead, "Lead"),
        (Self::SynthPad, "Pad"),
        (Self::SynthFx, "Synth FX"),
        (Self::Percussion, "Percussion"),
        (Self::SoundEffects, "SFX"),
        (Self::Drums, "Drums"),
    ];

    #[must_use]
    pub fn matches(&self, bank: u8, program: u8, name: &str) -> bool {
        if *self == Self::All {
            return true;
        }
        if bank == 128 {
            return *self == Self::Drums || *self == Self::Percussion;
        }
        let lower = name.to_lowercase();
        match self {
            Self::All => true,
            Self::Piano => {
                (0..=7).contains(&program)
                    || lower.contains("piano")
                    || lower.contains("rhodes")
                    || lower.contains("clav")
                    || lower.contains("harpsi")
            }
            Self::ChromaticPercussion => {
                (8..=15).contains(&program)
                    || lower.contains("celesta")
                    || lower.contains("glocken")
                    || lower.contains("music box")
                    || lower.contains("vibra")
                    || lower.contains("marimba")
                    || lower.contains("xylo")
                    || lower.contains("bells")
                    || lower.contains("dulcimer")
            }
            Self::Organ => {
                (16..=23).contains(&program)
                    || lower.contains("organ")
                    || lower.contains("accord")
                    || lower.contains("harmonica")
            }
            Self::Guitar => {
                (24..=31).contains(&program) || lower.contains("guitar") || lower.contains("gtr")
            }
            Self::Bass => (32..=39).contains(&program) || lower.contains("bass"),
            Self::Strings => {
                (40..=55).contains(&program)
                    || lower.contains("string")
                    || lower.contains("violin")
                    || lower.contains("viola")
                    || lower.contains("cello")
                    || lower.contains("contra")
                    || lower.contains("harp")
                    || lower.contains("timpani")
                    || lower.contains("choir")
                    || lower.contains("voice")
            }
            Self::Brass => {
                (56..=63).contains(&program)
                    || lower.contains("brass")
                    || lower.contains("trumpet")
                    || lower.contains("trombone")
                    || lower.contains("tuba")
                    || lower.contains("horn")
            }
            Self::Reed => {
                (64..=71).contains(&program)
                    || lower.contains("sax")
                    || lower.contains("oboe")
                    || lower.contains("bassoon")
                    || lower.contains("clarinet")
            }
            Self::Pipe => {
                (72..=79).contains(&program)
                    || lower.contains("flute")
                    || lower.contains("pipe")
                    || lower.contains("whistle")
                    || lower.contains("recorder")
                    || lower.contains("ocarina")
            }
            Self::SynthLead => {
                (80..=87).contains(&program)
                    || lower.contains("lead")
                    || lower.contains("saw")
                    || lower.contains("square")
            }
            Self::SynthPad => {
                (88..=95).contains(&program)
                    || lower.contains("pad")
                    || lower.contains("warm")
                    || lower.contains("halo")
                    || lower.contains("sweep")
            }
            Self::SynthFx => {
                (96..=103).contains(&program)
                    || lower.contains("fx")
                    || lower.contains("rain")
                    || lower.contains("atmosphere")
                    || lower.contains("crystal")
            }
            Self::Percussion => {
                (112..=119).contains(&program)
                    || lower.contains("drum")
                    || lower.contains("tom")
                    || lower.contains("tinkle")
                    || lower.contains("steel drum")
            }
            Self::SoundEffects => {
                (120..=127).contains(&program)
                    || lower.contains("applause")
                    || lower.contains("bird")
                    || lower.contains("tele")
                    || lower.contains("gun")
            }
            Self::Drums => bank == 128 || lower.contains("drum") || lower.contains("kit"),
        }
    }
}

/// Draws the SoundFont player and instrument browser, if open.
pub(crate) fn show(
    ctx: &egui::Context,
    project: &Project,
    soundfont: &SoundfontView,
    state: &mut UiState,
    actions: &mut Vec<UiAction>,
) {
    player_window(ctx, project, soundfont, state, actions);
    browser_window(ctx, project, soundfont, state, actions);
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
        .resizable(false)
        .default_width(380.0)
        .show(ctx, |ui| {
            // Header
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
            ui.add_space(4.0);

            // SoundFont selection line
            ui.horizontal(|ui| {
                ui.label(RichText::new("Bank:").size(10.5).color(theme::TEXT_DIM));
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
                        .color(Color32::from_rgb(222, 112, 102)),
                );
            }

            ui.add_space(8.0);

            // Selected Instrument Card
            egui::Frame::group(ui.style())
                .fill(theme::PANEL_RAISED)
                .stroke(Stroke::new(1.0, theme::GRID_LINE))
                .inner_margin(10.0)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("CURRENT INSTRUMENT")
                            .size(9.0)
                            .strong()
                            .color(theme::TEXT_FAINT),
                    );
                    ui.horizontal(|ui| {
                        let current_name = preset_label(soundfont, pattern.preset);
                        ui.label(
                            RichText::new(current_name)
                                .size(14.0)
                                .strong()
                                .color(theme::TEXT),
                        );
                    });
                    ui.label(
                        RichText::new(format!(
                            "Bank {:03} · Program {:03}",
                            pattern.preset.bank, pattern.preset.program
                        ))
                        .size(10.0)
                        .color(theme::TEXT_DIM),
                    );

                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        // Prev / Next stepper buttons
                        let current_idx = soundfont.presets.iter().position(|p| {
                            p.bank == pattern.preset.bank && p.program == pattern.preset.program
                        });

                        if ui
                            .button("◀ Prev")
                            .on_hover_text("Previous instrument")
                            .clicked()
                            && !soundfont.presets.is_empty()
                        {
                            let prev_idx = match current_idx {
                                Some(0) | None => soundfont.presets.len().saturating_sub(1),
                                Some(i) => i - 1,
                            };
                            let next_preset = &soundfont.presets[prev_idx];
                            let preset = Preset::new(next_preset.bank, next_preset.program);
                            actions.push(UiAction::Edit(EditCommand::SetPatternPreset {
                                pattern: pattern_id,
                                preset,
                            }));
                            actions.push(UiAction::AuditionNote {
                                track: state.selected_track,
                                preset,
                                sound_params: pattern.sound_params,
                                key: 60,
                                velocity: 100,
                            });
                        }

                        if ui
                            .button("Next ▶")
                            .on_hover_text("Next instrument")
                            .clicked()
                            && !soundfont.presets.is_empty()
                        {
                            let next_idx = match current_idx {
                                Some(i) if i + 1 < soundfont.presets.len() => i + 1,
                                _ => 0,
                            };
                            let next_preset = &soundfont.presets[next_idx];
                            let preset = Preset::new(next_preset.bank, next_preset.program);
                            actions.push(UiAction::Edit(EditCommand::SetPatternPreset {
                                pattern: pattern_id,
                                preset,
                            }));
                            actions.push(UiAction::AuditionNote {
                                track: state.selected_track,
                                preset,
                                sound_params: pattern.sound_params,
                                key: 60,
                                velocity: 100,
                            });
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let is_browser_open = state.soundfont_browser == Some(pattern_id);
                            let browse_text = if is_browser_open {
                                "Close Browser ✖"
                            } else {
                                "🔍 Browse Instruments…"
                            };
                            let btn = egui::Button::new(
                                RichText::new(browse_text)
                                    .color(if is_browser_open {
                                        theme::TEXT
                                    } else {
                                        theme::ACCENT
                                    })
                                    .strong(),
                            );
                            if ui.add(btn).clicked() {
                                if is_browser_open {
                                    state.soundfont_browser = None;
                                } else {
                                    state.soundfont_browser = Some(pattern_id);
                                }
                            }
                        });
                    });
                });

            ui.add_space(8.0);

            // Sound Shaping Section
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("SOUND SHAPING")
                        .size(9.5)
                        .strong()
                        .color(theme::TEXT_FAINT),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button("Reset")
                        .on_hover_text("Reset parameters to default")
                        .clicked()
                    {
                        actions.push(UiAction::Edit(EditCommand::SetPatternSoundParams {
                            pattern: pattern_id,
                            params: SoundParams::default(),
                        }));
                    }
                });
            });

            let mut params = pattern.sound_params;
            let mut changed = false;

            egui::Grid::new("sound_params_grid")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    // Attack
                    ui.label(RichText::new("Attack").size(11.0).color(theme::TEXT));
                    let mut attack = params.attack * 100.0;
                    if ui
                        .add(egui::Slider::new(&mut attack, 0.0..=100.0).suffix("%"))
                        .changed()
                    {
                        params.attack = attack / 100.0;
                        changed = true;
                    }
                    ui.end_row();

                    // Release / Duration
                    ui.label(RichText::new("Release").size(11.0).color(theme::TEXT));
                    let mut release = params.release * 100.0;
                    if ui
                        .add(egui::Slider::new(&mut release, 0.0..=100.0).suffix("%"))
                        .changed()
                    {
                        params.release = release / 100.0;
                        changed = true;
                    }
                    ui.end_row();

                    // Cutoff / Brightness
                    ui.label(RichText::new("Cutoff").size(11.0).color(theme::TEXT));
                    let mut cutoff = params.cutoff * 100.0;
                    if ui
                        .add(egui::Slider::new(&mut cutoff, 0.0..=100.0).suffix("%"))
                        .changed()
                    {
                        params.cutoff = cutoff / 100.0;
                        changed = true;
                    }
                    ui.end_row();

                    // Resonance
                    ui.label(RichText::new("Resonance").size(11.0).color(theme::TEXT));
                    let mut resonance = params.resonance * 100.0;
                    if ui
                        .add(egui::Slider::new(&mut resonance, 0.0..=100.0).suffix("%"))
                        .changed()
                    {
                        params.resonance = resonance / 100.0;
                        changed = true;
                    }
                    ui.end_row();

                    // Reverb Send
                    ui.label(RichText::new("Reverb").size(11.0).color(theme::TEXT));
                    let mut reverb = params.reverb * 100.0;
                    if ui
                        .add(egui::Slider::new(&mut reverb, 0.0..=100.0).suffix("%"))
                        .changed()
                    {
                        params.reverb = reverb / 100.0;
                        changed = true;
                    }
                    ui.end_row();

                    // Chorus Send
                    ui.label(RichText::new("Chorus").size(11.0).color(theme::TEXT));
                    let mut chorus = params.chorus * 100.0;
                    if ui
                        .add(egui::Slider::new(&mut chorus, 0.0..=100.0).suffix("%"))
                        .changed()
                    {
                        params.chorus = chorus / 100.0;
                        changed = true;
                    }
                    ui.end_row();
                });

            if changed {
                actions.push(UiAction::Edit(EditCommand::SetPatternSoundParams {
                    pattern: pattern_id,
                    params,
                }));
            }

            ui.add_space(10.0);

            // Audition keyboard
            ui.label(
                RichText::new("AUDITION KEYBOARD")
                    .size(9.5)
                    .strong()
                    .color(theme::TEXT_FAINT),
            );

            ui.horizontal(|ui| {
                const KEYS: [(u8, &str, bool); 13] = [
                    (60, "C4", false),
                    (61, "C#", true),
                    (62, "D4", false),
                    (63, "D#", true),
                    (64, "E4", false),
                    (65, "F4", false),
                    (66, "F#", true),
                    (67, "G4", false),
                    (68, "G#", true),
                    (69, "A4", false),
                    (70, "A#", true),
                    (71, "B4", false),
                    (72, "C5", false),
                ];

                for (key, label, is_sharp) in KEYS {
                    let fill = if is_sharp {
                        Color32::from_rgb(32, 36, 43)
                    } else {
                        Color32::from_rgb(180, 185, 192)
                    };
                    let text_color = if is_sharp {
                        Color32::from_rgb(200, 205, 212)
                    } else {
                        Color32::from_rgb(20, 22, 26)
                    };
                    let btn = egui::Button::new(
                        RichText::new(label).size(9.0).color(text_color).strong(),
                    )
                    .fill(fill)
                    .min_size(Vec2::new(18.0, 30.0));

                    if ui.add(btn).clicked() {
                        actions.push(UiAction::AuditionNote {
                            track: state.selected_track,
                            preset: pattern.preset,
                            sound_params: pattern.sound_params,
                            key,
                            velocity: 100,
                        });
                    }
                }
            });
        });

    if !open {
        state.soundfont_player = None;
    }
}

fn browser_window(
    ctx: &egui::Context,
    project: &Project,
    soundfont: &SoundfontView,
    state: &mut UiState,
    actions: &mut Vec<UiAction>,
) {
    let Some(pattern_id) = state.soundfont_browser else {
        return;
    };
    let Some(pattern) = project.pattern(pattern_id) else {
        state.soundfont_browser = None;
        return;
    };

    let mut open = true;
    egui::Window::new(format!("Instrument Browser · {}", pattern.name))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(460.0)
        .default_height(480.0)
        .min_width(360.0)
        .min_height(320.0)
        .show(ctx, |ui| {
            // Search field
            ui.horizontal(|ui| {
                ui.label(RichText::new("🔍").size(12.0));
                let search_box = ui.add_sized(
                    Vec2::new(ui.available_width() - 60.0, 22.0),
                    egui::TextEdit::singleline(&mut state.soundfont_search)
                        .hint_text("Search by name, bank, or program (e.g. piano, 0:1)..."),
                );
                if search_box.changed() {
                    ui.ctx().request_repaint();
                }
                if !state.soundfont_search.is_empty() && ui.small_button("Clear").clicked() {
                    state.soundfont_search.clear();
                }
            });

            ui.add_space(4.0);

            // Category tabs
            ScrollArea::horizontal()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for &(cat, label) in SoundCategory::TABS {
                            let selected = state.soundfont_category == cat;
                            if ui
                                .add(
                                    egui::Button::new(RichText::new(label).size(10.5))
                                        .selected(selected),
                                )
                                .clicked()
                            {
                                state.soundfont_category = cat;
                            }
                        }
                    });
                });

            ui.add_space(6.0);

            // Filter presets
            let search_term = state.soundfont_search.trim().to_lowercase();
            let matches: Vec<&SoundfontPreset> = soundfont
                .presets
                .iter()
                .filter(|p| {
                    let cat_matches = state.soundfont_category.matches(p.bank, p.program, &p.name);
                    if !cat_matches {
                        return false;
                    }
                    if search_term.is_empty() {
                        return true;
                    }
                    let p_num = format!("{}:{}", p.bank, p.program);
                    p.name.to_lowercase().contains(&search_term)
                        || p_num.contains(&search_term)
                        || p.program.to_string() == search_term
                })
                .collect();

            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "Showing {} of {} instruments",
                        matches.len(),
                        soundfont.presets.len()
                    ))
                    .size(10.0)
                    .color(theme::TEXT_DIM),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new("Click to select & audition")
                            .size(9.5)
                            .color(theme::TEXT_FAINT),
                    );
                });
            });

            ui.separator();

            if matches.is_empty() {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("No instruments found matching filters")
                            .size(11.0)
                            .color(theme::TEXT_DIM),
                    );
                });
                return;
            }

            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for preset in matches {
                        let selected = pattern.preset.bank == preset.bank
                            && pattern.preset.program == preset.program;
                        let label =
                            format!("{:03}:{:03}   {}", preset.bank, preset.program, preset.name);

                        let btn =
                            egui::Button::new(RichText::new(label).size(11.5).color(if selected {
                                theme::ACCENT
                            } else {
                                theme::TEXT
                            }))
                            .selected(selected);

                        if ui.add(btn).clicked() {
                            let p = Preset::new(preset.bank, preset.program);
                            actions.push(UiAction::Edit(EditCommand::SetPatternPreset {
                                pattern: pattern_id,
                                preset: p,
                            }));
                            actions.push(UiAction::AuditionNote {
                                track: state.selected_track,
                                preset: p,
                                sound_params: pattern.sound_params,
                                key: 60,
                                velocity: 100,
                            });
                        }
                        ui.add_space(1.0);
                    }
                });
        });

    if !open {
        state.soundfont_browser = None;
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
