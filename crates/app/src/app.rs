use std::sync::Arc;
use std::time::Duration;

use eframe::egui;
use octava_audio::{AudioEngine, TransportState};
use octava_core::{ProjectStore, Ticks};
use octava_ui::{OctavaUi, PlaybackView, UiAction};

use crate::demo::demo_project;

const METER_FALLOFF: f32 = 0.82;

pub struct OctavaApp {
    ui: OctavaUi,
    store: ProjectStore,
    playback: PlaybackView,
    engine: Option<AudioEngine>,
    last_audio_revision: u64,
    last_error: Option<String>,
}

impl OctavaApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        OctavaUi::install_theme(&cc.egui_ctx);

        let store = ProjectStore::new(demo_project());
        let mut playback = PlaybackView::default();
        let engine = match AudioEngine::start() {
            Ok(engine) => {
                playback.audio_sample_rate = Some(engine.sample_rate());
                Some(engine)
            }
            Err(error) => {
                eprintln!("Octava: audio output unavailable: {error}");
                None
            }
        };
        let last_audio_revision = store.revision();

        Self {
            ui: OctavaUi::new(),
            store,
            playback,
            engine,
            last_audio_revision,
            last_error: None,
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }

        let (space, undo, redo) = ctx.input(|input| {
            let command = input.modifiers.command;
            let shift = input.modifiers.shift;
            (
                input.key_pressed(egui::Key::Space),
                command && !shift && input.key_pressed(egui::Key::Z),
                (command && shift && input.key_pressed(egui::Key::Z))
                    || (command && input.key_pressed(egui::Key::Y)),
            )
        });

        if space {
            self.toggle_playback(ctx);
        }
        if undo {
            self.store.undo();
        }
        if redo {
            self.store.redo();
        }
    }

    fn handle_action(&mut self, ctx: &egui::Context, action: UiAction) {
        match action {
            UiAction::Play => {
                if !self.playback.playing {
                    self.start_playback(ctx);
                }
            }
            UiAction::Pause => self.pause_playback(ctx),
            UiAction::Stop => self.stop_playback(ctx),
            UiAction::Seek(tick) => {
                let tick = tick.min(self.store.project().length_ticks());
                self.playback.playhead = tick;
                if let Some(engine) = &mut self.engine
                    && let Err(error) = engine.seek(tick)
                {
                    self.last_error = Some(error.to_string());
                }
                if self.playback.playing {
                    ctx.request_repaint();
                }
            }
            UiAction::Edit(command) => {
                if let Err(error) = self.store.apply(command) {
                    self.last_error = Some(error.to_string());
                }
            }
        }
    }

    fn toggle_playback(&mut self, ctx: &egui::Context) {
        if self.playback.playing {
            self.pause_playback(ctx);
        } else {
            self.start_playback(ctx);
        }
    }

    fn start_playback(&mut self, ctx: &egui::Context) {
        let length = self.store.project().length_ticks();
        if length == Ticks::ZERO {
            return;
        }
        if self.playback.playhead >= length {
            self.playback.playhead = Ticks::ZERO;
        }

        if let Some(engine) = &mut self.engine {
            let can_resume = engine.telemetry().state() == TransportState::Paused
                && self.playback.playhead > Ticks::ZERO;
            let result = if can_resume {
                engine.resume()
            } else {
                let snapshot = Arc::new(self.store.project().compile_snapshot());
                self.last_audio_revision = self.store.revision();
                engine.play(snapshot, self.playback.playhead)
            };
            if let Err(error) = result {
                self.last_error = Some(error.to_string());
                self.playback.playing = false;
                return;
            }
        }

        self.playback.playing = true;
        ctx.request_repaint();
    }

    fn pause_playback(&mut self, ctx: &egui::Context) {
        self.playback.playing = false;
        if let Some(engine) = &mut self.engine
            && let Err(error) = engine.pause()
        {
            self.last_error = Some(error.to_string());
        }
        ctx.request_repaint();
    }

    fn stop_playback(&mut self, ctx: &egui::Context) {
        self.playback.playing = false;
        self.playback.playhead = Ticks::ZERO;
        if let Some(engine) = &mut self.engine
            && let Err(error) = engine.stop()
        {
            self.last_error = Some(error.to_string());
        }
        ctx.request_repaint();
    }

    fn pump_audio(&mut self, ctx: &egui::Context) {
        let Some(engine) = &mut self.engine else {
            return;
        };

        engine.poll();
        let telemetry = engine.telemetry();
        self.playback.playing = telemetry.state() == TransportState::Playing;
        self.playback.playhead = telemetry.playhead();
        self.playback.audio_sample_rate = Some(telemetry.sample_rate());

        let (peak_left, peak_right) = telemetry.peaks();
        self.playback.peak_left = (self.playback.peak_left * METER_FALLOFF).max(peak_left);
        self.playback.peak_right = (self.playback.peak_right * METER_FALLOFF).max(peak_right);

        if self.playback.playing
            || self.playback.peak_left > 0.001
            || self.playback.peak_right > 0.001
        {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }

    fn sync_audio_snapshot(&mut self) {
        let Some(engine) = &mut self.engine else {
            return;
        };
        let revision = self.store.revision();
        if revision == self.last_audio_revision {
            return;
        }

        let snapshot = Arc::new(self.store.project().compile_snapshot());
        match engine.set_snapshot(snapshot) {
            Ok(()) => self.last_audio_revision = revision,
            Err(error) => self.last_error = Some(error.to_string()),
        }
    }

    fn advance_transport(&mut self, ctx: &egui::Context) {
        if !self.playback.playing {
            return;
        }

        let dt = ctx.input(|input| f64::from(input.stable_dt.min(0.1)));
        let project = self.store.project();
        let length = project.length_ticks();
        if length == Ticks::ZERO {
            self.playback.playing = false;
            return;
        }

        let seconds = project.tempo_map.seconds_at(self.playback.playhead) + dt;
        let next = project.tempo_map.tick_at_seconds(seconds);
        if next >= length {
            self.playback.playhead = length;
            self.playback.playing = false;
        } else {
            self.playback.playhead = next;
            ctx.request_repaint();
        }
    }
}

impl eframe::App for OctavaApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.pump_audio(&ctx);
        self.handle_shortcuts(&ctx);
        self.sync_audio_snapshot();

        let actions = self.ui.show(ui, self.store.project(), &self.playback);
        for action in actions {
            self.handle_action(&ctx, action);
        }
        self.sync_audio_snapshot();

        if self.engine.is_none() {
            self.advance_transport(&ctx);
        }

        if let Some(error) = self.last_error.clone() {
            egui::Window::new("Command failed")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::RIGHT_BOTTOM, [-16.0, -34.0])
                .show(&ctx, |ui| {
                    ui.label(error);
                    if ui.button("Dismiss").clicked() {
                        self.last_error = None;
                    }
                });
        }
    }
}
