use egui::{Align2, Color32, ComboBox, RichText, vec2};

use crate::state::{AudioSettingsView, UiAction};
use crate::theme;

pub fn show(
    ctx: &egui::Context,
    view: &AudioSettingsView,
    open: &mut bool,
    actions: &mut Vec<UiAction>,
) {
    if !*open {
        return;
    }

    let mut is_open = *open;
    egui::Window::new("Audio Settings")
        .id(egui::Id::new("octava-audio-settings"))
        .open(&mut is_open)
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(380.0)
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);

            // Status header
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    if view.is_online {
                        ui.label(
                            RichText::new("●")
                                .size(14.0)
                                .color(Color32::from_rgb(70, 200, 120)),
                        );
                        ui.label(RichText::new("Audio Engine: Online").strong().size(12.0));
                    } else {
                        ui.label(
                            RichText::new("●")
                                .size(14.0)
                                .color(Color32::from_rgb(220, 80, 70)),
                        );
                        ui.label(RichText::new("Audio Engine: Offline").strong().size(12.0));
                    }
                });

                if let Some(err) = &view.error {
                    ui.label(
                        RichText::new(err)
                            .size(10.5)
                            .color(Color32::from_rgb(220, 100, 90)),
                    );
                } else if view.is_online {
                    ui.label(
                        RichText::new(format!(
                            "Device: {}  ·  Sample Rate: {} Hz",
                            view.active_device, view.sample_rate
                        ))
                        .size(10.5)
                        .color(theme::TEXT_DIM),
                    );
                }
            });

            ui.add_space(4.0);

            // Output Device
            ui.label(RichText::new("Output Device:").strong());
            let current_device_label = view
                .selected_device
                .as_deref()
                .unwrap_or("Default (System Default)");
            ComboBox::from_id_salt("audio-device-select")
                .selected_text(current_device_label)
                .width(360.0)
                .show_ui(ui, |ui| {
                    let is_default = view.selected_device.is_none();
                    if ui
                        .selectable_label(is_default, "Default (System Default)")
                        .clicked()
                        && !is_default
                    {
                        actions.push(UiAction::SetAudioDevice(None));
                    }
                    ui.separator();
                    for dev in &view.available_devices {
                        let selected = view.selected_device.as_deref() == Some(dev.as_str());
                        if ui.selectable_label(selected, dev).clicked() && !selected {
                            actions.push(UiAction::SetAudioDevice(Some(dev.clone())));
                        }
                    }
                });

            ui.add_space(4.0);

            // Buffer Size
            ui.label(RichText::new("Buffer Size:").strong());
            let current_buffer_label = match view.buffer_size {
                None => "Default (Device recommended)".to_owned(),
                Some(128) => "128 frames (~2.7 ms at 48 kHz)".to_owned(),
                Some(256) => "256 frames (~5.3 ms at 48 kHz)".to_owned(),
                Some(512) => "512 frames (~10.7 ms at 48 kHz)".to_owned(),
                Some(1024) => "1024 frames (~21.3 ms at 48 kHz)".to_owned(),
                Some(2048) => "2048 frames (~42.7 ms at 48 kHz)".to_owned(),
                Some(n) => format!("{n} frames"),
            };

            ComboBox::from_id_salt("audio-buffer-size-select")
                .selected_text(current_buffer_label)
                .width(360.0)
                .show_ui(ui, |ui| {
                    let is_default = view.buffer_size.is_none();
                    if ui
                        .selectable_label(is_default, "Default (Device recommended)")
                        .clicked()
                        && !is_default
                    {
                        actions.push(UiAction::SetAudioBufferSize(None));
                    }
                    const SIZES: &[(u32, &str)] = &[
                        (128, "128 frames (~2.7 ms at 48 kHz)"),
                        (256, "256 frames (~5.3 ms at 48 kHz)"),
                        (512, "512 frames (~10.7 ms at 48 kHz)"),
                        (1024, "1024 frames (~21.3 ms at 48 kHz)"),
                        (2048, "2048 frames (~42.7 ms at 48 kHz)"),
                    ];
                    for &(sz, label) in SIZES {
                        let selected = view.buffer_size == Some(sz);
                        if ui.selectable_label(selected, label).clicked() && !selected {
                            actions.push(UiAction::SetAudioBufferSize(Some(sz)));
                        }
                    }
                });

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                if ui
                    .button("▶ Play Test Tone")
                    .on_hover_text("Play a test note (A4 / 440 Hz) to verify sound output")
                    .clicked()
                {
                    actions.push(UiAction::TestAudioTone);
                }

                if ui
                    .button("⟳ Restart Engine")
                    .on_hover_text("Re-initialize the audio output stream")
                    .clicked()
                {
                    actions.push(UiAction::RestartAudioEngine);
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        actions.push(UiAction::CloseAudioSettings);
                    }
                });
            });
        });

    if !is_open {
        *open = false;
        actions.push(UiAction::CloseAudioSettings);
    }
}
