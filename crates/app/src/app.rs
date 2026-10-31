use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use eframe::egui;
use octava_audio::{
    AudioEngine, PreviewNote, SoundFontPreset, TransportState, default_soundfont_path, list_presets,
};
use octava_core::{
    EditCommand, PROJECT_EXTENSION, Project, ProjectStore, SampleCache, Ticks, decode_project_at,
    encode_project_at, load_sample, probe_sample,
};
use octava_render::{RenderSettings, render_wav};
use octava_ui::{
    DocumentView, FilesView, OctavaUi, PlaybackView, SampleEntry, SoundfontView, UiAction,
};

use crate::config::AppConfig;
use crate::demo::demo_project;

const METER_FALLOFF: f32 = 0.82;

pub struct OctavaApp {
    ui: OctavaUi,
    store: ProjectStore,
    playback: PlaybackView,
    soundfont: SoundfontView,
    files: FilesView,
    config: AppConfig,
    /// Decoded WAVs reused across snapshot compilations.
    sample_cache: SampleCache,
    /// Path of the project file, `None` for a project that has never been saved.
    project_path: Option<PathBuf>,
    /// SoundFont currently loaded into the synthesizer.
    loaded_soundfont: Option<PathBuf>,
    engine: Option<AudioEngine>,
    last_audio_revision: u64,
    last_error: Option<String>,
    notice: Option<String>,
    export_open: bool,
    export_path: String,
    dialog_tx: Sender<DialogMessage>,
    dialog_rx: Receiver<DialogMessage>,
    dialog_in_flight: bool,
}

#[derive(Debug)]
enum DialogMessage {
    SaveProjectAs(PathBuf),
    OpenProject(PathBuf),
    ChooseSoundfont(PathBuf),
    OpenSampleFolder(PathBuf),
    Error(String),
    Closed,
}

impl OctavaApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        OctavaUi::install_theme(&cc.egui_ctx);

        let config = AppConfig::load();
        let mut files = FilesView::default();
        if let Some(folder) = &config.sample_folder {
            files.folder = Some(folder.display().to_string());
        }

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
        let (dialog_tx, dialog_rx) = channel();

        let mut app = Self {
            ui: OctavaUi::new(),
            store,
            playback,
            soundfont: SoundfontView::default(),
            files,
            config,
            sample_cache: SampleCache::default(),
            project_path: None,
            loaded_soundfont: None,
            engine,
            last_audio_revision,
            last_error: None,
            notice: None,
            export_open: false,
            export_path: String::new(),
            dialog_tx,
            dialog_rx,
            dialog_in_flight: false,
        };
        app.refresh_soundfont();
        if app.files.folder.is_some() {
            app.refresh_samples();
        }
        app
    }

    /// Loads the SoundFont requested by the project and refreshes the preset
    /// list shown in the SoundFont player.
    fn sync_soundfont(&mut self) {
        let requested = self.store.project().soundfont.clone();
        if requested == self.loaded_soundfont {
            return;
        }

        let path = requested.clone().unwrap_or_else(default_soundfont_path);
        self.loaded_soundfont = requested;
        self.refresh_soundfont();
        if let Some(engine) = &mut self.engine
            && let Err(error) = engine.set_soundfont(&path)
        {
            self.last_error = Some(error.to_string());
        }
    }

    fn refresh_soundfont(&mut self) {
        let requested = self.loaded_soundfont.clone();
        let path = requested.clone().unwrap_or_else(default_soundfont_path);
        self.soundfont.path = requested.map(|path| path.display().to_string());
        match list_presets(&path) {
            Ok(presets) => {
                self.soundfont.presets = presets.into_iter().map(ui_preset).collect();
                self.soundfont.error = None;
            }
            Err(error) => {
                self.soundfont.presets.clear();
                self.soundfont.error = Some(error.to_string());
            }
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }

        let (space, undo, redo, open, save, save_as) = ctx.input(|input| {
            let command = input.modifiers.command;
            let shift = input.modifiers.shift;
            (
                input.key_pressed(egui::Key::Space),
                command && !shift && input.key_pressed(egui::Key::Z),
                (command && shift && input.key_pressed(egui::Key::Z))
                    || (command && input.key_pressed(egui::Key::Y)),
                command && !shift && input.key_pressed(egui::Key::O),
                command && !shift && input.key_pressed(egui::Key::S),
                command && shift && input.key_pressed(egui::Key::S),
            )
        });

        if open {
            self.open_project(ctx);
        }
        if save_as {
            self.save_project_as(ctx);
        } else if save {
            self.save_project(ctx);
        }
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
            UiAction::Undo => {
                if !self.store.undo() {
                    self.last_error = Some("Nothing to undo".to_owned());
                }
            }
            UiAction::Redo => {
                if !self.store.redo() {
                    self.last_error = Some("Nothing to redo".to_owned());
                }
            }
            UiAction::NewProject => self.new_project(ctx),
            UiAction::OpenProject => self.open_project(ctx),
            UiAction::OpenRecentProject(path) => self.open_project_at_path(path, ctx),
            UiAction::ClearRecentProjects => self.config.clear_recent(),
            UiAction::SaveProject => self.save_project(ctx),
            UiAction::SaveProjectAs => self.save_project_as(ctx),
            UiAction::OpenSampleFolder => self.open_sample_folder(ctx),
            UiAction::SetSampleFolder(path) => self.load_sample_folder(path),
            UiAction::RefreshSamples => self.refresh_samples(),
            UiAction::AuditionSample(path) => self.audition_sample(path),
            UiAction::AuditionNote {
                track,
                preset,
                key,
                velocity,
            } => {
                if self.playback.playing {
                    return;
                }
                let Some(track) = self.store.project().track(track) else {
                    return;
                };
                let preview = PreviewNote {
                    channel: track.channel,
                    preset,
                    gain: track.gain,
                    pan: track.pan,
                    key,
                    velocity,
                };
                if let Some(engine) = &mut self.engine
                    && let Err(error) = engine.preview_note(preview)
                {
                    self.last_error = Some(error.to_string());
                }
                ctx.request_repaint();
            }
            UiAction::ExportWav => {
                self.export_open = true;
                self.export_path = default_export_path(self.store.project());
            }
            UiAction::ChooseSoundfont => self.choose_soundfont(ctx),
        }
    }

    fn pump_dialogs(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.dialog_rx.try_recv() {
            self.dialog_in_flight = false;
            match msg {
                DialogMessage::SaveProjectAs(path) => {
                    self.write_project(&path);
                }
                DialogMessage::OpenProject(path) => {
                    self.open_project_at_path(path, ctx);
                }
                DialogMessage::ChooseSoundfont(path) => {
                    if let Err(error) = self
                        .store
                        .apply(EditCommand::SetSoundfont { path: Some(path) })
                    {
                        self.last_error = Some(error.to_string());
                    }
                }
                DialogMessage::OpenSampleFolder(path) => {
                    self.load_sample_folder(path);
                }
                DialogMessage::Error(error) => {
                    self.last_error = Some(error);
                }
                DialogMessage::Closed => {}
            }
        }
    }

    /// Opens the desktop's native file picker for SoundFonts on a background thread.
    fn choose_soundfont(&mut self, ctx: &egui::Context) {
        if self.dialog_in_flight {
            return;
        }
        self.dialog_in_flight = true;

        let default_dir = self
            .store
            .project()
            .soundfont
            .as_deref()
            .and_then(|path| path.parent())
            .map(Path::to_path_buf)
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from));

        let tx = self.dialog_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            if let Some(choice) = zenity_open_file(
                "Choose a SoundFont",
                "SoundFont (*.sf2, *.sf3)",
                "*.sf2 *.sf3",
                default_dir.as_deref(),
            ) {
                if let Some(path) = choice {
                    let _ = tx.send(DialogMessage::ChooseSoundfont(path));
                } else {
                    let _ = tx.send(DialogMessage::Closed);
                }
                ctx.request_repaint();
                return;
            }

            let mut dialog = rfd::AsyncFileDialog::new()
                .set_title("Choose a SoundFont")
                .add_filter("SoundFont", &["sf2", "sf3"]);
            if let Some(ref dir) = default_dir {
                dialog = dialog.set_directory(dir);
            }

            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(error) => {
                    let _ = tx.send(DialogMessage::Error(error.to_string()));
                    ctx.request_repaint();
                    return;
                }
            };
            if let Some(file) = runtime.block_on(dialog.pick_file()) {
                let _ = tx.send(DialogMessage::ChooseSoundfont(file.path().to_path_buf()));
            } else {
                let _ = tx.send(DialogMessage::Closed);
            }
            ctx.request_repaint();
        });
    }

    fn open_project(&mut self, ctx: &egui::Context) {
        if self.dialog_in_flight {
            return;
        }
        self.dialog_in_flight = true;

        let default_dir = self
            .project_path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .or_else(|| {
                self.config
                    .recent_projects
                    .first()
                    .and_then(|p| p.parent().map(Path::to_path_buf))
            })
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Documents")))
            .or_else(|| std::env::current_dir().ok());

        let tx = self.dialog_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            if let Some(choice) = zenity_open_file(
                "Open Project",
                "Octava project",
                &format!("*.{PROJECT_EXTENSION}"),
                default_dir.as_deref(),
            ) {
                if let Some(path) = choice {
                    let _ = tx.send(DialogMessage::OpenProject(path));
                } else {
                    let _ = tx.send(DialogMessage::Closed);
                }
                ctx.request_repaint();
                return;
            }

            let mut dialog = rfd::AsyncFileDialog::new()
                .set_title("Open Project")
                .add_filter("Octava project", &[PROJECT_EXTENSION]);
            if let Some(ref dir) = default_dir {
                dialog = dialog.set_directory(dir);
            }

            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(error) => {
                    let _ = tx.send(DialogMessage::Error(error.to_string()));
                    ctx.request_repaint();
                    return;
                }
            };
            if let Some(file) = runtime.block_on(dialog.pick_file()) {
                let _ = tx.send(DialogMessage::OpenProject(file.path().to_path_buf()));
            } else {
                let _ = tx.send(DialogMessage::Closed);
            }
            ctx.request_repaint();
        });
    }

    fn open_project_at_path(&mut self, path: PathBuf, ctx: &egui::Context) {
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                self.last_error = Some(format!("Could not read {}: {error}", path.display()));
                return;
            }
        };
        let project = match decode_project_at(&text, &path) {
            Ok(project) => project,
            Err(error) => {
                self.last_error = Some(format!("Could not open {}: {error}", path.display()));
                return;
            }
        };

        self.stop_playback(ctx);
        self.store = ProjectStore::new(project);
        self.project_path = Some(path.clone());
        self.config.add_recent(path.clone());
        self.last_audio_revision = self.store.revision();
        self.sample_cache.clear();
        self.playback.playhead = Ticks::ZERO;
        self.playback.peak_left = 0.0;
        self.playback.peak_right = 0.0;
        self.ui.project_replaced();
        self.last_error = None;
        self.notice = Some(format!("Opened {}", path.display()));
        ctx.request_repaint();
    }

    fn save_project(&mut self, ctx: &egui::Context) {
        let Some(path) = self.project_path.clone() else {
            self.save_project_as(ctx);
            return;
        };
        self.write_project(&path);
    }

    fn save_project_as(&mut self, ctx: &egui::Context) {
        if self.dialog_in_flight {
            return;
        }
        self.dialog_in_flight = true;

        let default_dir = self
            .project_path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .or_else(|| {
                self.config
                    .recent_projects
                    .first()
                    .and_then(|p| p.parent().map(Path::to_path_buf))
            })
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Documents")))
            .or_else(|| std::env::current_dir().ok());

        let default_name = self
            .project_path
            .as_ref()
            .and_then(|p| p.file_name().and_then(|n| n.to_str()).map(str::to_owned))
            .unwrap_or_else(|| format!("{}.{PROJECT_EXTENSION}", slug(&self.store.project().name)));

        let default_path = default_dir.as_ref().map(|dir| dir.join(&default_name));

        let tx = self.dialog_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            if let Some(choice) = zenity_save_file(
                "Save Project As",
                "Octava project",
                PROJECT_EXTENSION,
                default_path.as_deref(),
            ) {
                if let Some(path) = choice {
                    let _ = tx.send(DialogMessage::SaveProjectAs(path));
                } else {
                    let _ = tx.send(DialogMessage::Closed);
                }
                ctx.request_repaint();
                return;
            }

            let mut dialog = rfd::AsyncFileDialog::new()
                .set_title("Save Project As")
                .add_filter("Octava project", &[PROJECT_EXTENSION])
                .set_file_name(&default_name);
            if let Some(ref dir) = default_dir {
                dialog = dialog.set_directory(dir);
            }

            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(error) => {
                    let _ = tx.send(DialogMessage::Error(error.to_string()));
                    ctx.request_repaint();
                    return;
                }
            };
            if let Some(file) = runtime.block_on(dialog.save_file()) {
                let mut path = file.path().to_path_buf();
                if path.extension().is_none() {
                    path.set_extension(PROJECT_EXTENSION);
                }
                let _ = tx.send(DialogMessage::SaveProjectAs(path));
            } else {
                let _ = tx.send(DialogMessage::Closed);
            }
            ctx.request_repaint();
        });
    }

    fn write_project(&mut self, path: &Path) {
        let text = match encode_project_at(self.store.project(), path) {
            Ok(text) => text,
            Err(error) => {
                self.last_error = Some(error.to_string());
                return;
            }
        };
        match std::fs::write(path, text) {
            Ok(()) => {
                self.store.mark_saved();
                self.project_path = Some(path.to_path_buf());
                self.config.add_recent(path.to_path_buf());
                self.last_error = None;
                self.notice = Some(format!("Saved {}", path.display()));
            }
            Err(error) => {
                self.last_error = Some(format!("Could not save {}: {error}", path.display()));
            }
        }
    }

    fn open_sample_folder(&mut self, ctx: &egui::Context) {
        if self.dialog_in_flight {
            return;
        }
        self.dialog_in_flight = true;

        let default_dir = self
            .files
            .folder
            .as_deref()
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from));

        let tx = self.dialog_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            if let Some(choice) =
                zenity_pick_folder("Choose a sample folder", default_dir.as_deref())
            {
                if let Some(folder) = choice {
                    let _ = tx.send(DialogMessage::OpenSampleFolder(folder));
                } else {
                    let _ = tx.send(DialogMessage::Closed);
                }
                ctx.request_repaint();
                return;
            }

            let mut dialog = rfd::AsyncFileDialog::new().set_title("Choose a sample folder");
            if let Some(ref dir) = default_dir {
                dialog = dialog.set_directory(dir);
            }

            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(error) => {
                    let _ = tx.send(DialogMessage::Error(error.to_string()));
                    ctx.request_repaint();
                    return;
                }
            };
            if let Some(folder) = runtime.block_on(dialog.pick_folder()) {
                let _ = tx.send(DialogMessage::OpenSampleFolder(folder.path().to_path_buf()));
            } else {
                let _ = tx.send(DialogMessage::Closed);
            }
            ctx.request_repaint();
        });
    }

    fn load_sample_folder(&mut self, folder: PathBuf) {
        self.config.set_sample_folder(Some(folder.clone()));
        self.files.folder = Some(folder.display().to_string());
        self.sample_cache.clear();
        self.refresh_samples();
    }

    fn refresh_samples(&mut self) {
        let Some(folder) = self.files.folder.clone() else {
            return;
        };
        self.sample_cache.clear();
        let (entries, error) = scan_sample_folder(Path::new(&folder));
        self.files.entries = entries;
        self.files.error = error;
    }

    fn audition_sample(&mut self, path: PathBuf) {
        let data = match load_sample(&path) {
            Ok(data) => Arc::new(data),
            Err(error) => {
                self.last_error = Some(error.to_string());
                return;
            }
        };
        if let Some(engine) = &mut self.engine
            && let Err(error) = engine.preview_sample(data)
        {
            self.last_error = Some(error.to_string());
        }
    }

    fn document_view(&self) -> DocumentView {
        DocumentView {
            path: self
                .project_path
                .as_ref()
                .map(|path| path.display().to_string()),
            dirty: self.store.is_dirty(),
            recent_projects: self.config.recent_projects.clone(),
        }
    }

    fn new_project(&mut self, ctx: &egui::Context) {
        self.playback.playing = false;
        self.playback.playhead = Ticks::ZERO;
        self.playback.peak_left = 0.0;
        self.playback.peak_right = 0.0;
        if let Some(engine) = &mut self.engine
            && let Err(error) = engine.stop()
        {
            self.last_error = Some(error.to_string());
        }

        let mut project = Project::new("Untitled");
        if let Ok(track) = project.add_track("Instrument") {
            let bar = project
                .time_signature
                .ticks_per_bar(project.tempo_map.ppq());
            if let Ok(pattern) = project.add_pattern("Clip 1", Ticks(bar.saturating_mul(4))) {
                let _ = project.add_clip(track, pattern, Ticks::ZERO);
            }
        }
        self.store = ProjectStore::new(project);
        self.project_path = None;
        self.sample_cache.clear();
        self.last_audio_revision = self.store.revision();
        self.ui.project_replaced();
        self.last_error = None;
        self.notice = None;
        ctx.request_repaint();
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

        let can_resume = self.engine.as_ref().is_some_and(|engine| {
            engine.telemetry().state() == TransportState::Paused
                && self.playback.playhead > Ticks::ZERO
        });
        let snapshot = if can_resume {
            None
        } else {
            let snapshot = Arc::new(
                self.store
                    .project()
                    .compile_snapshot_with(&mut self.sample_cache),
            );
            self.last_audio_revision = self.store.revision();
            Some(snapshot)
        };

        if let Some(engine) = &mut self.engine {
            let result = match snapshot {
                Some(snapshot) => engine.play(snapshot, self.playback.playhead),
                None => engine.resume(),
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
        if self.engine.is_none() {
            return;
        }
        let revision = self.store.revision();
        if revision == self.last_audio_revision {
            return;
        }

        let snapshot = Arc::new(
            self.store
                .project()
                .compile_snapshot_with(&mut self.sample_cache),
        );
        if let Some(engine) = &mut self.engine {
            match engine.set_snapshot(snapshot) {
                Ok(()) => self.last_audio_revision = revision,
                Err(error) => self.last_error = Some(error.to_string()),
            }
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

fn ui_preset(preset: SoundFontPreset) -> octava_ui::SoundfontPreset {
    octava_ui::SoundfontPreset {
        name: preset.name,
        bank: preset.bank,
        program: preset.program,
    }
}

impl eframe::App for OctavaApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.pump_dialogs(&ctx);
        self.pump_audio(&ctx);
        self.handle_shortcuts(&ctx);
        self.sync_audio_snapshot();

        self.sync_soundfont();
        let document = self.document_view();
        let actions = self.ui.show(
            ui,
            self.store.project(),
            &self.playback,
            &self.soundfont,
            &self.files,
            &document,
        );
        for action in actions {
            self.handle_action(&ctx, action);
        }
        self.sync_soundfont();
        self.sync_audio_snapshot();

        if self.engine.is_none() {
            self.advance_transport(&ctx);
        }

        if self.export_open {
            export_dialog(
                &ctx,
                &mut self.export_open,
                &mut self.export_path,
                self.store.project(),
                &mut self.last_error,
                &mut self.notice,
            );
        }

        if let Some(message) = self.notice.clone() {
            egui::Window::new("Notice")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::RIGHT_BOTTOM, [-16.0, -64.0])
                .show(&ctx, |ui| {
                    ui.label(message);
                    if ui.button("Dismiss").clicked() {
                        self.notice = None;
                    }
                });
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

fn default_export_path(project: &Project) -> String {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(format!("{}.wav", slug(&project.name)))
        .display()
        .to_string()
}

fn export_dialog(
    ctx: &egui::Context,
    open: &mut bool,
    path: &mut String,
    project: &Project,
    last_error: &mut Option<String>,
    notice: &mut Option<String>,
) {
    let mut close_requested = false;
    egui::Window::new("Export WAV")
        .open(open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label("Output file");
            ui.add(
                egui::TextEdit::singleline(path)
                    .desired_width(420.0)
                    .hint_text("path/to/output.wav"),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("Stereo 16-bit PCM · 44.1 kHz · 1 s release tail")
                    .size(10.0)
                    .color(egui::Color32::GRAY),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Render WAV").clicked() {
                    let mut output = PathBuf::from(path.trim());
                    if output.as_os_str().is_empty() {
                        *last_error = Some("Export path must not be empty".to_owned());
                        return;
                    }
                    if output.extension().is_none() {
                        output.set_extension("wav");
                    }
                    *path = output.display().to_string();
                    match render_wav(project, &output, RenderSettings::default()) {
                        Ok(()) => {
                            *notice = Some(format!("Rendered {}", output.display()));
                            *last_error = None;
                            close_requested = true;
                        }
                        Err(error) => {
                            *last_error = Some(error.to_string());
                        }
                    }
                }
                if ui.button("Cancel").clicked() {
                    close_requested = true;
                }
            });
        });
    if close_requested {
        *open = false;
    }
}

fn scan_sample_folder(folder: &Path) -> (Vec<SampleEntry>, Option<String>) {
    let mut entries = Vec::new();
    let mut failures = 0usize;
    let directory = match std::fs::read_dir(folder) {
        Ok(directory) => directory,
        Err(error) => {
            return (
                entries,
                Some(format!("Could not read {}: {error}", folder.display())),
            );
        }
    };

    for entry in directory.flatten() {
        let path = entry.path();
        let is_wav = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("wav"));
        if !is_wav || !path.is_file() {
            continue;
        }
        match probe_sample(&path) {
            Ok(info) => entries.push(SampleEntry {
                name: path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("sample")
                    .to_owned(),
                path,
                duration_seconds: info.duration_seconds,
                sample_rate: info.sample_rate,
                channels: info.channels,
            }),
            Err(error) => {
                failures += 1;
                eprintln!("Octava: {error}");
            }
        }
        if entries.len() >= 500 {
            break;
        }
    }

    entries.sort_by_key(|entry| entry.name.to_lowercase());
    let error = (failures > 0).then(|| format!("{failures} WAV file(s) could not be read"));
    (entries, error)
}

fn slug(name: &str) -> String {
    let mut slug = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "octava".to_owned()
    } else {
        slug.to_owned()
    }
}

fn zenity_save_file(
    title: &str,
    filter_name: &str,
    filter_ext: &str,
    default_path: Option<&Path>,
) -> Option<Option<PathBuf>> {
    let mut cmd = std::process::Command::new("zenity");
    cmd.arg("--file-selection")
        .arg("--save")
        .arg("--confirm-overwrite")
        .arg(format!("--title={title}"))
        .arg(format!("--file-filter={filter_name} | *.{filter_ext}"));
    if let Some(path) = default_path {
        cmd.arg(format!("--filename={}", path.display()));
    }
    let output = cmd.output().ok()?;
    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if s.is_empty() {
            Some(None)
        } else {
            let mut path = PathBuf::from(s);
            if path.extension().is_none() {
                path.set_extension(filter_ext);
            }
            Some(Some(path))
        }
    } else if output.status.code() == Some(1) {
        Some(None)
    } else {
        None
    }
}

fn zenity_open_file(
    title: &str,
    filter_name: &str,
    filter_pattern: &str,
    default_dir: Option<&Path>,
) -> Option<Option<PathBuf>> {
    let mut cmd = std::process::Command::new("zenity");
    cmd.arg("--file-selection")
        .arg(format!("--title={title}"))
        .arg(format!("--file-filter={filter_name} | {filter_pattern}"));
    if let Some(dir) = default_dir {
        let dir_str = dir.display().to_string();
        let filename_arg = if dir_str.ends_with('/') {
            dir_str
        } else {
            format!("{dir_str}/")
        };
        cmd.arg(format!("--filename={filename_arg}"));
    }
    let output = cmd.output().ok()?;
    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if s.is_empty() {
            Some(None)
        } else {
            Some(Some(PathBuf::from(s)))
        }
    } else if output.status.code() == Some(1) {
        Some(None)
    } else {
        None
    }
}

fn zenity_pick_folder(title: &str, default_dir: Option<&Path>) -> Option<Option<PathBuf>> {
    let mut cmd = std::process::Command::new("zenity");
    cmd.arg("--file-selection")
        .arg("--directory")
        .arg(format!("--title={title}"));
    if let Some(dir) = default_dir {
        let dir_str = dir.display().to_string();
        let filename_arg = if dir_str.ends_with('/') {
            dir_str
        } else {
            format!("{dir_str}/")
        };
        cmd.arg(format!("--filename={filename_arg}"));
    }
    let output = cmd.output().ok()?;
    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if s.is_empty() {
            Some(None)
        } else {
            Some(Some(PathBuf::from(s)))
        }
    } else if output.status.code() == Some(1) {
        Some(None)
    } else {
        None
    }
}
