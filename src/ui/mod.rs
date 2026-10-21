//! The window: master toggle and meters on top, devices on the left, effects and soundboard in
//! the middle.

pub mod devices;
pub mod effects;
pub mod meters;
pub mod presets;
pub mod soundboard;
pub mod theme;
pub mod toast;

use crate::audio::device_manager::{DeviceStatus, Event, ManagerCmd, ManagerHandle};
use crate::audio::shared::{ActiveMic, Shared};
use crate::config::{self, Config};
use crate::dsp::params::Snapshot;
use crate::soundboard::decode;
use crate::soundboard::hotkeys::{Captured, HotkeyService};
use crate::soundboard::library::{SoundEntry, Soundboard};
use crate::soundboard::mixer::SoundId;
use egui::{Color32, RichText, Ui};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::time::Duration;
use theme::{BAD, OFF, ON, WARN};
use toast::Toasts;

/// How long the clip lamp stays lit after a clip.
const CLIP_HOLD_S: f64 = 1.5;

pub use crate::config::Tab;

pub struct VoxApp {
    pub cfg: Config,
    /// Current effect values (mirrored into the shared atomics on every change).
    pub fx: Snapshot,
    shared: Arc<Shared>,
    manager: ManagerHandle,
    status: DeviceStatus,
    toasts: Toasts,
    clip_seen: u32,
    clip_until: f64,
    defaults_applied: bool,
    sounds: Soundboard,
    hotkeys: Option<HotkeyService>,
    /// Sound waiting for its hotkey to be pressed.
    capturing: Option<SoundId>,
    preset_name: String,
    /// Where settings are saved (None in tests).
    config_path: Option<PathBuf>,
    saved: Config,
    changed_at: Option<f64>,
    /// Shown as a toast on the first frame (e.g. "settings file was damaged").
    startup_notice: Option<String>,
}

/// Save this long after the last change, so dragging a slider does not write 30 times a second.
const SAVE_DELAY_S: f64 = 1.0;

/// Pushes the non-device parts of the config into the shared atomics.
fn apply_to_shared(cfg: &Config, shared: &Shared) {
    shared.voice_on.store(cfg.voice_on, Relaxed);
    shared.monitor_volume.set(cfg.monitor_volume);
    shared.monitor_voice.store(cfg.monitor_voice, Relaxed);
    shared.monitor_sounds.store(cfg.monitor_sounds, Relaxed);
    shared.params.store(&Snapshot::from_settings(&cfg.effects));
}

impl VoxApp {
    /// Loads the settings from `config_path`, starts audio and hotkeys.
    pub fn new(config_path: PathBuf) -> Self {
        let (cfg, notice) = config::load_from(&config_path);
        let shared = Arc::new(Shared::default());
        let manager = ManagerHandle::spawn(shared.clone(), cfg.devices.clone(), cfg.mic.clone());
        let mut app = Self::with_manager(cfg, shared, manager);
        app.hotkeys = Some(HotkeyService::start(app.sounds.board.clone()));
        app.config_path = Some(config_path);
        app.startup_notice = notice;
        app
    }

    fn with_manager(cfg: Config, shared: Arc<Shared>, manager: ManagerHandle) -> Self {
        apply_to_shared(&cfg, &shared);
        let sounds = Soundboard::new(manager.port.clone(), cfg.sounds.clone());
        Self {
            sounds,
            hotkeys: None,
            capturing: None,
            preset_name: String::new(),
            config_path: None,
            saved: cfg.clone(),
            changed_at: None,
            startup_notice: None,
            fx: Snapshot::from_settings(&cfg.effects),
            cfg,
            shared,
            manager,
            status: DeviceStatus::default(),
            toasts: Toasts::default(),
            clip_seen: 0,
            clip_until: 0.0,
            defaults_applied: false,
        }
    }

    /// On the very first scan, pick VB-CABLE as output and the Windows default mic as primary
    /// if nothing is configured yet.
    fn first_run_defaults(&mut self) {
        if self.defaults_applied || !self.status.scanned {
            return;
        }
        self.defaults_applied = true;
        let d = &mut self.cfg.devices;
        let mut changed = false;
        if d.output.is_empty()
            && let Some(cable) = &self.status.cable
        {
            d.output = cable.clone();
            changed = true;
        }
        if d.primary.is_empty()
            && d.fallback.is_empty()
            && let Some(mic) = &self.status.default_input
        {
            d.primary = mic.clone();
            changed = true;
        }
        if changed {
            self.manager.send(ManagerCmd::Devices(d.clone()));
        }
    }

    pub fn draw(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.status = self.manager.status();
        self.first_run_defaults();
        self.sounds.lock().set_output_rate(self.status.output.rate);
        while let Ok(ev) = self.manager.events.try_recv() {
            match ev {
                Event::Switched { to, reason } => {
                    let text = match to {
                        ActiveMic::Primary => "Switched to the primary mic".to_string(),
                        ActiveMic::Fallback => format!("Switched to the fallback mic ({reason})"),
                        ActiveMic::None => "No microphone available - sending silence".to_string(),
                    };
                    self.toasts.push(&ctx, text);
                }
            }
        }

        egui::Panel::top("top").show(ui, |ui| self.top_bar(ui));
        egui::Panel::left("devices")
            .resizable(true)
            .default_size(340.0)
            .min_size(280.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let ch = devices::panel(ui, &mut self.cfg, &self.status, &self.shared);
                    if ch.devices {
                        self.manager
                            .send(ManagerCmd::Devices(self.cfg.devices.clone()));
                    }
                    if ch.mic {
                        self.manager.send(ManagerCmd::Mic(self.cfg.mic.clone()));
                    }
                    if ch.monitor {
                        apply_to_shared(&self.cfg, &self.shared);
                    }
                    if ch.rescan {
                        self.manager.send(ManagerCmd::Rescan);
                    }
                });
            });
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut self.cfg.tab,
                    Tab::Effects,
                    RichText::new("Effects").heading(),
                );
                ui.selectable_value(
                    &mut self.cfg.tab,
                    Tab::Soundboard,
                    RichText::new("Soundboard").heading(),
                );
            });
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| match self.cfg.tab {
                Tab::Effects => {
                    if let Some(a) = presets::bar(ui, &self.cfg, &self.fx, &mut self.preset_name) {
                        self.apply_preset_action(ui.ctx(), a);
                    }
                    ui.add_space(6.0);
                    if effects::panel(ui, &mut self.fx) {
                        self.effects_changed();
                    }
                }
                Tab::Soundboard => self.soundboard_tab(ui),
            });
        });

        self.handle_dropped_files(&ctx);
        self.poll_hotkey_capture(&ctx);
        if let Some(n) = self.startup_notice.take() {
            self.toasts.push(&ctx, n);
        }
        self.autosave(ctx.input(|i| i.time));
        self.toasts.show(&ctx);
        ctx.request_repaint_after(Duration::from_millis(33));
    }

    fn soundboard_tab(&mut self, ui: &mut Ui) {
        let actions = {
            let board = self.sounds.lock();
            let view = soundboard::View {
                board: &board,
                shared: &self.shared,
                capturing: self.capturing,
                hotkeys_available: self.hotkeys.as_ref().is_some_and(|h| h.available()),
                monitor_sounds: self.cfg.monitor_sounds,
                monitor_on: self.cfg.devices.monitor_enabled,
            };
            soundboard::panel(ui, &view)
        };
        for a in actions {
            self.apply_sound_action(ui.ctx(), a);
        }
    }

    fn apply_sound_action(&mut self, ctx: &egui::Context, action: soundboard::Action) {
        use soundboard::Action as A;
        match action {
            A::AddFiles => {
                let files = rfd::FileDialog::new()
                    .add_filter("Audio", &decode::EXTENSIONS)
                    .pick_files()
                    .unwrap_or_default();
                self.add_sounds(ctx, files);
            }
            A::Play(id) => {
                if !self.sounds.lock().play(id) {
                    self.toasts
                        .push(ctx, "Cannot play: no virtual mic output is running");
                }
            }
            A::Stop(id) => self.sounds.lock().stop(id),
            A::StopAll => self.sounds.lock().stop_all(),
            A::Remove(id) => {
                self.sounds.lock().remove(id);
                self.sounds_changed();
            }
            A::Volume(id, v) => {
                self.sounds.lock().set_volume(id, v);
                self.sounds_changed();
            }
            A::BeginHotkey(id) => {
                if let Some(h) = &self.hotkeys {
                    h.begin_capture();
                    self.capturing = Some(id);
                }
            }
            A::CancelHotkey => {
                if let Some(h) = &self.hotkeys {
                    h.cancel_capture();
                }
                self.capturing = None;
            }
            A::ClearHotkey(id) => {
                if let Some(s) = self.sounds.lock().get_mut(id) {
                    s.entry.hotkey = None;
                }
                self.sounds_changed();
            }
            A::MonitorSounds(on) => {
                self.cfg.monitor_sounds = on;
                apply_to_shared(&self.cfg, &self.shared);
            }
        }
    }

    pub fn add_sounds(&mut self, ctx: &egui::Context, files: Vec<PathBuf>) {
        let (ok, skipped): (Vec<_>, Vec<_>) =
            files.into_iter().partition(|p| decode::is_supported(p));
        if !ok.is_empty() {
            let mut board = self.sounds.lock();
            for p in &ok {
                board.add(SoundEntry::from_path(p.clone()));
            }
            drop(board);
            self.sounds_changed();
            self.cfg.tab = Tab::Soundboard;
        }
        if !skipped.is_empty() {
            self.toasts.push(
                ctx,
                format!(
                    "Skipped {} file(s): only wav, mp3, ogg and flac",
                    skipped.len()
                ),
            );
        }
    }

    fn sounds_changed(&mut self) {
        self.cfg.sounds = self.sounds.lock().entries();
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        if !dropped.is_empty() {
            self.add_sounds(ctx, dropped);
        }
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            let screen = ctx.content_rect();
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("drop"),
            ));
            painter.rect_filled(screen, 0.0, Color32::from_black_alpha(160));
            painter.text(
                screen.center(),
                egui::Align2::CENTER_CENTER,
                "Drop to add to the soundboard",
                egui::FontId::proportional(28.0),
                Color32::WHITE,
            );
        }
    }

    fn poll_hotkey_capture(&mut self, ctx: &egui::Context) {
        let (Some(h), Some(id)) = (&self.hotkeys, self.capturing) else {
            return;
        };
        let Some(result) = h.poll_capture() else {
            return;
        };
        self.capturing = None;
        let Captured::Key(key) = result else { return };
        let mut board = self.sounds.lock();
        let clash = board
            .hotkey_owner(&key)
            .filter(|&other| other != id)
            .and_then(|other| {
                board
                    .sounds
                    .iter()
                    .find(|s| s.id == other)
                    .map(|s| s.entry.name.clone())
            });
        let label = key.label.clone();
        if let Some(s) = board.get_mut(id) {
            s.entry.hotkey = Some(key);
        }
        drop(board);
        self.sounds_changed();
        let msg = match clash {
            Some(other) => format!("{label} assigned (also used by \"{other}\": both will play)"),
            None => format!("Hotkey {label} assigned"),
        };
        self.toasts.push(ctx, msg);
    }

    fn apply_preset_action(&mut self, ctx: &egui::Context, action: presets::Action) {
        match action {
            presets::Action::Apply(p) => {
                self.fx = config::presets::apply(&p, &self.fx);
                self.effects_changed();
                self.cfg.preset = Some(p.name);
            }
            presets::Action::Save(name) => {
                let preset = config::presets::Preset {
                    name: name.clone(),
                    effects: self.fx.to_settings(),
                };
                match self.cfg.user_presets.iter_mut().find(|p| p.name == name) {
                    Some(existing) => *existing = preset,
                    None => self.cfg.user_presets.push(preset),
                }
                self.cfg.preset = Some(name.clone());
                self.preset_name.clear();
                self.toasts.push(ctx, format!("Saved preset \"{name}\""));
            }
            presets::Action::Delete(name) => {
                self.cfg.user_presets.retain(|p| p.name != name);
                if self.cfg.preset.as_ref() == Some(&name) {
                    self.cfg.preset = None;
                }
                self.toasts.push(ctx, format!("Deleted preset \"{name}\""));
            }
        }
    }

    /// Saves shortly after the last change.
    fn autosave(&mut self, now: f64) {
        if self.cfg == self.saved {
            self.changed_at = None;
            return;
        }
        let since = *self.changed_at.get_or_insert(now);
        if now - since >= SAVE_DELAY_S {
            self.save_now();
        }
    }

    pub fn save_now(&mut self) {
        if let Some(path) = &self.config_path
            && let Err(e) = config::save_to(path, &self.cfg)
        {
            eprintln!("saving settings failed: {e}");
            return;
        }
        self.saved = self.cfg.clone();
        self.changed_at = None;
    }

    fn effects_changed(&mut self) {
        self.shared.params.store(&self.fx);
        self.cfg.effects = self.fx.to_settings();
    }

    fn top_bar(&mut self, ui: &mut Ui) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let on = self.cfg.voice_on;
            let (text, fill) = if on {
                ("Voice changer ON", ON)
            } else {
                ("Voice changer OFF", OFF)
            };
            let button = egui::Button::new(
                RichText::new(text)
                    .size(20.0)
                    .strong()
                    .color(Color32::WHITE),
            )
            .fill(fill)
            .min_size(egui::vec2(230.0, 44.0));
            if ui
                .add(button)
                .on_hover_text("Off = your clean voice passes straight through")
                .clicked()
            {
                self.cfg.voice_on = !on;
                self.shared.voice_on.store(self.cfg.voice_on, Relaxed);
            }
            ui.add_space(12.0);
            ui.vertical(|ui| {
                self.active_mic_label(ui);
                self.meter_row(ui);
            });
        });
        ui.add_space(6.0);
    }

    fn active_mic_label(&self, ui: &mut Ui) {
        let s = &self.status;
        let device = |slot: usize| s.mics[slot].device.clone().unwrap_or_default();
        let (text, color) = match s.active {
            ActiveMic::Primary if s.reason.is_empty() => {
                (format!("Active: Primary mic - {}", device(0)), ON)
            }
            ActiveMic::Primary => (
                format!("Active: Primary mic ({}) - {}", s.reason, device(0)),
                ON,
            ),
            ActiveMic::Fallback => (
                format!("Active: Fallback mic ({}) - {}", s.reason, device(1)),
                WARN,
            ),
            ActiveMic::None if !s.scanned => ("Starting audio...".to_string(), OFF),
            ActiveMic::None if s.output.device.is_none() => (
                "No virtual mic output selected - nothing is being sent".to_string(),
                BAD,
            ),
            ActiveMic::None => (
                "No microphone available - sending silence, still looking...".to_string(),
                BAD,
            ),
        };
        ui.label(RichText::new(text).strong().color(color));
    }

    fn meter_row(&mut self, ui: &mut Ui) {
        let now = ui.input(|i| i.time);
        let clips = self.shared.clip_count.load(Relaxed);
        if clips != self.clip_seen {
            self.clip_seen = clips;
            self.clip_until = now + CLIP_HOLD_S;
        }
        ui.horizontal(|ui| {
            ui.label("In");
            let floor = meters::PROGRAM_FLOOR_DB;
            meters::level_bar(ui, self.shared.input_level.get(), 180.0, floor, None);
            ui.label("Out");
            meters::level_bar(ui, self.shared.output_level.get(), 180.0, floor, None);
            meters::clip_lamp(ui, now < self.clip_until)
                .on_hover_text("Lights when the output exceeds full scale. Lower the output gain.");
            if let Some(ms) = self.status.latency_ms {
                ui.label(RichText::new(format!("~{ms:.0} ms")).small());
            }
        });
    }
}

impl eframe::App for VoxApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_now();
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::audio::device_manager::{Endpoint, LinkState};

    pub fn fake_status() -> DeviceStatus {
        DeviceStatus {
            scanned: true,
            inputs: vec![
                "Microphone (HyperX Cloud II Wireless)".into(),
                "Microphone (USB PnP Audio Device)".into(),
            ],
            outputs: vec!["Speakers (HyperX Cloud II Wireless)".into()],
            default_input: Some("Microphone (USB PnP Audio Device)".into()),
            cable: None,
            output: Endpoint {
                device: Some("Speakers (HyperX Cloud II Wireless)".into()),
                state: LinkState::Running,
                rate: 48_000,
                period: 480,
            },
            active: ActiveMic::Fallback,
            reason: "primary mic is silent".into(),
            latency_ms: Some(41.0),
            ..Default::default()
        }
    }

    pub fn test_app() -> VoxApp {
        let shared = Arc::new(Shared::default());
        VoxApp::with_manager(
            Config::default(),
            shared,
            ManagerHandle::detached(fake_status()),
        )
    }

    /// Runs one real egui frame headlessly (layout, painting and input handling).
    pub fn frame(
        ctx: &egui::Context,
        app: &mut VoxApp,
        size: (f32, f32),
        t: f64,
        events: Vec<egui::Event>,
    ) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(size.0, size.1),
            )),
            time: Some(t),
            events,
            ..Default::default()
        };
        ctx.run_ui(input, |ui| app.draw(ui)).textures_delta.clear();
    }

    #[test]
    fn draws_at_many_sizes_and_applies_first_run_defaults() {
        let mut app = test_app();
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let mut t = 0.0;
        for tab in [Tab::Effects, Tab::Soundboard] {
            app.cfg.tab = tab;
            for size in [
                (920.0, 600.0),
                (1240.0, 800.0),
                (1920.0, 1080.0),
                (3840.0, 2160.0),
            ] {
                frame(&ctx, &mut app, size, t, vec![]);
                t += 0.1;
            }
        }
        assert_eq!(app.cfg.devices.primary, "Microphone (USB PnP Audio Device)");
        assert!(
            app.cfg.devices.output.is_empty(),
            "no CABLE: output stays unselected"
        );
    }

    #[test]
    fn presets_apply_save_delete_and_everything_persists() {
        use crate::dsp::params::P;
        let dir = crate::soundboard::decode::tests::temp_dir("app-config");
        let path = dir.join("config.json");
        let mut app = test_app();
        app.config_path = Some(path.clone());
        let ctx = egui::Context::default();

        let robot = config::presets::builtin()
            .into_iter()
            .find(|p| p.name == "Robot")
            .unwrap();
        app.apply_preset_action(&ctx, presets::Action::Apply(robot));
        assert!(app.fx.on(P::RobotOn));
        assert!(
            app.shared.params.get(P::RobotOn) >= 0.5,
            "reaches the audio thread"
        );
        app.fx.set(P::RobotFreq, 120.0);
        app.effects_changed();
        app.apply_preset_action(&ctx, presets::Action::Save("My robot".into()));
        assert_eq!(app.cfg.preset.as_deref(), Some("My robot"));

        app.autosave(10.0);
        assert!(!path.exists(), "waits for the debounce");
        app.autosave(11.5);
        let (loaded, warning) = config::load_from(&path);
        assert!(warning.is_none());
        assert_eq!(loaded, app.cfg);
        assert_eq!(loaded.user_presets[0].effects["robot_freq"], 120.0);

        app.apply_preset_action(&ctx, presets::Action::Delete("My robot".into()));
        assert!(app.cfg.user_presets.is_empty() && app.cfg.preset.is_none());
        frame(&ctx, &mut app, (1240.0, 800.0), 0.0, vec![]);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn soundboard_tab_draws_sounds_in_every_state_and_filters_drops() {
        let mut app = test_app();
        let ctx = egui::Context::default();
        app.add_sounds(
            &ctx,
            vec![
                "C:/nope/airhorn.mp3".into(),
                "C:/nope/notes.txt".into(),
                "C:/nope/bruh.wav".into(),
            ],
        );
        assert_eq!(app.cfg.sounds.len(), 2, "the .txt is skipped");
        assert_eq!(app.cfg.tab, Tab::Soundboard);
        app.capturing = Some(app.sounds.lock().sounds[0].id);
        for (i, size) in [(920.0, 600.0), (1920.0, 1080.0)].into_iter().enumerate() {
            frame(&ctx, &mut app, size, i as f64, vec![]);
        }
    }
}
