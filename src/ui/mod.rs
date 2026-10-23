//! The window: master toggle and meters on top, devices on the left, effects and soundboard in
//! the middle.

pub mod devices;
pub mod effects;
pub mod meters;
pub mod presets;
pub mod settings;
pub mod soundboard;
pub mod theme;
pub mod toast;
pub mod tray_status;

use crate::audio::device_manager::{DeviceStatus, Event, ManagerCmd, ManagerHandle};
use crate::audio::shared::{ActiveMic, Shared};
use crate::config::{self, CloseBehavior, Config};
use crate::dsp::params::Snapshot;
use crate::shell::tray::Tray;
use crate::shell::{TrayAction, autostart};
use crate::soundboard::decode;
use crate::soundboard::hotkeys::{Captured, HotkeyService};
use crate::soundboard::library::{SoundEntry, Soundboard};
use crate::soundboard::mixer::SoundId;
use egui::{Color32, RichText, Ui, ViewportCommand};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
use theme::{ACCENT, BAD, OFF, ON, WARN};
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
    /// Wall clock for autosave: egui's input time stands still while the window is hidden.
    clock: Instant,

    tray: Option<Tray>,
    /// Show / Quit requests from the tray thread (or a second launch of the app).
    shell_requests: Option<mpsc::Receiver<TrayAction>>,
    /// Latest config, for the tray thread to save on Windows shutdown/logoff.
    mirror: Arc<Mutex<Config>>,
    session_ending: Arc<AtomicBool>,
    close_dialog: Option<CloseDialog>,
    /// Set when we really want to exit: the next close request is let through.
    quitting: bool,
    hidden: bool,
    /// The "still running in the tray" balloon was shown this run.
    tray_hint_shown: bool,
    start_hidden_pending: bool,
    recenter_on_show: bool,
    frames_drawn: u64,
    autostart: bool,
    autostart_elsewhere: bool,
}

struct CloseDialog {
    remember: bool,
    focus_pending: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CloseChoice {
    Minimize,
    Quit,
    Cancel,
}

pub const WINDOW_SIZE: [f32; 2] = [1240.0, 800.0];
/// Where the window starts when "Start minimized to tray" is on (moved back when first shown).
pub const OFFSCREEN: [f32; 2] = [-32000.0, -32000.0];

/// Save this long after the last change, so dragging a slider does not write 30 times a second.
const SAVE_DELAY_S: f64 = 1.0;
/// How often the app logic ticks while the window is hidden.
const HIDDEN_TICK: Duration = Duration::from_millis(250);

/// Pushes the non-device parts of the config into the shared atomics.
fn apply_to_shared(cfg: &Config, shared: &Shared) {
    shared.voice_on.store(cfg.voice_on, Relaxed);
    shared.monitor_volume.set(cfg.monitor_volume);
    shared.monitor_voice.store(cfg.monitor_voice, Relaxed);
    shared.monitor_sounds.store(cfg.monitor_sounds, Relaxed);
    shared.params.store(&Snapshot::from_settings(&cfg.effects));
}

impl VoxApp {
    /// Starts audio, hotkeys and the tray icon with settings loaded from `config_path`.
    pub fn new(
        config_path: PathBuf,
        cfg: Config,
        notice: Option<String>,
        ctx: egui::Context,
    ) -> Self {
        let shared = Arc::new(Shared::default());
        let manager = ManagerHandle::spawn(shared.clone(), cfg.devices.clone(), cfg.mic.clone());
        let mut app = Self::with_manager(cfg, shared, manager);
        app.hotkeys = Some(HotkeyService::start(app.sounds.board.clone()));
        app.config_path = Some(config_path);
        app.startup_notice = notice;
        app.start_tray(ctx);
        app.start_hidden_pending = app.cfg.start_minimized && app.tray.is_some();
        app.autostart = autostart::is_enabled();
        app.autostart_elsewhere = autostart::points_elsewhere();
        app
    }

    /// The tray runs on its own thread. Toggles and "stop all" act directly on the shared
    /// state; Show and Quit come back to us through a channel plus a repaint request, which
    /// wakes `logic` even while the window is hidden.
    fn start_tray(&mut self, ctx: egui::Context) {
        let (tx, rx) = mpsc::channel();
        let status = self.manager.status_handle();
        let (shared_s, shared_a) = (self.shared.clone(), self.shared.clone());
        let board = self.sounds.board.clone();
        let mirror = self.mirror.clone();
        let path = self.config_path.clone();
        let ending = self.session_ending.clone();
        let tray = Tray::start(
            move || {
                let dev = status.lock().unwrap_or_else(|e| e.into_inner()).clone();
                tray_status::compute(
                    &dev,
                    shared_s.voice_on.load(Relaxed),
                    shared_s.mic_muted.load(Relaxed),
                )
            },
            move |action| match action {
                TrayAction::Show | TrayAction::Quit => {
                    let _ = tx.send(action);
                    ctx.request_repaint();
                }
                TrayAction::ToggleVoice => {
                    shared_a.voice_on.fetch_xor(true, Relaxed);
                    ctx.request_repaint();
                }
                TrayAction::ToggleMute => {
                    shared_a.mic_muted.fetch_xor(true, Relaxed);
                    ctx.request_repaint();
                }
                TrayAction::StopAllSounds => {
                    board.lock().unwrap_or_else(|e| e.into_inner()).stop_all();
                }
                TrayAction::SessionEnd => {
                    // The process may be ended right after this returns: save synchronously.
                    ending.store(true, Relaxed);
                    let mut cfg = mirror.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    cfg.voice_on = shared_a.voice_on.load(Relaxed);
                    if let Some(p) = &path {
                        let _ = config::save_to(p, &cfg);
                    }
                    ctx.request_repaint();
                }
            },
        );
        match tray {
            Ok(t) => {
                self.tray = Some(t);
                self.shell_requests = Some(rx);
            }
            Err(e) => {
                self.startup_notice = Some(format!(
                    "Tray icon unavailable ({e}); closing will minimize instead"
                ));
            }
        }
    }

    fn with_manager(cfg: Config, shared: Arc<Shared>, manager: ManagerHandle) -> Self {
        apply_to_shared(&cfg, &shared);
        let sounds = Soundboard::new(manager.port.clone(), cfg.sounds.clone());
        Self {
            clock: Instant::now(),
            tray: None,
            shell_requests: None,
            mirror: Arc::new(Mutex::new(cfg.clone())),
            session_ending: Arc::new(AtomicBool::new(false)),
            close_dialog: None,
            quitting: false,
            hidden: false,
            tray_hint_shown: false,
            start_hidden_pending: false,
            recenter_on_show: false,
            frames_drawn: 0,
            autostart: false,
            autostart_elsewhere: false,
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

    /// Everything that must keep running while the window is hidden. Called from
    /// `App::logic`, which eframe keeps calling for a hidden window as long as repaints are
    /// requested (we request one every 250 ms while hidden, and the tray wakes us on clicks).
    pub fn tick(&mut self, ctx: &egui::Context) {
        self.status = self.manager.status();
        self.first_run_defaults();
        self.sounds.lock().set_output_rate(self.status.output.rate);
        // The tray may have toggled the voice changer.
        let voice_on = self.shared.voice_on.load(Relaxed);
        if voice_on != self.cfg.voice_on {
            self.cfg.voice_on = voice_on;
        }
        self.handle_manager_events(ctx);
        self.handle_shell_requests(ctx);
        self.handle_close_request(ctx);
        // eframe shows the window after painting the first frame, overriding any earlier
        // hide, so "start minimized" hides it on the frame after (it starts off-screen).
        if self.start_hidden_pending {
            if self.frames_drawn >= 1 {
                self.start_hidden_pending = false;
                self.recenter_on_show = true;
                self.hide_to_tray(ctx, false);
            } else {
                ctx.request_repaint();
            }
        }
        if let Some(n) = self.startup_notice.take() {
            self.toasts.push(ctx, n);
        }
        self.autosave(self.clock.elapsed().as_secs_f64());
        self.sync_mirror();
        if self.hidden {
            ctx.request_repaint_after(HIDDEN_TICK);
        }
    }

    fn handle_manager_events(&mut self, ctx: &egui::Context) {
        while let Ok(ev) = self.manager.events.try_recv() {
            match ev {
                Event::Switched { to, reason } => {
                    let text = match to {
                        ActiveMic::Primary => "Switched to the primary mic".to_string(),
                        ActiveMic::Fallback => format!("Switched to the fallback mic ({reason})"),
                        ActiveMic::None => "No microphone available - sending silence".to_string(),
                    };
                    // Hidden in the tray, a toast would go unseen: use a tray balloon.
                    match (&self.tray, self.hidden) {
                        (Some(t), true) => t.notify("VoxShift", &text),
                        _ => self.toasts.push(ctx, text),
                    }
                }
            }
        }
    }

    fn handle_shell_requests(&mut self, ctx: &egui::Context) {
        let requests: Vec<TrayAction> = match &self.shell_requests {
            Some(rx) => rx.try_iter().collect(),
            None => return,
        };
        for r in requests {
            match r {
                TrayAction::Show => self.show_window(ctx),
                TrayAction::Quit => self.quit(ctx),
                _ => {}
            }
        }
    }

    /// The X button or Alt+F4. Closing must never silently kill the virtual mic.
    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        if self.session_ending.load(Relaxed) {
            self.quitting = true; // Windows is shutting down: no dialog
        }
        if self.quitting {
            return; // let it close
        }
        match self.cfg.close_behavior {
            CloseBehavior::Quit => self.quitting = true,
            CloseBehavior::MinimizeToTray => {
                ctx.send_viewport_cmd(ViewportCommand::CancelClose);
                self.hide_to_tray(ctx, true);
            }
            CloseBehavior::Ask => {
                ctx.send_viewport_cmd(ViewportCommand::CancelClose);
                self.show_window(ctx);
                self.close_dialog = Some(CloseDialog {
                    remember: false,
                    focus_pending: true,
                });
            }
        }
    }

    fn hide_to_tray(&mut self, ctx: &egui::Context, show_hint: bool) {
        let Some(tray) = &self.tray else {
            // Without a tray icon a hidden window could not be brought back: minimize.
            ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
            return;
        };
        ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        self.hidden = true;
        if show_hint && !self.tray_hint_shown {
            self.tray_hint_shown = true;
            tray.notify(
                "VoxShift is still running in the tray",
                "Your virtual mic keeps working. Right-click the tray icon to quit.",
            );
        }
        ctx.request_repaint_after(HIDDEN_TICK);
    }

    fn show_window(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.recenter_on_show) {
            // Started off-screen ("start minimized"): bring it to the middle of the monitor.
            let monitor = ctx
                .input(|i| i.viewport().monitor_size)
                .unwrap_or(egui::vec2(1920.0, 1080.0));
            let size = egui::Vec2::from(WINDOW_SIZE);
            let pos = ((monitor - size) / 2.0).max(egui::Vec2::ZERO);
            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(pos.to_pos2()));
        }
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
        self.hidden = false;
        ctx.request_repaint();
    }

    /// Really exit: the next close request is let through, and `on_exit` cleans up.
    fn quit(&mut self, ctx: &egui::Context) {
        self.quitting = true;
        self.close_dialog = None;
        ctx.send_viewport_cmd(ViewportCommand::Close);
        ctx.request_repaint();
    }

    fn refresh_tray(&self) {
        if let Some(t) = &self.tray {
            t.refresh();
        }
    }

    fn sync_mirror(&self) {
        let mut m = self.mirror.lock().unwrap_or_else(|e| e.into_inner());
        if *m != self.cfg {
            *m = self.cfg.clone();
        }
    }

    /// Stops the tray, the hotkey hook and all audio. Safe to call twice.
    fn shutdown_background(&mut self) {
        if let Some(mut t) = self.tray.take() {
            t.shutdown(); // removes the icon, so no ghost stays in the tray
        }
        self.hotkeys = None;
        self.manager.shutdown();
    }

    fn close_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.close_dialog else {
            return;
        };
        let tray_ok = self.tray.is_some();
        let mut choice = None;
        let response = egui::Modal::new(egui::Id::new("close_dialog")).show(ctx, |ui| {
            ui.set_width(400.0);
            ui.heading("Close VoxShift?");
            ui.add_space(4.0);
            ui.label(
                "Your virtual mic only works while VoxShift is running. If you quit, Discord \
                 and games hear nothing until you start it again.",
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let minimize = egui::Button::new(
                    RichText::new(if tray_ok {
                        "Minimize to tray"
                    } else {
                        "Minimize"
                    })
                    .strong()
                    .color(Color32::WHITE),
                )
                .fill(ACCENT);
                let r = ui.add(minimize);
                if std::mem::take(&mut dialog.focus_pending) {
                    r.request_focus(); // Enter picks the default
                }
                if r.clicked() {
                    choice = Some(CloseChoice::Minimize);
                }
                if ui.button("Quit completely").clicked() {
                    choice = Some(CloseChoice::Quit);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(CloseChoice::Cancel);
                }
            });
            ui.add_space(4.0);
            ui.checkbox(&mut dialog.remember, "Remember my choice")
                .on_hover_text("Change it later under Window & startup");
        });
        if choice.is_none() && response.should_close() {
            choice = Some(CloseChoice::Cancel); // Esc or a click outside
        }
        if let Some(c) = choice {
            self.apply_close_choice(ctx, c);
        }
    }

    fn apply_close_choice(&mut self, ctx: &egui::Context, choice: CloseChoice) {
        let remember = self.close_dialog.take().is_some_and(|d| d.remember);
        match choice {
            CloseChoice::Minimize => {
                if remember {
                    self.cfg.close_behavior = CloseBehavior::MinimizeToTray;
                }
                self.hide_to_tray(ctx, true);
            }
            CloseChoice::Quit => {
                if remember {
                    self.cfg.close_behavior = CloseBehavior::Quit;
                }
                self.quit(ctx);
            }
            CloseChoice::Cancel => {}
        }
    }

    pub fn draw(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.frames_drawn += 1;
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
                    ui.add_space(6.0);
                    let view = settings::View {
                        autostart: self.autostart,
                        autostart_elsewhere: self.autostart_elsewhere,
                        tray_available: self.tray.is_some(),
                    };
                    if let Some(on) = settings::panel(ui, &mut self.cfg, &view) {
                        self.set_autostart(&ctx, on);
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
        self.close_dialog(&ctx);
        self.toasts.show(&ctx);
        ctx.request_repaint_after(Duration::from_millis(33));
    }

    fn set_autostart(&mut self, ctx: &egui::Context, on: bool) {
        match autostart::set_enabled(on) {
            Ok(()) => {
                self.autostart = on;
                self.autostart_elsewhere = false;
                let msg = if on {
                    "VoxShift will start with Windows"
                } else {
                    "VoxShift will no longer start with Windows"
                };
                self.toasts.push(ctx, msg);
            }
            Err(e) => self
                .toasts
                .push(ctx, format!("Could not change startup: {e}")),
        }
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
                self.refresh_tray();
            }
            let muted = self.shared.mic_muted.load(Relaxed);
            let (text, fill) = if muted {
                ("Mic muted", BAD)
            } else {
                ("Mute mic", Color32::from_gray(52))
            };
            let mute = egui::Button::new(RichText::new(text).strong().color(Color32::WHITE))
                .fill(fill)
                .min_size(egui::vec2(110.0, 44.0));
            if ui
                .add(mute)
                .on_hover_text("Silences your mic; the soundboard still plays")
                .clicked()
            {
                self.shared.mic_muted.store(!muted, Relaxed);
                self.refresh_tray();
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
            ActiveMic::Primary | ActiveMic::Fallback if self.shared.mic_muted.load(Relaxed) => {
                ("Mic muted - only the soundboard is sent".to_string(), BAD)
            }
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
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.tick(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_now();
        self.shutdown_background();
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
        ctx.run_ui(input, |ui| {
            app.tick(ui.ctx()); // what eframe's `App::logic` does
            app.draw(ui)
        })
        .textures_delta
        .clear();
    }

    /// One frame where the window's X was clicked; returns the root viewport commands.
    fn close_frame(ctx: &egui::Context, app: &mut VoxApp) -> Vec<ViewportCommand> {
        let mut info = egui::ViewportInfo::default();
        info.events.push(egui::ViewportEvent::Close);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1240.0, 800.0),
            )),
            viewports: std::iter::once((egui::ViewportId::ROOT, info)).collect(),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            app.tick(ui.ctx());
            app.draw(ui)
        });
        out.textures_delta.clear();
        out.viewport_output
            .remove(&egui::ViewportId::ROOT)
            .map(|v| v.commands)
            .unwrap_or_default()
    }

    #[test]
    fn closing_asks_and_remembering_quit_stops_asking() {
        let mut app = test_app();
        let ctx = egui::Context::default();
        let cmds = close_frame(&ctx, &mut app);
        assert!(
            cmds.contains(&ViewportCommand::CancelClose),
            "the X never closes silently"
        );
        assert!(app.close_dialog.is_some(), "dialog shown");
        frame(&ctx, &mut app, (1240.0, 800.0), 0.1, vec![]); // dialog draws

        app.close_dialog.as_mut().unwrap().remember = true;
        app.apply_close_choice(&ctx, CloseChoice::Quit);
        assert_eq!(app.cfg.close_behavior, CloseBehavior::Quit);
        assert!(app.quitting && app.close_dialog.is_none());
        let cmds = close_frame(&ctx, &mut app);
        assert!(
            !cmds.contains(&ViewportCommand::CancelClose),
            "now it really closes"
        );
    }

    #[test]
    fn cancel_keeps_running_and_does_not_remember() {
        let mut app = test_app();
        let ctx = egui::Context::default();
        close_frame(&ctx, &mut app);
        app.close_dialog.as_mut().unwrap().remember = true;
        app.apply_close_choice(&ctx, CloseChoice::Cancel);
        assert_eq!(app.cfg.close_behavior, CloseBehavior::Ask);
        assert!(!app.quitting && !app.hidden && app.close_dialog.is_none());
    }

    #[test]
    fn remembered_choices_skip_the_dialog() {
        let ctx = egui::Context::default();
        // "Always quit": the close goes through.
        let mut app = test_app();
        app.cfg.close_behavior = CloseBehavior::Quit;
        assert!(!close_frame(&ctx, &mut app).contains(&ViewportCommand::CancelClose));
        assert!(app.close_dialog.is_none());

        // "Always minimize", but no tray icon (as in tests): minimize instead of hiding,
        // so the window can always be found again.
        let mut app = test_app();
        app.cfg.close_behavior = CloseBehavior::MinimizeToTray;
        let cmds = close_frame(&ctx, &mut app);
        assert!(cmds.contains(&ViewportCommand::CancelClose));
        assert!(cmds.contains(&ViewportCommand::Minimized(true)));
        assert!(!cmds.contains(&ViewportCommand::Visible(false)));
        assert!(app.close_dialog.is_none() && !app.hidden);
    }

    #[test]
    fn windows_shutdown_closes_without_a_dialog() {
        let mut app = test_app();
        let ctx = egui::Context::default();
        app.session_ending.store(true, Relaxed);
        let cmds = close_frame(&ctx, &mut app);
        assert!(!cmds.contains(&ViewportCommand::CancelClose));
        assert!(app.close_dialog.is_none());
    }

    #[test]
    fn tray_show_and_quit_requests_reach_the_window_even_without_drawing() {
        let mut app = test_app();
        let ctx = egui::Context::default();
        let (tx, rx) = mpsc::channel();
        app.shell_requests = Some(rx);
        app.hidden = true;
        tx.send(TrayAction::Show).unwrap();
        // While hidden, eframe runs only the logic (`run_logic`), never a ui pass.
        let out = ctx.run_logic(&egui::RawInput::default(), |ctx| app.tick(ctx));
        let cmds = &out.viewport_commands[&egui::ViewportId::ROOT];
        assert!(cmds.contains(&ViewportCommand::Visible(true)));
        assert!(cmds.contains(&ViewportCommand::Focus));
        assert!(!app.hidden);

        tx.send(TrayAction::Quit).unwrap();
        let out = ctx.run_logic(&egui::RawInput::default(), |ctx| app.tick(ctx));
        assert!(out.viewport_commands[&egui::ViewportId::ROOT].contains(&ViewportCommand::Close));
        assert!(app.quitting);
    }

    #[test]
    fn tray_toggles_reach_the_config_and_the_mirror_follows() {
        let mut app = test_app();
        let ctx = egui::Context::default();
        app.shared.voice_on.store(false, Relaxed); // what the tray's "Voice changer" item does
        frame(&ctx, &mut app, (1240.0, 800.0), 0.0, vec![]);
        assert!(!app.cfg.voice_on);
        assert!(
            !app.mirror.lock().unwrap().voice_on,
            "saved on Windows shutdown"
        );
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
