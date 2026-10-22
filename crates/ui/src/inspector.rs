use egui::RichText;
use octava_core::{EditCommand, Project};

use crate::state::{ClipRef, UiAction, UiState};
use crate::theme;

pub(crate) fn show(
    ui: &mut egui::Ui,
    project: &Project,
    state: &mut UiState,
    actions: &mut Vec<UiAction>,
) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(4.0);
            project_summary(ui, project);
            ui.add_space(10.0);
            track_list(ui, project, state);
            ui.add_space(10.0);

            if let Some(track) = state.selected_track(project) {
                track_details(ui, project, track, actions);
                ui.add_space(10.0);
                clips_section(ui, project, state);
            }

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "Space: preview play/pause\nCtrl+Z / Ctrl+Shift+Z: undo / redo\nCtrl+wheel in piano roll: zoom",
                )
                .size(10.0)
                .color(theme::TEXT_FAINT),
            );
        });
}

fn project_summary(ui: &mut egui::Ui, project: &Project) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("OCTAVA")
                .size(17.0)
                .strong()
                .color(theme::ACCENT),
        );
        ui.label(RichText::new("project").size(10.0).color(theme::TEXT_FAINT));
    });
    ui.add_space(4.0);
    ui.label(RichText::new(&project.name).size(15.0).color(theme::TEXT));

    let bar_ticks = project
        .time_signature
        .ticks_per_bar(project.tempo_map.ppq())
        .max(1) as f64;
    let bars = project.length_ticks().0 as f64 / bar_ticks;

    egui::Grid::new("project-summary")
        .num_columns(2)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            detail_row(
                ui,
                "Tempo",
                &format!(
                    "{:.1} BPM",
                    project.tempo_map.tempo_at(octava_core::Ticks::ZERO)
                ),
            );
            detail_row(
                ui,
                "Meter",
                &format!(
                    "{}/{}",
                    project.time_signature.numerator, project.time_signature.denominator
                ),
            );
            detail_row(ui, "Length", &format!("{bars:.1} bars"));
            detail_row(
                ui,
                "Content",
                &format!(
                    "{} tracks · {} notes",
                    project.tracks.len(),
                    project.note_count()
                ),
            );
        });
}

fn track_list(ui: &mut egui::Ui, project: &Project, state: &mut UiState) {
    section(ui, "TRACKS");
    if project.tracks.is_empty() {
        ui.label(
            RichText::new("No tracks in this project")
                .size(11.0)
                .color(theme::TEXT_FAINT),
        );
        return;
    }

    for (index, track) in project.tracks.iter().enumerate() {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(9.0, 9.0), egui::Sense::hover());
            ui.painter()
                .circle_filled(rect.center(), 3.5, theme::track_color(index));

            let selected = state.selected_track == Some(track.id);
            if ui
                .selectable_label(selected, RichText::new(&track.name).size(12.0))
                .clicked()
            {
                state.select_track(project, track.id);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{} notes", project.track_note_count(track.id)))
                        .size(9.5)
                        .color(theme::TEXT_FAINT),
                );
            });
        });
    }
}

fn track_details(
    ui: &mut egui::Ui,
    _project: &Project,
    track: &octava_core::Track,
    actions: &mut Vec<UiAction>,
) {
    section(ui, "SELECTED TRACK");
    ui.label(
        RichText::new(&track.name)
            .size(14.0)
            .strong()
            .color(theme::TEXT),
    );
    ui.add_space(4.0);

    egui::Grid::new("track-details")
        .num_columns(2)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            detail_row(
                ui,
                "Channel",
                &format!(
                    "{} (MIDI {})",
                    u16::from(track.channel) + 1,
                    track.channel + 1
                ),
            );
            detail_row(
                ui,
                "Preset",
                &format!(
                    "bank {} · program {}",
                    track.preset.bank, track.preset.program
                ),
            );
            detail_row(ui, "Gain", &format!("{:.2}", track.gain));
            detail_row(ui, "Pan", &pan_label(track.pan));
        });

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let mut mute = track.mute;
        if ui.toggle_value(&mut mute, "Mute").changed() {
            actions.push(UiAction::Edit(EditCommand::SetTrackParams {
                track: track.id,
                gain: None,
                pan: None,
                mute: Some(mute),
                solo: None,
                preset: None,
            }));
        }

        let mut solo = track.solo;
        if ui.toggle_value(&mut solo, "Solo").changed() {
            actions.push(UiAction::Edit(EditCommand::SetTrackParams {
                track: track.id,
                gain: None,
                pan: None,
                mute: None,
                solo: Some(solo),
                preset: None,
            }));
        }
    });
}

fn clips_section(ui: &mut egui::Ui, project: &Project, state: &mut UiState) {
    section(ui, "CLIPS");
    let Some(track) = state.selected_track(project) else {
        return;
    };

    if track.clips.is_empty() {
        ui.label(
            RichText::new("No clips on this track")
                .size(11.0)
                .color(theme::TEXT_FAINT),
        );
        return;
    }

    let bar_ticks = project
        .time_signature
        .ticks_per_bar(project.tempo_map.ppq())
        .max(1) as f64;

    for clip in &track.clips {
        let Some(pattern) = project.pattern(clip.pattern) else {
            continue;
        };
        let start = clip.start.0 as f64 / bar_ticks + 1.0;
        let end = clip.end().0 as f64 / bar_ticks + 1.0;
        let selection = ClipRef {
            track: track.id,
            clip: clip.id,
        };
        let selected = state.selected_clip == Some(selection);
        let label = format!(
            "{}  ·  {} notes  ·  {start:.1}–{end:.1}",
            pattern.name,
            pattern.notes.len()
        );

        if ui
            .selectable_label(selected, RichText::new(label).size(11.0))
            .clicked()
        {
            state.select_clip(project, track.id, clip.id);
        }
    }

    if let Some(clip) = state.selected_clip(project)
        && let Some(pattern) = project.pattern(clip.pattern)
    {
        ui.add_space(8.0);
        section(ui, "PATTERN");
        egui::Grid::new("pattern-details")
            .num_columns(2)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                detail_row(ui, "Name", &pattern.name);
                detail_row(
                    ui,
                    "Instrument",
                    crate::clips::instrument_name(pattern.preset),
                );
                detail_row(
                    ui,
                    "Used",
                    &format!("{}× on the timeline", project.pattern_use_count(pattern.id)),
                );
            });
        ui.label(
            RichText::new("Editing a pattern updates every clip that plays it.")
                .size(9.5)
                .color(theme::TEXT_FAINT),
        );
    }
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(2.0);
    ui.label(
        RichText::new(title)
            .size(9.5)
            .strong()
            .color(theme::TEXT_FAINT),
    );
    ui.add_space(2.0);
}

fn detail_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(RichText::new(label).size(10.5).color(theme::TEXT_DIM));
    ui.label(RichText::new(value).size(10.5).color(theme::TEXT));
    ui.end_row();
}

fn pan_label(pan: f32) -> String {
    if pan.abs() < 0.005 {
        "center".to_owned()
    } else if pan < 0.0 {
        format!("L{:.0}", -pan * 100.0)
    } else {
        format!("R{:.0}", pan * 100.0)
    }
}
