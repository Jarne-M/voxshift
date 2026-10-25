//! The window: master toggle and meters on top, devices on the left, effects and soundboard in
//! the middle.

pub mod devices;
pub mod effects;
pub mod hotkey_settings;
pub mod melody;
pub mod meters;
pub mod presets;
pub mod settings;
pub mod soundboard;
pub mod theme;
pub mod toast;
pub mod tray_status;

use crate::audio::device_manager::{DeviceStatus, Event, ManagerCmd, ManagerHandle};
use crate::audio::shared::{ActiveMic, Shared};
use crate::config::{self, CloseBehavior, Config, StartVoice};
use crate::dsp::params::{P, Snapshot};
use crate::keymap;
use crate::melody::{Loaded, MelodySource};
use crate::shell::tray::{Tray, TrayPoster};
use crate::shell::{TrayAction, autostart};
use crate::soundboard::decode;
use crate::soundboard::hotkeys::{self, Captured, HotkeyService, KeyHandler};
use crate::soundboard::library::{SoundEntry, Soundboard};
use crate::soundboard::mixer::SoundId;
use crate::soundboard::organize;
use crate::voice_keys::{self, Feedback, KeyTarget, VoiceKeys, VoiceRequest};
use egui::{Color32, RichText, Ui, ViewportCommand};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex, OnceLock, mpsc};
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
    /// Melody Voice's melody (also published to the audio thread).
    melody: Option<Loaded>,
    shared: Arc<Shared>,
    manager: ManagerHandle,
    status: DeviceStatus,
    toasts: Toasts,
    clip_seen: u32,
    clip_until: f64,
    defaults_applied: bool,
    sounds: Soundboard,
    hotkeys: Option<HotkeyService>,
    /// The hotkey waiting for a key combination (a sound, the voice toggle, ...).
    capturing: Option<KeyTarget>,
    /// A captured key that is already taken: the Keep / Change dialog.
    key_conflict: Option<KeyConflict>,
    /// The soundboard search box (not saved).
    search: String,
    /// Board revision last copied into the config (plays happen on the hotkey thread too).
    board_revision: u64,
    capture_watch: Option<CaptureWatch>,
    /// Switches the voice changer and announces it (button, tray and hotkey all use this).
    feedback: Arc<Feedback>,
    /// Set once the tray exists (the overlay is drawn by the tray thread).
    tray_poster: Arc<OnceLock<TrayPoster>>,
    voice_keys: Option<Arc<VoiceKeys>>,
    voice_requests: Option<mpsc::Receiver<VoiceRequest>>,
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

/// Watches a hotkey capture for key presses that never reach the global listener.
struct CaptureWatch {
    presses_at_start: u64,
    /// When this window first saw a key press during the capture.
    window_key_at: Option<f64>,
    /// Last "still waiting" line in the troubleshooting log.
    last_log: f64,
}

/// How long a key press in the window may go unreported by the listener.
const CAPTURE_GRACE_S: f64 = 0.75;

/// A captured key that another action already has, waiting for Keep or Change.
struct KeyConflict {
    key: hotkeys::Hotkey,
    /// The action being assigned, and how it is called.
    target: KeyTarget,
    target_name: String,
    owner: keymap::Owner,
    focus_pending: bool,
}

/// Makes every hotkey in a loaded config unique (see [`keymap::dedupe`]); returns what was
/// cleared.
fn dedupe_hotkeys(cfg: &mut Config) -> Vec<String> {
    let mut sounds: Vec<keymap::SoundKey> = cfg
        .sounds
        .iter_mut()
        .enumerate()
        .map(|(i, s)| keymap::SoundKey {
            id: i as u64 + 1,
            name: &s.name,
            hotkey: &mut s.hotkey,
        })
        .collect();
    keymap::dedupe(&mut cfg.voice_keys, &mut sounds)
}

/// "the sound \"x\"" -> "The sound \"x\"".
fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
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
    apply_feedback(cfg, shared);
}

fn apply_feedback(cfg: &Config, shared: &Shared) {
    shared.cue_enabled.store(cfg.feedback.cues, Relaxed);
    shared.cue_volume.set(cfg.feedback.cue_volume);
    shared.overlay_enabled.store(cfg.feedback.overlay, Relaxed);
}

/// Effects on or off at start, per "Start with effects".
fn start_voice_on(cfg: &Config) -> bool {
    match cfg.start_voice {
        StartVoice::RememberLast => cfg.voice_on,
        StartVoice::On => true,
        StartVoice::Off => false,
    }
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
        hotkeys::diag::set_path(config_path.with_file_name("hotkeys-debug.log"));
        app.config_path = Some(config_path);
        if let Some(n) = notice {
            app.startup_notice = Some(match app.startup_notice.take() {
                Some(m) => format!("{n}\n{m}"),
                None => n,
            });
        }
        app.start_tray(ctx.clone());
        app.start_hotkeys(ctx);
        app.start_hidden_pending = app.cfg.start_minimized && app.tray.is_some();
        app.autostart = autostart::is_enabled();
        app.autostart_elsewhere = autostart::points_elsewhere();
        app
    }

    /// Global hotkeys: soundboard clips plus the voice changer keys (toggle/hold, mute,
    /// presets). Toggle and mute act on the shared state right on the hotkey thread; preset
    /// keys come back to us through a channel, like the tray's requests.
    fn start_hotkeys(&mut self, ctx: egui::Context) {
        let (tx, rx) = mpsc::channel();
        let poster = self.tray_poster.clone();
        let keys = Arc::new(VoiceKeys::new(self.feedback.clone(), tx, move || {
            ctx.request_repaint();
            if let Some(p) = poster.get() {
                p.refresh();
            }
        }));
        keys.set_bindings(self.cfg.voice_keys.clone());
        let handler: Arc<dyn KeyHandler> = keys.clone();
        self.hotkeys = Some(HotkeyService::start(
            self.sounds.board.clone(),
            Some(handler),
        ));
        self.voice_keys = Some(keys);
        self.voice_requests = Some(rx);
    }

    /// The tray runs on its own thread. Toggles and "stop all" act directly on the shared
    /// state; Show and Quit come back to us through a channel plus a repaint request, which
    /// wakes `logic` even while the window is hidden.
    fn start_tray(&mut self, ctx: egui::Context) {
        let (tx, rx) = mpsc::channel();
        let status = self.manager.status_handle();
        let (shared_s, shared_a) = (self.shared.clone(), self.shared.clone());
        let feedback = self.feedback.clone();
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
                    shared_s.monitor_voice.load(Relaxed),
                )
            },
            move |action| match action {
                TrayAction::Show | TrayAction::Quit => {
                    let _ = tx.send(action);
                    ctx.request_repaint();
                }
                TrayAction::ToggleVoice => {
                    feedback.toggle_voice();
                    ctx.request_repaint();
                }
                TrayAction::ToggleMute => {
                    feedback.toggle_mute();
                    ctx.request_repaint();
                }
                TrayAction::ToggleMonitor => {
                    feedback.toggle_monitor();
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
                let _ = self.tray_poster.set(t.poster());
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

    fn with_manager(mut cfg: Config, shared: Arc<Shared>, manager: ManagerHandle) -> Self {
        cfg.voice_on = start_voice_on(&cfg);
        // `saved` is the config as loaded, so a cleanup below differs from it and is saved:
        // the notice about it shows once.
        let loaded = cfg.clone();
        let cleared = dedupe_hotkeys(&mut cfg);
        cfg.soundboard.repair(&mut cfg.sounds);
        apply_to_shared(&cfg, &shared);
        let sounds = Soundboard::new(manager.port.clone(), cfg.sounds.clone());
        {
            // Clips from older versions get their permanent ids here (saved right after).
            let mut board = sounds.lock();
            board.recent = cfg.soundboard.recent.clone();
            cfg.sounds = board.entries();
        }
        let tray_poster: Arc<OnceLock<TrayPoster>> = Arc::new(OnceLock::new());
        let poster = tray_poster.clone();
        let feedback = Arc::new(Feedback::new(shared.clone(), move |on| {
            if let Some(p) = poster.get() {
                p.overlay(on);
            }
        }));
        let mut app = Self {
            melody: None,
            feedback,
            tray_poster,
            voice_keys: None,
            voice_requests: None,
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
            key_conflict: None,
            search: String::new(),
            board_revision: 0,
            capture_watch: None,
            preset_name: String::new(),
            config_path: None,
            saved: loaded,
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
        };
        let source = app.cfg.melody.source.clone();
        match app.select_melody(source, false) {
            Ok(warning) => app.startup_notice = warning,
            Err(e) => {
                app.startup_notice = Some(format!("Melody Voice: {e}. Using the default melody."));
                let _ = app.select_melody(MelodySource::default(), false);
            }
        }
        if !cleared.is_empty() {
            let note = format!(
                "Some hotkeys were set twice. The first one kept its key; cleared: {}",
                cleared.join(", ")
            );
            app.startup_notice = Some(match app.startup_notice.take() {
                Some(other) => format!("{note}\n{other}"),
                None => note,
            });
        }
        app
    }

    /// Loads a melody, hands it to the audio thread and remembers the choice. On failure the
    /// current melody stays. `adopt_tempo` also sets the tempo slider to the melody's own.
    /// `Ok(Some(..))` is a warning worth showing (a melody cut short).
    pub fn select_melody(
        &mut self,
        source: MelodySource,
        adopt_tempo: bool,
    ) -> Result<Option<String>, String> {
        let loaded = crate::melody::load(&source)?;
        let kept = self.shared.melody.publish(&loaded.melody.notes);
        if adopt_tempo {
            self.fx.set(P::MelodyBpm, loaded.melody.bpm);
            self.effects_changed();
        }
        self.cfg.melody.source = match source {
            MelodySource::File { path, .. } => {
                self.cfg.melody.remember_file(&path);
                MelodySource::File {
                    path,
                    part: loaded.part,
                }
            }
            builtin => builtin,
        };
        let total = loaded.melody.notes.len();
        self.melody = Some(loaded);
        Ok((kept < total)
            .then(|| format!("The melody has {total} notes; only the first {kept} are used.")))
    }

    /// Uses a MIDI file as the melody (from the button or a drop) and says which part.
    pub fn import_midi(&mut self, ctx: &egui::Context, path: PathBuf) {
        let source = MelodySource::File { path, part: None };
        match self.select_melody(source, true) {
            Ok(warning) => {
                if let Some(w) = warning {
                    self.toasts.push(ctx, w);
                }
                let msg = match &self.melody {
                    Some(Loaded {
                        song: Some(song),
                        part: Some(part),
                        melody,
                    }) => format!(
                        "Melody: {} \u{2014} {} ({} of {} parts; pick another under Part)",
                        melody.name,
                        song.parts[*part].describe(),
                        part + 1,
                        song.parts.len()
                    ),
                    _ => "Melody imported".to_string(),
                };
                self.toasts.push(ctx, msg);
                self.cfg.tab = Tab::Effects;
            }
            Err(e) => self
                .toasts
                .push(ctx, format!("Could not use that MIDI file: {e}")),
        }
    }

    fn melody_panel(&mut self, ui: &mut Ui) {
        let note = self.shared.melody_note.load(Relaxed);
        let view = melody::View {
            detected: self.shared.melody_detected.get(),
            target: self.shared.melody_target.get(),
            latency_ms: crate::dsp::melody::latency_ms(&self.fx, self.status.output.rate),
            note: usize::try_from(note).ok(),
            finished: self.shared.melody_finished.load(Relaxed),
            melody: self.melody.as_ref(),
            source: &self.cfg.melody.source,
            files: &self.cfg.melody.files,
            beat: self.shared.melody_beat.get(),
            preview: self
                .shared
                .melody_preview
                .load(Relaxed)
                .then(|| self.shared.melody_preview_beat.get()),
            monitor_on: self.cfg.devices.monitor_enabled,
        };
        let response = melody::panel(ui, &mut self.fx, &view);
        if response.changed {
            self.effects_changed();
        }
        for action in response.actions {
            match action {
                melody::Action::Select(source) => match self.select_melody(source, true) {
                    Ok(None) => {}
                    Ok(Some(warning)) => self.toasts.push(ui.ctx(), warning),
                    Err(e) => self.toasts.push(ui.ctx(), format!("Melody: {e}")),
                },
                melody::Action::Import => {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("MIDI", &["mid", "midi"])
                        .pick_file()
                    {
                        self.import_midi(ui.ctx(), path);
                    }
                }
                melody::Action::Restart => {
                    self.shared.melody_restart.fetch_add(1, Relaxed);
                }
                melody::Action::Next => {
                    self.shared.melody_next.fetch_add(1, Relaxed);
                }
                melody::Action::Preview(play) => {
                    self.shared.melody_preview.store(play, Relaxed);
                }
            }
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
        // The tray or a hotkey may have toggled the voice changer or "hear myself".
        let voice_on = self.shared.voice_on.load(Relaxed);
        if voice_on != self.cfg.voice_on {
            self.cfg.voice_on = voice_on;
        }
        let monitor_voice = self.shared.monitor_voice.load(Relaxed);
        if monitor_voice != self.cfg.monitor_voice {
            self.cfg.monitor_voice = monitor_voice;
        }
        let monitor_sounds = self.shared.monitor_sounds.load(Relaxed);
        if monitor_sounds != self.cfg.monitor_sounds {
            self.cfg.monitor_sounds = monitor_sounds;
        }
        // A hotkey may have played clips (play counts, Recent).
        if self.sounds.lock().revision != self.board_revision {
            self.sounds_changed();
        }
        self.handle_manager_events(ctx);
        self.handle_shell_requests(ctx);
        self.sync_voice_keys(ctx);
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

    /// Pushes changed voice hotkey settings to the hotkey thread and handles preset keys.
    fn sync_voice_keys(&mut self, ctx: &egui::Context) {
        if let Some(keys) = &self.voice_keys
            && keys.bindings() != self.cfg.voice_keys
        {
            keys.set_bindings(self.cfg.voice_keys.clone());
            // A hold mode may have just set its resting state.
            self.cfg.voice_on = self.shared.voice_on.load(Relaxed);
        }
        apply_feedback(&self.cfg, &self.shared);
        let requests: Vec<VoiceRequest> = match &self.voice_requests {
            Some(rx) => rx.try_iter().collect(),
            None => return,
        };
        for request in requests {
            let name = match request {
                VoiceRequest::Preset(name) => name,
                VoiceRequest::ToggleMelody => {
                    let on = !self.fx.on(P::MelodyOn);
                    self.fx.set(P::MelodyOn, if on { 1.0 } else { 0.0 });
                    self.effects_changed();
                    let state = if on { "ON" } else { "off" };
                    self.notify(ctx, format!("Melody Voice {state}"));
                    continue;
                }
            };
            let all = config::presets::builtin()
                .into_iter()
                .chain(self.cfg.user_presets.iter().cloned());
            let Some(preset) = all.into_iter().find(|p| p.name == name) else {
                self.notify(ctx, format!("Preset \"{name}\" no longer exists"));
                continue;
            };
            self.apply_preset(ctx, &preset);
            // Jumping to a preset means you want to hear it.
            self.feedback.set_voice(true, true);
            self.cfg.voice_on = true;
            self.refresh_tray();
            self.notify(ctx, format!("Preset: {}", preset.name));
        }
    }

    /// A toast, or a tray balloon while the window is hidden.
    fn notify(&mut self, ctx: &egui::Context, text: String) {
        match (&self.tray, self.hidden) {
            (Some(t), true) => t.notify("VoxShift", &text),
            _ => self.toasts.push(ctx, text),
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
                    ui.add_space(6.0);
                    self.hotkey_settings(ui);
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
                    self.melody_panel(ui);
                    ui.add_space(8.0);
                    if effects::panel(ui, &mut self.fx) {
                        self.effects_changed();
                    }
                }
                Tab::Soundboard => self.soundboard_tab(ui),
            });
        });

        self.handle_dropped_files(&ctx);
        self.poll_hotkey_capture(&ctx);
        self.key_conflict_dialog(&ctx);
        self.close_dialog(&ctx);
        self.toasts.show(&ctx);
        ctx.request_repaint_after(Duration::from_millis(33));
    }

    fn preset_names(&self) -> Vec<String> {
        config::presets::builtin()
            .into_iter()
            .map(|p| p.name)
            .chain(self.cfg.user_presets.iter().map(|p| p.name.clone()))
            .collect()
    }

    fn hotkey_settings(&mut self, ui: &mut Ui) {
        let view = hotkey_settings::View {
            capturing: self.capturing,
            hotkeys_available: self.hotkeys.as_ref().is_some_and(|h| h.available()),
            monitor_on: self.cfg.devices.monitor_enabled,
            preset_names: self.preset_names(),
        };
        for action in hotkey_settings::panel(ui, &mut self.cfg, &view) {
            match action {
                hotkey_settings::Action::Capture(t) => self.begin_capture(t),
                hotkey_settings::Action::CancelCapture => self.cancel_capture(),
                hotkey_settings::Action::Clear(t) => self.clear_hotkey(t),
                hotkey_settings::Action::AddPresetKey => {
                    let row = hotkey_settings::new_preset_key(
                        &self.cfg.voice_keys.presets,
                        &view.preset_names,
                    );
                    self.cfg.voice_keys.presets.push(row);
                }
                hotkey_settings::Action::RemovePresetKey(i) => {
                    if matches!(self.capturing, Some(KeyTarget::Preset(_))) {
                        self.cancel_capture(); // indexes are about to shift
                    }
                    if i < self.cfg.voice_keys.presets.len() {
                        self.cfg.voice_keys.presets.remove(i);
                    }
                }
            }
        }
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
                stop_key: self.cfg.voice_keys.stop_sounds.as_ref(),
                monitor_sounds_key: self.cfg.voice_keys.monitor_sounds_toggle.as_ref(),
                settings: &self.cfg.soundboard,
            };
            soundboard::panel(ui, &view, &mut self.search)
        };
        for a in actions {
            self.apply_sound_action(ui.ctx(), a);
        }
    }

    /// Runs `f` on the saved entries of all clips (in board order) and writes them back.
    fn edit_entries(&mut self, f: impl FnOnce(&mut organize::BoardSettings, &mut [SoundEntry])) {
        {
            let mut board = self.sounds.lock();
            let mut entries = board.entries();
            f(&mut self.cfg.soundboard, &mut entries);
            for (s, e) in board.sounds.iter_mut().zip(entries) {
                s.entry = e;
            }
        }
        self.sounds_changed();
    }

    fn edit_clip(&mut self, id: SoundId, f: impl FnOnce(&mut SoundEntry)) {
        if let Some(s) = self.sounds.lock().get_mut(id) {
            f(&mut s.entry);
        }
        self.sounds_changed();
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
            A::BeginHotkey(id) => self.begin_capture(KeyTarget::Sound(id)),
            A::CancelHotkey => self.cancel_capture(),
            A::ClearHotkey(id) => self.clear_hotkey(KeyTarget::Sound(id)),
            A::MonitorSounds(on) => {
                self.cfg.monitor_sounds = on;
                apply_to_shared(&self.cfg, &self.shared);
            }
            A::BeginActionKey(target) => self.begin_capture(target),
            A::ClearActionKey(target) => self.clear_hotkey(target),
            A::Tab(t) => self.cfg.soundboard.tab = t,
            A::Sort(s) => self.cfg.soundboard.sort = s,
            A::ToggleFavorite(id) => self.edit_clip(id, |e| e.favorite = !e.favorite),
            A::Color(id, c) => self.edit_clip(id, |e| e.color = c),
            A::MoveTo(id, folder) => {
                self.edit_clip(id, |e| e.folder = folder);
                let to = folder
                    .and_then(|f| self.cfg.soundboard.folder_name(f))
                    .unwrap_or("Unsorted");
                self.toasts.push(ctx, format!("Moved to {to}"));
            }
            A::Reorder(from, to) => {
                {
                    let mut board = self.sounds.lock();
                    let pos = |id| board.sounds.iter().position(|s| s.id == id);
                    if let (Some(a), Some(b)) = (pos(from), pos(to)) {
                        organize::reorder(&mut board.sounds, a, b);
                    }
                }
                self.sounds_changed();
            }
            A::Locate(id) => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Audio", &decode::EXTENSIONS)
                    .pick_file()
                {
                    self.sounds.lock().relocate(id, path);
                    self.sounds_changed();
                }
            }
            A::NewFolder => {
                let id = self.cfg.soundboard.create_folder("New folder");
                self.cfg.soundboard.tab = organize::BoardTab::Folder(id);
                self.toasts.push(
                    ctx,
                    "Folder created: right-click its tab to rename it, drag clips onto it",
                );
            }
            A::RenameFolder(id, name) => {
                self.cfg.soundboard.rename_folder(id, &name);
            }
            A::DeleteFolder(id) => {
                let name = self
                    .cfg
                    .soundboard
                    .folder_name(id)
                    .unwrap_or("")
                    .to_string();
                self.edit_entries(|s, entries| s.delete_folder(id, entries));
                self.toasts.push(
                    ctx,
                    format!("Deleted the folder \"{name}\"; its clips are in Unsorted"),
                );
            }
            A::MoveFolder(id, delta) => self.cfg.soundboard.move_folder(id, delta),
            A::ClearRecent => {
                self.sounds.lock().clear_recent();
                self.sounds_changed();
            }
        }
    }

    pub fn add_sounds(&mut self, ctx: &egui::Context, files: Vec<PathBuf>) {
        let (ok, skipped): (Vec<_>, Vec<_>) =
            files.into_iter().partition(|p| decode::is_supported(p));
        if !ok.is_empty() {
            // Dropped while a folder is open: they go into that folder.
            let folder = match self.cfg.soundboard.tab {
                organize::BoardTab::Folder(f) => Some(f),
                _ => None,
            };
            let mut board = self.sounds.lock();
            for p in &ok {
                board.add(SoundEntry {
                    folder,
                    ..SoundEntry::from_path(p.clone())
                });
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

    /// Copies the clips (and Recent) from the board into the config.
    fn sounds_changed(&mut self) {
        let board = self.sounds.lock();
        self.cfg.sounds = board.entries();
        self.cfg.soundboard.recent = board.recent.clone();
        self.board_revision = board.revision;
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
        // Preset packs are imported, MIDI files are melodies for Melody Voice; everything else
        // goes to the soundboard.
        let (packs, dropped): (Vec<_>, Vec<_>) =
            dropped.into_iter().partition(|p| config::pack::is_pack(p));
        for p in &packs {
            self.import_pack(ctx, p);
        }
        let (midi, sounds): (Vec<_>, Vec<_>) =
            dropped.into_iter().partition(|p| crate::melody::is_midi(p));
        if let Some(path) = midi.into_iter().next() {
            self.import_midi(ctx, path);
        }
        if !sounds.is_empty() {
            self.add_sounds(ctx, sounds);
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
                "Drop sounds for the soundboard, a MIDI file for Melody Voice, or a .voxpack",
                egui::FontId::proportional(28.0),
                Color32::WHITE,
            );
        }
    }

    /// Waits for the next key combination for `target` (pressed anywhere).
    fn begin_capture(&mut self, target: KeyTarget) {
        if let Some(h) = &self.hotkeys {
            h.begin_capture();
            self.capturing = Some(target);
            self.capture_watch = Some(CaptureWatch {
                presses_at_start: h.presses_seen(),
                window_key_at: None,
                last_log: 0.0,
            });
        }
    }

    fn cancel_capture(&mut self) {
        hotkeys::diag::log("window: the 'Press a key' button was clicked -> cancel");
        if let Some(h) = &self.hotkeys {
            h.cancel_capture();
        }
        self.capturing = None;
        self.capture_watch = None;
    }

    fn clear_hotkey(&mut self, target: KeyTarget) {
        self.set_hotkey(target, None);
    }

    /// Runs `f` on the whole keymap: the voice hotkeys and every clip's key.
    fn with_keymap<R>(
        &mut self,
        f: impl FnOnce(&mut voice_keys::VoiceHotkeys, &mut [keymap::SoundKey]) -> R,
    ) -> R {
        let r = {
            let mut board = self.sounds.lock();
            let mut keys: Vec<keymap::SoundKey> = board
                .sounds
                .iter_mut()
                .map(|s| keymap::SoundKey {
                    id: s.id,
                    name: &s.entry.name,
                    hotkey: &mut s.entry.hotkey,
                })
                .collect();
            f(&mut self.cfg.voice_keys, &mut keys)
        };
        self.sounds_changed();
        r
    }

    fn set_hotkey(&mut self, target: KeyTarget, key: Option<hotkeys::Hotkey>) {
        self.with_keymap(|voice, sounds| keymap::assign(voice, sounds, target, key));
    }

    /// What assigning `key` to `target` would do (see [`keymap::check`]), and how `target` is
    /// called.
    fn hotkey_check(&self, key: &hotkeys::Hotkey, target: KeyTarget) -> (keymap::Check, String) {
        let board = self.sounds.lock();
        let sounds = board
            .sounds
            .iter()
            .map(|s| (s.id, s.entry.name.as_str(), s.entry.hotkey.as_ref()));
        let check = keymap::check(key, target, &self.cfg.voice_keys, sounds.clone());
        (check, keymap::name_of(target, &self.cfg.voice_keys, sounds))
    }

    /// Who else uses this key combination, if anyone.
    #[cfg(test)]
    fn hotkey_conflict(&self, key: &hotkeys::Hotkey, target: KeyTarget) -> Option<String> {
        match self.hotkey_check(key, target).0 {
            keymap::Check::Conflict(o) => Some(o.name),
            _ => None,
        }
    }

    fn poll_hotkey_capture(&mut self, ctx: &egui::Context) {
        let (Some(h), Some(target)) = (&self.hotkeys, self.capturing) else {
            self.capture_watch = None;
            return;
        };
        let Some(result) = h.poll_capture() else {
            self.check_capture_watch(ctx);
            return;
        };
        self.capturing = None;
        self.capture_watch = None;
        let Captured::Key(key) = result else {
            hotkeys::diag::log("window: got 'cancelled' (Esc)");
            return;
        };
        hotkeys::diag::log(format!("window: got key {} -> checking", key.label));
        self.key_captured(ctx, target, key);
    }

    /// A key was pressed for `target`: assign it, refuse it, or ask (see [`keymap`]).
    fn key_captured(&mut self, ctx: &egui::Context, target: KeyTarget, key: hotkeys::Hotkey) {
        let label = key.label.clone();
        match self.hotkey_check(&key, target) {
            (keymap::Check::Reserved(why), _) => {
                self.toasts.push(
                    ctx,
                    format!("{label} can't be a hotkey: {why}. Pick another key."),
                );
            }
            (keymap::Check::AlreadyYours, _) => {
                self.set_hotkey(target, Some(key)); // keeps it (with the fresh label)
            }
            (keymap::Check::Assign, _) => {
                self.set_hotkey(target, Some(key));
                self.toasts.push(ctx, format!("Hotkey {label} assigned"));
            }
            (keymap::Check::Conflict(owner), name) => {
                // Ask. Until answered, no hotkey triggers anything (Tab, Enter and Esc in the
                // dialog must not start a clip or switch the voice changer).
                if let Some(h) = &self.hotkeys {
                    h.set_paused(true);
                }
                self.key_conflict = Some(KeyConflict {
                    key,
                    target,
                    target_name: name,
                    owner,
                    focus_pending: true,
                });
            }
        }
    }

    /// The answer to the "already used" dialog.
    fn resolve_key_conflict(&mut self, ctx: &egui::Context, change: bool) {
        let Some(c) = self.key_conflict.take() else {
            return;
        };
        if let Some(h) = &self.hotkeys {
            h.set_paused(false);
        }
        let label = c.key.label.clone();
        if change {
            let (key, target, owner) = (c.key, c.target, c.owner.target);
            self.with_keymap(|voice, sounds| keymap::change(voice, sounds, key, target, owner));
            self.toasts.push(
                ctx,
                format!(
                    "{label} now goes to {}. {} is not set any more.",
                    c.target_name,
                    capitalize(&c.owner.name)
                ),
            );
        } else {
            self.toasts
                .push(ctx, format!("Kept: {label} stays with {}.", c.owner.name));
        }
    }

    /// "F8 is already used for the voice changer toggle." Keep (default) or Change.
    fn key_conflict_dialog(&mut self, ctx: &egui::Context) {
        let Some(c) = &mut self.key_conflict else {
            return;
        };
        let mut choice = None;
        let response = egui::Modal::new(egui::Id::new("key_conflict")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.heading("Key already in use");
            ui.add_space(4.0);
            ui.label(format!(
                "{} is already used for {}.",
                c.key.label, c.owner.name
            ));
            ui.label(
                RichText::new(format!(
                    "Change moves it to {}; {} is then not set.",
                    c.target_name, c.owner.name
                ))
                .small()
                .weak(),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let keep = egui::Button::new(RichText::new("Keep").strong().color(Color32::WHITE))
                    .fill(ACCENT);
                let r = ui
                    .add(keep)
                    .on_hover_text("Leave both hotkeys as they were (Esc)");
                if std::mem::take(&mut c.focus_pending) {
                    r.request_focus(); // Enter picks the default; Tab moves to Change
                }
                if r.clicked() {
                    choice = Some(false);
                }
                if ui
                    .button("Change")
                    .on_hover_text(format!("Use {} here instead", c.key.label))
                    .clicked()
                {
                    choice = Some(true);
                }
            });
        });
        if choice.is_none() && response.should_close() {
            choice = Some(false); // Esc or a click outside: keep
        }
        if let Some(change) = choice {
            self.resolve_key_conflict(ctx, change);
        }
    }

    /// Applies a preset's sliders and, if it has one, its melody.
    fn apply_preset(&mut self, ctx: &egui::Context, p: &config::presets::Preset) {
        self.fx = config::presets::apply(p, &self.fx);
        self.effects_changed();
        self.cfg.preset = Some(p.name.clone());
        if let Some(source) = &p.melody
            && *source != self.cfg.melody.source
            && let Err(e) = self.select_melody(source.clone(), false)
        {
            self.notify(
                ctx,
                format!("Preset \"{}\": melody not loaded ({e})", p.name),
            );
        }
    }

    /// Where imported MIDI files are kept: a "melodies" folder next to the settings file.
    fn melody_dir(&self) -> PathBuf {
        self.config_path
            .as_ref()
            .and_then(|p| p.parent())
            .map(PathBuf::from)
            .unwrap_or_else(config::config_dir)
            .join("melodies")
    }

    /// Writes your presets and MIDI files to `path`.
    pub fn export_pack(&mut self, ctx: &egui::Context, path: &std::path::Path) {
        let (pack, skipped) = config::pack::export(&self.cfg.user_presets, &self.cfg.melody.files);
        if pack.presets.is_empty() && pack.melodies.is_empty() {
            self.toasts.push(
                ctx,
                "Nothing to share yet: save a preset of your own or import a MIDI file first.",
            );
            return;
        }
        match config::pack::save(path, &pack) {
            Ok(()) => {
                let mut msg = format!(
                    "Saved {} preset(s) and {} melody file(s) to {}",
                    pack.presets.len(),
                    pack.melodies.len(),
                    path.display()
                );
                if !skipped.is_empty() {
                    msg += &format!(". Left out: {}", skipped.join(", "));
                }
                self.toasts.push(ctx, msg);
            }
            Err(e) => self.toasts.push(ctx, format!("Could not share: {e}")),
        }
    }

    /// Adds the presets and MIDI files of a .voxpack.
    pub fn import_pack(&mut self, ctx: &egui::Context, path: &std::path::Path) {
        let result = config::pack::load(path).and_then(|pack| {
            config::pack::import(&pack, &self.cfg.user_presets, &self.melody_dir())
        });
        match result {
            Ok(got) => {
                for f in got.files.iter().rev() {
                    self.cfg.melody.remember_file(f);
                }
                let names: Vec<String> = got.presets.iter().map(|p| p.name.clone()).collect();
                self.cfg.user_presets.extend(got.presets);
                let mut msg = format!(
                    "Imported {} preset(s) and {} melody file(s)",
                    names.len(),
                    got.files.len()
                );
                if !names.is_empty() {
                    msg += &format!(": {} (pick them under Preset)", names.join(", "));
                }
                if got.already_had > 0 {
                    msg += &format!("; {} you already had", got.already_had);
                }
                self.toasts.push(ctx, msg);
                self.cfg.tab = Tab::Effects;
            }
            Err(e) => self.toasts.push(ctx, format!("Could not import: {e}")),
        }
    }

    fn apply_preset_action(&mut self, ctx: &egui::Context, action: presets::Action) {
        match action {
            presets::Action::Apply(p) => self.apply_preset(ctx, &p),
            presets::Action::Export => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("VoxShift preset pack", &[config::pack::EXTENSION])
                    .set_file_name(format!("VoxShift presets.{}", config::pack::EXTENSION))
                    .save_file()
                {
                    self.export_pack(ctx, &path);
                }
            }
            presets::Action::Import => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("VoxShift preset pack", &[config::pack::EXTENSION])
                    .pick_file()
                {
                    self.import_pack(ctx, &path);
                }
            }
            presets::Action::Save(name) => {
                let preset = config::presets::Preset {
                    name: name.clone(),
                    effects: self.fx.to_settings(),
                    melody: self
                        .fx
                        .on(P::MelodyOn)
                        .then(|| self.cfg.melody.source.clone()),
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

    /// While waiting for a hotkey: if this window itself received a key press but the global
    /// listener reported nothing, global keys are not reaching VoxShift. Say so instead of
    /// waiting forever.
    fn check_capture_watch(&mut self, ctx: &egui::Context) {
        let (Some(h), Some(watch)) = (&self.hotkeys, &mut self.capture_watch) else {
            return;
        };
        let now = self.clock.elapsed().as_secs_f64();
        let window_got_key = ctx.input(|i| {
            i.events
                .iter()
                .any(|e| matches!(e, egui::Event::Key { pressed: true, .. }))
        });
        if window_got_key {
            hotkeys::diag::log(format!(
                "window: this window got a key press (listener presses so far {})",
                h.presses_seen()
            ));
            if watch.window_key_at.is_none() {
                watch.window_key_at = Some(now);
            }
        }
        if now - watch.last_log >= 1.0 {
            watch.last_log = now;
            hotkeys::diag::log(format!(
                "window: still waiting (listener presses so far {})",
                h.presses_seen()
            ));
        }
        let silent = h.presses_seen() == watch.presses_at_start;
        if let Some(t) = watch.window_key_at
            && silent
            && now - t > CAPTURE_GRACE_S
        {
            hotkeys::diag::log("window: listener heard nothing -> giving up");
            h.cancel_capture();
            self.capturing = None;
            self.capture_watch = None;
            self.toasts.push(
                ctx,
                "VoxShift did not receive that key press, so hotkeys can't be set right now. \
                 Restart VoxShift and try again.",
            );
        }
    }

    fn effects_changed(&mut self) {
        self.shared.params.store(&self.fx);
        self.cfg.effects = self.fx.to_settings();
    }

    fn top_bar(&mut self, ui: &mut Ui) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            // The shared state is the truth: the tray and hotkeys change it too.
            let on = self.shared.voice_on.load(Relaxed);
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
            let tip = match &self.cfg.voice_keys.toggle {
                Some(k) => format!(
                    "Off = your clean voice passes straight through. Hotkey: {}",
                    k.label
                ),
                None => "Off = your clean voice passes straight through".to_string(),
            };
            if ui.add(button).on_hover_text(tip).clicked() {
                self.feedback.toggle_voice();
                self.cfg.voice_on = !on;
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
    fn melody_panel_draws_in_every_mode_and_names_the_notes() {
        use crate::dsp::params::P;
        let mut app = test_app();
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        app.fx.set(P::MelodyOn, 1.0);
        app.shared.melody_detected.set(57.2);
        app.shared.melody_target.set(60.0);
        let mut t = 0.0;
        for mode in 0..4 {
            app.fx.set(P::MelodyMode, mode as f32);
            for size in [(920.0, 600.0), (1920.0, 1080.0)] {
                frame(&ctx, &mut app, size, t, vec![]);
                t += 0.1;
            }
        }
        app.shared.melody_note.store(3, Relaxed);
        app.shared.melody_finished.store(true, Relaxed);
        frame(&ctx, &mut app, (1240.0, 800.0), t, vec![]);
        // The piano roll scrolls along a long melody, with both playheads.
        app.fx.set(P::MelodyMode, 1.0);
        app.shared.melody_beat.set(30.0);
        app.shared.melody_preview.store(true, Relaxed);
        app.shared.melody_preview_beat.set(40.0);
        frame(&ctx, &mut app, (1240.0, 800.0), t + 0.1, vec![]);
        app.capturing = Some(KeyTarget::MelodyNext);
        frame(&ctx, &mut app, (920.0, 600.0), t + 0.2, vec![]);
    }

    #[test]
    fn the_melody_hotkey_toggles_melody_voice_from_the_window_side() {
        let mut app = test_app();
        let ctx = egui::Context::default();
        let (tx, rx) = mpsc::channel();
        app.voice_requests = Some(rx);
        tx.send(VoiceRequest::ToggleMelody).unwrap();
        frame(&ctx, &mut app, (1240.0, 800.0), 0.0, vec![]);
        assert!(app.fx.on(P::MelodyOn));
        assert!(
            app.shared.params.get(P::MelodyOn) >= 0.5,
            "reaches the audio thread"
        );
        assert!(app.toasts.contains("Melody Voice ON"));
        tx.send(VoiceRequest::ToggleMelody).unwrap();
        frame(&ctx, &mut app, (1240.0, 800.0), 0.1, vec![]);
        assert!(!app.fx.on(P::MelodyOn));
        assert!(
            app.shared.voice_on.load(Relaxed),
            "the voice changer itself is untouched"
        );
    }

    #[test]
    fn presets_save_and_restore_the_melody() {
        let mut app = test_app();
        let ctx = egui::Context::default();
        let sing = config::presets::builtin()
            .into_iter()
            .find(|p| p.name == "Sing the Song (per-syllable)")
            .unwrap();
        app.select_melody(MelodySource::BuiltIn("Greensleeves".into()), false)
            .unwrap();
        app.apply_preset_action(&ctx, presets::Action::Apply(sing));
        assert!(app.fx.on(P::MelodyOn));
        assert_eq!(
            app.cfg.melody.source,
            MelodySource::BuiltIn("Greensleeves".into()),
            "a built-in preset keeps your melody"
        );
        app.apply_preset_action(&ctx, presets::Action::Save("Green".into()));
        let saved = app.cfg.user_presets.last().unwrap().clone();
        assert_eq!(
            saved.melody,
            Some(MelodySource::BuiltIn("Greensleeves".into()))
        );
        // Somewhere else, then back to the preset: the melody comes back too.
        app.select_melody(MelodySource::BuiltIn("Ode to Joy".into()), false)
            .unwrap();
        app.apply_preset_action(&ctx, presets::Action::Apply(saved));
        assert_eq!(
            app.cfg.melody.source,
            MelodySource::BuiltIn("Greensleeves".into())
        );
        let mut out = Vec::with_capacity(crate::melody::store::MAX_NOTES);
        app.shared.melody.read_into(&mut out).unwrap();
        assert_eq!(out[0].key, 69, "Greensleeves starts on A4");
        // With Melody Voice off, a saved preset does not carry a melody.
        app.fx.set(P::MelodyOn, 0.0);
        app.apply_preset_action(&ctx, presets::Action::Save("Plain".into()));
        assert_eq!(app.cfg.user_presets.last().unwrap().melody, None);
    }

    #[test]
    fn the_saved_melody_is_loaded_and_published_at_start() {
        let mut out = Vec::with_capacity(crate::melody::store::MAX_NOTES);
        // Default: the built-in default melody reaches the audio thread.
        let app = test_app();
        app.shared.melody.read_into(&mut out).unwrap();
        let twinkle = crate::melody::library::get("Twinkle Twinkle Little Star").unwrap();
        assert_eq!(out, twinkle.notes);

        // A saved choice is restored...
        let cfg = Config {
            melody: config::MelodySettings {
                source: MelodySource::BuiltIn("Ode to Joy".into()),
                files: vec![],
            },
            ..Config::default()
        };
        let app = VoxApp::with_manager(
            cfg,
            Arc::new(Shared::default()),
            ManagerHandle::detached(fake_status()),
        );
        app.shared.melody.read_into(&mut out).unwrap();
        assert_eq!(out.len(), 62);
        // ...and a missing file falls back to the default with a notice.
        let cfg = Config {
            melody: config::MelodySettings {
                source: MelodySource::File {
                    path: "C:/nope/gone.mid".into(),
                    part: None,
                },
                files: vec![],
            },
            ..Config::default()
        };
        let app = VoxApp::with_manager(
            cfg,
            Arc::new(Shared::default()),
            ManagerHandle::detached(fake_status()),
        );
        assert_eq!(app.cfg.melody.source, MelodySource::default());
        assert!(
            app.startup_notice
                .as_ref()
                .unwrap()
                .contains("Melody Voice")
        );
    }

    #[test]
    fn importing_a_midi_file_picks_the_melody_part_and_remembers_the_file() {
        use crate::melody::midi::tests::{TestNote, build};
        let dir = crate::soundboard::decode::tests::temp_dir("app-midi");
        let path = dir.join("my song.mid");
        let tune: Vec<TestNote> = (0..10).map(|i| (0, 76, i * 480, i * 480 + 400)).collect();
        let bass: Vec<TestNote> = (0..10).map(|i| (1, 36, i * 480, i * 480 + 400)).collect();
        std::fs::write(
            &path,
            build(midly::Format::Parallel, &[("Bass", &bass), ("Tune", &tune)]),
        )
        .unwrap();
        let mut app = test_app();
        let ctx = egui::Context::default();
        app.import_midi(&ctx, path.clone());
        assert_eq!(
            app.cfg.melody.source,
            MelodySource::File {
                path: path.clone(),
                part: Some(1)
            },
            "the higher part was guessed"
        );
        assert_eq!(app.cfg.melody.files, vec![path.clone()]);
        assert!(app.toasts.contains("2 of 2 parts"));
        assert!((app.fx.get(P::MelodyBpm) - 90.0).abs() < 0.01, "file tempo");
        let mut out = Vec::with_capacity(crate::melody::store::MAX_NOTES);
        app.shared.melody.read_into(&mut out).unwrap();
        assert!(out.iter().all(|n| n.key == 76));
        // The part picker's choice:
        app.select_melody(
            MelodySource::File {
                path: path.clone(),
                part: Some(0),
            },
            false,
        )
        .unwrap();
        app.shared.melody.read_into(&mut out).unwrap();
        assert!(out.iter().all(|n| n.key == 36));
        app.fx.set(P::MelodyOn, 1.0);
        frame(&ctx, &mut app, (1240.0, 800.0), 0.0, vec![]); // draws the part picker
        // A broken file leaves the current melody alone.
        let junk = dir.join("junk.mid");
        std::fs::write(&junk, b"nope").unwrap();
        app.import_midi(&ctx, junk);
        assert!(app.toasts.contains("Could not use that MIDI file"));
        assert!(matches!(
            app.cfg.melody.source,
            MelodySource::File { part: Some(0), .. }
        ));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn sharing_presets_and_melodies_with_a_friend_end_to_end() {
        use crate::melody::midi::tests::{TestNote, build};
        // Me: a MIDI file and a preset that sings it.
        let mine = crate::soundboard::decode::tests::temp_dir("share-me");
        let song = mine.join("song.mid");
        let tune: Vec<TestNote> = (0..10).map(|i| (0, 72, i * 480, i * 480 + 400)).collect();
        std::fs::write(&song, build(midly::Format::Parallel, &[("Tune", &tune)])).unwrap();
        let mut me = test_app();
        me.config_path = Some(mine.join("config.json"));
        let ctx = egui::Context::default();
        me.import_midi(&ctx, song.clone());
        me.fx.set(P::MelodyOn, 1.0);
        me.effects_changed();
        me.apply_preset_action(&ctx, presets::Action::Save("Our song".into()));
        let pack = mine.join("for you.voxpack");
        me.export_pack(&ctx, &pack);
        assert!(me.toasts.contains("Saved 1 preset(s) and 1 melody file(s)"));

        // My friend: a fresh VoxShift with their own settings folder.
        let theirs = crate::soundboard::decode::tests::temp_dir("share-friend");
        let mut friend = test_app();
        friend.config_path = Some(theirs.join("config.json"));
        friend.import_pack(&ctx, &pack);
        assert!(
            friend
                .toasts
                .contains("Imported 1 preset(s) and 1 melody file(s)")
        );
        let copy = theirs.join("melodies").join("song.mid");
        assert_eq!(friend.cfg.melody.files, vec![copy.clone()]);
        let preset = friend.cfg.user_presets[0].clone();
        assert_eq!(preset.name, "Our song");
        friend.apply_preset_action(&ctx, presets::Action::Apply(preset));
        assert!(friend.fx.on(P::MelodyOn));
        assert!(
            matches!(&friend.cfg.melody.source, MelodySource::File { path, .. } if *path == copy)
        );
        let mut out = Vec::with_capacity(crate::melody::store::MAX_NOTES);
        friend.shared.melody.read_into(&mut out).unwrap();
        assert!(
            out.len() == 10 && out.iter().all(|n| n.key == 72),
            "the song plays"
        );
        // Importing twice does not double anything.
        friend.import_pack(&ctx, &pack);
        assert_eq!(friend.cfg.user_presets.len(), 1);
        assert!(friend.toasts.contains("1 you already had"));
        // Nothing of your own yet: nothing to share.
        let mut empty = test_app();
        empty.export_pack(&ctx, &mine.join("empty.voxpack"));
        assert!(empty.toasts.contains("Nothing to share yet"));
        std::fs::remove_dir_all(mine).ok();
        std::fs::remove_dir_all(theirs).ok();
    }

    #[test]
    fn picking_a_melody_adopts_its_tempo_and_restart_reaches_the_audio_thread() {
        let mut app = test_app();
        app.select_melody(
            MelodySource::BuiltIn("F\u{fc}r Elise (opening)".into()),
            true,
        )
        .unwrap();
        assert_eq!(app.fx.get(P::MelodyBpm), 70.0);
        assert_eq!(app.shared.params.snapshot().get(P::MelodyBpm), 70.0);
        let before = app.shared.melody_restart.load(Relaxed);
        let ctx = egui::Context::default();
        app.fx.set(P::MelodyOn, 1.0);
        frame(&ctx, &mut app, (1240.0, 800.0), 0.0, vec![]);
        // What the Restart button does:
        app.shared.melody_restart.fetch_add(1, Relaxed);
        assert_eq!(app.shared.melody_restart.load(Relaxed), before + 1);
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

    /// The whole assign-a-hotkey flow with the real keyboard hook: click "Set key" (as the
    /// UI action), press a key (F24, injected, harmless), and the window must save it.
    #[cfg(windows)]
    #[test]
    fn assigning_a_hotkey_end_to_end() {
        use crate::soundboard::hotkeys::tests::{HOOK_TESTS, tap_f24};
        let _serial = HOOK_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let mut app = test_app();
        app.hotkeys = Some(HotkeyService::start(app.sounds.board.clone(), None));
        let ctx = egui::Context::default();
        app.add_sounds(&ctx, vec!["C:/nope/airhorn.wav".into()]);
        let id = app.sounds.lock().sounds[0].id;
        frame(&ctx, &mut app, (1240.0, 800.0), 0.0, vec![]);

        app.apply_sound_action(&ctx, soundboard::Action::BeginHotkey(id));
        frame(&ctx, &mut app, (1240.0, 800.0), 0.1, vec![]);
        assert_eq!(
            app.capturing,
            Some(KeyTarget::Sound(id)),
            "waiting for a key"
        );
        std::thread::sleep(Duration::from_millis(100));
        tap_f24();
        let mut t = 0.2;
        for _ in 0..100 {
            frame(&ctx, &mut app, (1240.0, 800.0), t, vec![]);
            t += 0.033;
            if app.capturing.is_none() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(app.capturing, None, "capture finished");
        let saved = app.cfg.sounds[0]
            .hotkey
            .clone()
            .expect("hotkey saved in the config");
        assert_eq!(saved.vk, 0x87);
    }

    /// If the window gets a key press but the global listener reports nothing, the capture
    /// must end with an explanation instead of waiting forever.
    #[cfg(windows)]
    #[test]
    fn capture_gives_up_with_a_message_when_the_listener_hears_nothing() {
        use crate::soundboard::hotkeys::tests::HOOK_TESTS;
        let _serial = HOOK_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let mut app = test_app();
        app.hotkeys = Some(HotkeyService::start(app.sounds.board.clone(), None));
        let ctx = egui::Context::default();
        app.add_sounds(&ctx, vec!["C:/nope/airhorn.wav".into()]);
        let id = app.sounds.lock().sounds[0].id;
        app.apply_sound_action(&ctx, soundboard::Action::BeginHotkey(id));
        // Only egui sees this key (nothing is injected), like a listener that went deaf.
        let key = vec![egui::Event::Key {
            key: egui::Key::G,
            physical_key: Some(egui::Key::G),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }];
        frame(&ctx, &mut app, (1240.0, 800.0), 0.0, key);
        assert_eq!(
            app.capturing,
            Some(KeyTarget::Sound(id)),
            "a short delay is allowed"
        );
        std::thread::sleep(Duration::from_millis(900));
        frame(&ctx, &mut app, (1240.0, 800.0), 1.0, vec![]);
        assert_eq!(app.capturing, None, "gave up");
        assert!(app.cfg.sounds[0].hotkey.is_none());
        assert!(app.toasts.contains("did not receive that key press"));
    }

    /// Waits until `cond` holds, running app frames meanwhile (up to 2 s).
    #[cfg(windows)]
    fn wait_for(
        ctx: &egui::Context,
        app: &mut VoxApp,
        mut cond: impl FnMut(&VoxApp) -> bool,
    ) -> bool {
        for i in 0..100 {
            frame(ctx, app, (1240.0, 800.0), f64::from(i) * 0.02, vec![]);
            if cond(app) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    /// The real key listener: F24 (a key no keyboard has) as the voice changer hotkey.
    #[cfg(windows)]
    #[test]
    fn voice_hotkey_toggles_and_holds_with_the_real_listener() {
        use crate::soundboard::hotkeys::tests::{HOOK_TESTS, VK_F24, f24, tap_f24};
        use crate::voice_keys::VoiceKeyMode;
        let _serial = HOOK_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let mut app = test_app();
        let ctx = egui::Context::default();
        app.start_hotkeys(ctx.clone());
        app.cfg.voice_keys.toggle = Some(hotkeys::Hotkey::new(
            VK_F24, false, false, false, false, "F24",
        ));
        app.shared.cue_enabled.store(true, Relaxed);
        frame(&ctx, &mut app, (1240.0, 800.0), 0.0, vec![]); // pushes the binding
        let before = app.fx;
        std::thread::sleep(Duration::from_millis(100));

        tap_f24();
        assert!(
            wait_for(&ctx, &mut app, |a| !a.cfg.voice_on),
            "first press: clean"
        );
        assert_eq!(
            app.shared.take_cue(),
            Some(crate::dsp::cue::CueKind::VoiceOff),
            "OFF beep"
        );
        tap_f24();
        assert!(
            wait_for(&ctx, &mut app, |a| a.cfg.voice_on),
            "second press: effects"
        );
        assert_eq!(app.fx, before, "sliders untouched");

        // Push-to-talk effect: clean at rest, effects only while held.
        app.cfg.voice_keys.mode = VoiceKeyMode::HoldForEffects;
        assert!(wait_for(&ctx, &mut app, |a| !a.cfg.voice_on), "rests clean");
        f24(true);
        assert!(
            wait_for(&ctx, &mut app, |a| a.cfg.voice_on),
            "held: effects"
        );
        f24(false);
        assert!(
            wait_for(&ctx, &mut app, |a| !a.cfg.voice_on),
            "released: clean"
        );
    }

    /// The real key listener: F24 as Melody Voice's "next note" key.
    #[cfg(windows)]
    #[test]
    fn melody_next_key_works_with_the_real_listener() {
        use crate::soundboard::hotkeys::tests::{HOOK_TESTS, VK_F24, tap_f24};
        let _serial = HOOK_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let mut app = test_app();
        let ctx = egui::Context::default();
        app.start_hotkeys(ctx.clone());
        app.cfg.voice_keys.melody_next = Some(hotkeys::Hotkey::new(
            VK_F24, false, false, false, false, "F24",
        ));
        frame(&ctx, &mut app, (1240.0, 800.0), 0.0, vec![]); // pushes the binding
        std::thread::sleep(Duration::from_millis(100));
        for presses in 1..=2 {
            tap_f24();
            assert!(
                wait_for(&ctx, &mut app, |a| a.shared.melody_next.load(Relaxed)
                    == presses),
                "press {presses}: one step each"
            );
        }
        assert!(app.cfg.voice_on, "nothing else changed");
        // Let the last key-up arrive here, not in the next listener test.
        std::thread::sleep(Duration::from_millis(200));
        frame(&ctx, &mut app, (1240.0, 800.0), 5.0, vec![]);
    }

    #[test]
    fn hear_myself_has_one_shared_state_for_hotkey_tray_and_checkbox() {
        let mut app = test_app();
        let ctx = egui::Context::default();
        app.cfg.devices.monitor_enabled = true;
        frame(&ctx, &mut app, (1240.0, 800.0), 0.0, vec![]);
        assert!(app.cfg.monitor_voice);
        // The hotkey and the tray both go through Feedback::toggle_monitor.
        assert!(!app.feedback.toggle_monitor());
        frame(&ctx, &mut app, (1240.0, 800.0), 0.1, vec![]);
        assert!(!app.cfg.monitor_voice, "the checkbox follows");
        assert!(!app.mirror.lock().unwrap().monitor_voice, "and it is saved");
        let dev = app.status.clone();
        let tray = tray_status::compute(
            &dev,
            app.shared.voice_on.load(Relaxed),
            app.shared.mic_muted.load(Relaxed),
            app.shared.monitor_voice.load(Relaxed),
        );
        assert!(!tray.monitor, "the tray menu shows it unticked");
        // The checkbox path (devices panel) writes the same atomic back.
        app.cfg.monitor_voice = true;
        apply_to_shared(&app.cfg, &app.shared);
        assert!(app.shared.monitor_voice.load(Relaxed));
        frame(&ctx, &mut app, (1240.0, 800.0), 0.2, vec![]);
        assert!(app.cfg.monitor_voice);
        // Toggling never touches the voice changer, the effects or the soundboard monitor.
        let fx = app.fx;
        app.feedback.toggle_monitor();
        frame(&ctx, &mut app, (1240.0, 800.0), 0.3, vec![]);
        assert_eq!(app.fx, fx);
        assert!(app.cfg.voice_on && app.cfg.monitor_sounds);

        // "Also play in my headphones" works the same way, on the Soundboard tab too.
        app.cfg.tab = Tab::Soundboard;
        assert!(!app.feedback.toggle_monitor_sounds());
        frame(&ctx, &mut app, (1240.0, 800.0), 0.4, vec![]);
        assert!(!app.cfg.monitor_sounds, "the checkbox follows");
        assert!(
            !app.mirror.lock().unwrap().monitor_sounds,
            "and it is saved"
        );
        app.apply_sound_action(&ctx, soundboard::Action::MonitorSounds(true)); // the checkbox
        assert!(app.shared.monitor_sounds.load(Relaxed));
        frame(&ctx, &mut app, (1240.0, 800.0), 0.5, vec![]);
        assert!(app.cfg.monitor_sounds);
        app.set_hotkey(
            KeyTarget::MonitorSoundsToggle,
            Some(hotkeys::Hotkey::new(0x4A, false, false, false, false, "J")),
        );
        assert_eq!(
            app.cfg
                .voice_keys
                .monitor_sounds_toggle
                .as_ref()
                .unwrap()
                .label,
            "J"
        );
    }

    #[test]
    fn monitor_and_stop_keys_are_assigned_cleared_and_refused_when_taken() {
        let mut app = test_app();
        let ctx = egui::Context::default();
        let h = hotkeys::Hotkey::new(0x48, false, false, true, false, "Alt+H");
        app.set_hotkey(KeyTarget::MonitorToggle, Some(h.clone()));
        app.set_hotkey(
            KeyTarget::StopSounds,
            Some(hotkeys::Hotkey::new(
                0x7B, false, false, false, false, "F12",
            )),
        );
        assert_eq!(
            app.cfg.voice_keys.monitor_toggle.as_ref().unwrap().label,
            "Alt+H"
        );
        assert_eq!(app.cfg.voice_keys.stop_sounds.as_ref().unwrap().vk, 0x7B);
        assert_eq!(
            app.hotkey_conflict(&h, KeyTarget::StopSounds).as_deref(),
            Some("the monitor (hear myself) toggle")
        );
        app.add_sounds(&ctx, vec!["C:/nope/airhorn.wav".into()]);
        let id = app.sounds.lock().sounds[0].id;
        assert_eq!(
            app.hotkey_conflict(&h, KeyTarget::Sound(id)).as_deref(),
            Some("the monitor (hear myself) toggle"),
            "a clip cannot take it either"
        );
        // The settings panel draws while one of them waits for a key.
        app.capturing = Some(KeyTarget::StopSounds);
        frame(&ctx, &mut app, (920.0, 600.0), 0.0, vec![]);
        app.clear_hotkey(KeyTarget::MonitorToggle);
        assert!(app.cfg.voice_keys.monitor_toggle.is_none());
    }

    /// The real key listener: F24 (a key no keyboard has) as the "hear myself" key.
    #[cfg(windows)]
    #[test]
    fn monitor_key_works_with_the_real_listener() {
        use crate::soundboard::hotkeys::tests::{HOOK_TESTS, VK_F24, tap_f24};
        let _serial = HOOK_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let mut app = test_app();
        let ctx = egui::Context::default();
        app.start_hotkeys(ctx.clone());
        app.cfg.voice_keys.monitor_toggle = Some(hotkeys::Hotkey::new(
            VK_F24, false, false, false, false, "F24",
        ));
        frame(&ctx, &mut app, (1240.0, 800.0), 0.0, vec![]); // pushes the binding
        std::thread::sleep(Duration::from_millis(100));
        tap_f24();
        assert!(wait_for(&ctx, &mut app, |a| !a.cfg.monitor_voice), "off");
        tap_f24();
        assert!(
            wait_for(&ctx, &mut app, |a| a.cfg.monitor_voice),
            "on again"
        );
        assert!(app.cfg.voice_on, "nothing else changed");
        // Let the last key-up arrive here, not in the next listener test.
        std::thread::sleep(Duration::from_millis(200));
        frame(&ctx, &mut app, (1240.0, 800.0), 5.0, vec![]);
    }

    /// Assigning a key another action already uses is refused.
    fn key_event(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    /// An app where the clip "airhorn" has F8; returns it with the clip's id.
    fn app_with_airhorn_on_f8(ctx: &egui::Context) -> (VoxApp, crate::soundboard::mixer::SoundId) {
        let mut app = test_app();
        app.add_sounds(ctx, vec!["C:/nope/airhorn.wav".into()]);
        let id = app.sounds.lock().sounds[0].id;
        app.set_hotkey(
            KeyTarget::Sound(id),
            Some(hotkeys::Hotkey::new(0x77, false, false, false, false, "F8")),
        );
        app.cfg.tab = Tab::Effects;
        (app, id)
    }

    fn f8() -> hotkeys::Hotkey {
        hotkeys::Hotkey::new(0x77, false, false, false, false, "F8")
    }

    #[test]
    fn a_taken_key_opens_the_dialog_and_esc_or_enter_keep_both() {
        let ctx = egui::Context::default();
        for (name, events) in [
            (
                "Esc",
                vec![key_event(egui::Key::Escape, egui::Modifiers::NONE)],
            ),
            (
                "Enter",
                vec![key_event(egui::Key::Enter, egui::Modifiers::NONE)],
            ),
        ] {
            let (mut app, id) = app_with_airhorn_on_f8(&ctx);
            app.key_captured(&ctx, KeyTarget::MonitorToggle, f8());
            let c = app.key_conflict.as_ref().expect("asks");
            assert_eq!(c.owner.name, "the sound \"airhorn\"", "names the clip");
            frame(&ctx, &mut app, (1240.0, 800.0), 0.0, vec![]); // dialog shows, Keep focused
            frame(&ctx, &mut app, (1240.0, 800.0), 0.1, events);
            assert!(app.key_conflict.is_none(), "{name} answers the dialog");
            assert!(
                app.cfg.voice_keys.monitor_toggle.is_none(),
                "{name}: nothing moved"
            );
            assert_eq!(app.cfg.sounds[0].hotkey.as_ref().unwrap().label, "F8");
            assert!(
                app.sounds
                    .lock()
                    .get_mut(id)
                    .unwrap()
                    .entry
                    .hotkey
                    .is_some()
            );
            assert!(
                app.toasts
                    .contains("Kept: F8 stays with the sound \"airhorn\"")
            );
        }
    }

    #[test]
    fn tab_then_enter_changes_and_the_old_owner_is_not_set() {
        let ctx = egui::Context::default();
        let (mut app, _) = app_with_airhorn_on_f8(&ctx);
        app.key_captured(&ctx, KeyTarget::StopSounds, f8());
        frame(&ctx, &mut app, (1240.0, 800.0), 0.0, vec![]);
        let tab = key_event(egui::Key::Tab, egui::Modifiers::NONE);
        frame(&ctx, &mut app, (1240.0, 800.0), 0.1, vec![tab]); // Keep -> Change
        let enter = key_event(egui::Key::Enter, egui::Modifiers::NONE);
        frame(&ctx, &mut app, (1240.0, 800.0), 0.2, vec![enter]);
        assert!(app.key_conflict.is_none());
        assert_eq!(app.cfg.voice_keys.stop_sounds.as_ref().unwrap().label, "F8");
        assert!(
            app.cfg.sounds[0].hotkey.is_none(),
            "airhorn is not set any more"
        );
        assert!(app.toasts.contains("F8 now goes to stop all sounds"));
        assert!(
            app.toasts
                .contains("The sound \"airhorn\" is not set any more")
        );
        // And the soundboard tab shows it as "Not set".
        app.cfg.tab = Tab::Soundboard;
        frame(&ctx, &mut app, (1240.0, 800.0), 0.3, vec![]);
    }

    #[test]
    fn change_works_for_voice_keys_too_and_same_key_or_reserved_never_ask() {
        let ctx = egui::Context::default();
        let mut app = test_app();
        app.set_hotkey(KeyTarget::Toggle, Some(f8()));
        // A clip takes F8 from the voice changer toggle.
        app.add_sounds(&ctx, vec!["C:/nope/bruh.wav".into()]);
        let id = app.sounds.lock().sounds[0].id;
        app.key_captured(&ctx, KeyTarget::Sound(id), f8());
        assert_eq!(
            app.key_conflict.as_ref().unwrap().owner.name,
            "the voice changer toggle"
        );
        app.resolve_key_conflict(&ctx, true);
        assert!(app.cfg.voice_keys.toggle.is_none());
        assert_eq!(app.cfg.sounds[0].hotkey.as_ref().unwrap().label, "F8");
        // Pressing F8 again for that clip: accepted silently, no dialog.
        app.toasts = Toasts::default();
        app.key_captured(&ctx, KeyTarget::Sound(id), f8());
        assert!(app.key_conflict.is_none());
        assert!(!app.toasts.contains("F8"), "nothing to say");
        assert_eq!(app.cfg.sounds[0].hotkey.as_ref().unwrap().label, "F8");
        // Windows keys are refused with a reason, without a dialog.
        let win_r = hotkeys::Hotkey::new(0x52, false, false, false, true, "Win+R");
        app.key_captured(&ctx, KeyTarget::Mute, win_r);
        assert!(app.key_conflict.is_none() && app.cfg.voice_keys.mute.is_none());
        assert!(app.toasts.contains("Win+R can't be a hotkey"));
        // A free key is simply assigned.
        let f9 = hotkeys::Hotkey::new(0x78, false, false, false, false, "F9");
        app.key_captured(&ctx, KeyTarget::Mute, f9);
        assert_eq!(app.cfg.voice_keys.mute.as_ref().unwrap().label, "F9");
        assert!(app.toasts.contains("Hotkey F9 assigned"));
    }

    #[test]
    fn duplicate_hotkeys_in_a_loaded_config_are_cleared_once_with_a_notice() {
        let dir = crate::soundboard::decode::tests::temp_dir("dupe-config");
        let path = dir.join("config.json");
        let mut cfg = Config::default();
        cfg.voice_keys.toggle = Some(f8());
        cfg.voice_keys.stop_sounds = Some(f8());
        cfg.sounds.push(SoundEntry {
            name: "airhorn".into(),
            path: "C:/nope/airhorn.wav".into(),
            volume: 1.0,
            hotkey: Some(f8()),
            ..Default::default()
        });
        let start = |cfg: Config| {
            VoxApp::with_manager(
                cfg,
                Arc::new(Shared::default()),
                ManagerHandle::detached(fake_status()),
            )
        };
        let mut app = start(cfg);
        app.config_path = Some(path.clone());
        assert_eq!(
            app.cfg.voice_keys.toggle.as_ref().unwrap().label,
            "F8",
            "first keeps it"
        );
        assert!(app.cfg.voice_keys.stop_sounds.is_none());
        assert!(app.cfg.sounds[0].hotkey.is_none());
        assert!(
            app.sounds.lock().sounds[0].entry.hotkey.is_none(),
            "the board agrees"
        );
        let notice = app.startup_notice.clone().unwrap();
        assert!(notice.contains("F8 (stop all sounds)"), "{notice}");
        assert!(notice.contains("F8 (the sound \"airhorn\")"), "{notice}");
        // The cleaned config is saved, so the next start has nothing to report.
        app.autosave(0.0);
        app.autosave(2.0);
        let (reloaded, _) = config::load_from(&path);
        let app = start(reloaded);
        assert!(app.startup_notice.is_none(), "{:?}", app.startup_notice);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn an_old_config_migrates_clips_keep_their_paths_and_start_unsorted() {
        let dir = crate::soundboard::decode::tests::temp_dir("old-soundboard");
        let path = dir.join("config.json");
        // A config from before folders, favourites and ids.
        std::fs::write(
            &path,
            r#"{ "sounds": [
                { "name": "airhorn", "path": "C:/s/airhorn.mp3", "volume": 0.5,
                  "hotkey": { "vk": 97, "ctrl": true, "shift": false, "alt": false,
                              "win": false, "label": "Ctrl+Num 1" } },
                { "name": "bruh", "path": "C:/s/bruh.wav", "volume": 0.8, "hotkey": null }
            ] }"#,
        )
        .unwrap();
        let (cfg, warning) = config::load_from(&path);
        assert!(warning.is_none(), "{warning:?}");
        let mut app = VoxApp::with_manager(
            cfg,
            Arc::new(Shared::default()),
            ManagerHandle::detached(fake_status()),
        );
        app.config_path = Some(path.clone());
        let s = &app.cfg.sounds;
        assert_eq!(
            (s[0].name.as_str(), s[1].name.as_str()),
            ("airhorn", "bruh")
        );
        assert_eq!(s[0].path, PathBuf::from("C:/s/airhorn.mp3"));
        assert_eq!(s[0].hotkey.as_ref().unwrap().label, "Ctrl+Num 1");
        assert!(
            s.iter()
                .all(|e| e.folder.is_none() && !e.favorite && e.color.is_none())
        );
        assert_eq!((s[0].uid, s[1].uid), (1, 2), "permanent ids handed out");
        assert_eq!(app.cfg.soundboard, organize::BoardSettings::default());
        // Saved straight away, and the ids are stable from then on.
        app.autosave(0.0);
        app.autosave(2.0);
        let (again, _) = config::load_from(&path);
        assert_eq!(again.sounds[1].uid, 2);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn folders_favourites_colours_and_order_through_the_ui_actions() {
        use soundboard::Action as A;
        let ctx = egui::Context::default();
        let mut app = test_app();
        app.add_sounds(
            &ctx,
            vec![
                "C:/nope/airhorn.wav".into(),
                "C:/nope/bruh.wav".into(),
                "C:/nope/Écoute.wav".into(),
            ],
        );
        let ids: Vec<SoundId> = app.sounds.lock().sounds.iter().map(|s| s.id).collect();
        let draw = |app: &mut VoxApp, t: f64| {
            for size in [(920.0, 600.0), (1920.0, 1080.0)] {
                frame(&ctx, app, size, t, vec![]);
            }
        };
        draw(&mut app, 0.0); // files are missing: greyed out with "Locate..."

        app.apply_sound_action(&ctx, A::NewFolder);
        let memes = app.cfg.soundboard.folders[0].id;
        assert_eq!(app.cfg.soundboard.tab, organize::BoardTab::Folder(memes));
        app.apply_sound_action(&ctx, A::RenameFolder(memes, "Memes".into()));
        app.apply_sound_action(&ctx, A::MoveTo(ids[1], Some(memes)));
        app.apply_sound_action(&ctx, A::ToggleFavorite(ids[2]));
        app.apply_sound_action(&ctx, A::Color(ids[0], Some(4)));
        app.apply_sound_action(&ctx, A::Sort(organize::SortMode::Name));
        draw(&mut app, 0.1);
        let e = &app.cfg.sounds;
        assert_eq!(e[1].folder, Some(memes));
        assert!(e[2].favorite && e[0].color == Some(4));
        assert_eq!(app.cfg.soundboard.folder_name(memes), Some("Memes"));

        // Dropping more files while a folder is open puts them in it.
        app.add_sounds(&ctx, vec!["C:/nope/wow.wav".into()]);
        assert_eq!(app.cfg.sounds[3].folder, Some(memes));

        // Manual order: drag the last clip onto the first.
        app.apply_sound_action(&ctx, A::Sort(organize::SortMode::Manual));
        let wow = app.sounds.lock().sounds[3].id;
        app.apply_sound_action(&ctx, A::Reorder(wow, ids[0]));
        assert_eq!(app.cfg.sounds[0].name, "wow");

        // Searching shows clips of every folder, accents ignored.
        app.search = "ecou".into();
        app.cfg.soundboard.tab = organize::BoardTab::Folder(memes);
        let shown = organize::visible(&app.cfg.sounds, &app.cfg.soundboard, &app.search);
        assert_eq!(shown.len(), 1);
        assert_eq!(app.cfg.sounds[shown[0]].name, "Écoute");
        draw(&mut app, 0.2);
        app.search.clear();

        // Deleting the folder keeps every clip, in Unsorted.
        app.apply_sound_action(&ctx, A::DeleteFolder(memes));
        assert_eq!(app.cfg.sounds.len(), 4);
        assert!(app.cfg.sounds.iter().all(|e| e.folder.is_none()));
        assert_eq!(app.cfg.soundboard.tab, organize::BoardTab::Unsorted);
        for tab in [
            organize::BoardTab::All,
            organize::BoardTab::Favorites,
            organize::BoardTab::Recent,
        ] {
            app.apply_sound_action(&ctx, A::Tab(tab));
            draw(&mut app, 0.3);
        }
        assert_eq!(
            app.cfg.soundboard.tab,
            organize::BoardTab::Recent,
            "remembered"
        );
    }

    #[test]
    fn stop_all_and_hear_soundboard_keys_can_be_set_on_the_soundboard_tab() {
        let ctx = egui::Context::default();
        let (mut app, _) = app_with_airhorn_on_f8(&ctx);
        app.cfg.tab = Tab::Soundboard;
        // The buttons start a capture for the same targets as the settings rows...
        app.apply_sound_action(
            &ctx,
            soundboard::Action::BeginActionKey(KeyTarget::StopSounds),
        );
        frame(&ctx, &mut app, (1240.0, 800.0), 0.0, vec![]); // "Press a key..." shows
        let f10 = hotkeys::Hotkey::new(0x79, false, false, false, false, "F10");
        app.key_captured(&ctx, KeyTarget::StopSounds, f10);
        assert_eq!(
            app.cfg.voice_keys.stop_sounds.as_ref().unwrap().label,
            "F10"
        );
        // ...so they share the conflict dialog with everything else.
        app.key_captured(&ctx, KeyTarget::MonitorSoundsToggle, f8());
        assert_eq!(
            app.key_conflict.as_ref().unwrap().owner.name,
            "the sound \"airhorn\""
        );
        app.resolve_key_conflict(&ctx, true);
        assert_eq!(
            app.cfg
                .voice_keys
                .monitor_sounds_toggle
                .as_ref()
                .unwrap()
                .label,
            "F8"
        );
        for size in [(920.0, 600.0), (1920.0, 1080.0)] {
            frame(&ctx, &mut app, size, 0.1, vec![]); // both keys shown, airhorn "Not set"
        }
        app.apply_sound_action(
            &ctx,
            soundboard::Action::ClearActionKey(KeyTarget::StopSounds),
        );
        assert!(app.cfg.voice_keys.stop_sounds.is_none());
        // The capture can be cancelled from there too.
        app.capturing = Some(KeyTarget::MonitorSoundsToggle);
        app.apply_sound_action(&ctx, soundboard::Action::CancelHotkey);
        assert!(app.capturing.is_none());
    }

    /// With the real listener: pressing a taken key asks, and while the dialog is open that
    /// key triggers nothing (the clip does not play, the global hotkeys are paused).
    #[cfg(windows)]
    #[test]
    fn duplicate_hotkeys_ask_and_pause_the_listener() {
        use crate::soundboard::hotkeys::tests::{HOOK_TESTS, VK_F24, tap_f24};
        let _serial = HOOK_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let mut app = test_app();
        let ctx = egui::Context::default();
        app.start_hotkeys(ctx.clone());
        app.add_sounds(&ctx, vec!["C:/nope/airhorn.wav".into()]);
        let id = app.sounds.lock().sounds[0].id;
        let f24_key = hotkeys::Hotkey::new(VK_F24, false, false, false, false, "F24");
        app.set_hotkey(KeyTarget::Sound(id), Some(f24_key));

        app.begin_capture(KeyTarget::Toggle);
        frame(&ctx, &mut app, (1240.0, 800.0), 0.0, vec![]);
        std::thread::sleep(Duration::from_millis(100));
        tap_f24();
        assert!(wait_for(&ctx, &mut app, |a| a.capturing.is_none()));
        let c = app.key_conflict.as_ref().expect("the dialog is open");
        assert_eq!(c.owner.name, "the sound \"airhorn\"");
        assert!(app.hotkeys.as_ref().unwrap().paused());
        assert!(app.cfg.voice_keys.toggle.is_none(), "nothing assigned yet");
        app.resolve_key_conflict(&ctx, false);
        assert!(
            !app.hotkeys.as_ref().unwrap().paused(),
            "hotkeys work again"
        );
        assert!(app.cfg.voice_keys.toggle.is_none());
        std::thread::sleep(Duration::from_millis(200)); // the key-up arrives here
        frame(&ctx, &mut app, (1240.0, 800.0), 5.0, vec![]);
    }

    #[test]
    fn hotkey_settings_draw_in_every_state() {
        let mut app = test_app();
        let ctx = egui::Context::default();
        app.cfg.voice_keys.toggle =
            Some(hotkeys::Hotkey::new(0x77, false, false, false, false, "F8"));
        app.cfg
            .voice_keys
            .presets
            .push(crate::voice_keys::PresetHotkey {
                preset: "Robot".into(),
                hotkey: None,
            });
        app.capturing = Some(KeyTarget::Preset(0));
        for size in [(920.0, 600.0), (1920.0, 1080.0)] {
            frame(&ctx, &mut app, size, 0.0, vec![]);
        }
    }

    #[test]
    fn start_state_setting_is_applied() {
        for (start, saved, want) in [
            (StartVoice::RememberLast, false, false),
            (StartVoice::RememberLast, true, true),
            (StartVoice::On, false, true),
            (StartVoice::Off, true, false),
        ] {
            let cfg = Config {
                start_voice: start,
                voice_on: saved,
                ..Config::default()
            };
            let app = VoxApp::with_manager(
                cfg,
                Arc::new(Shared::default()),
                ManagerHandle::detached(fake_status()),
            );
            assert_eq!(
                app.shared.voice_on.load(Relaxed),
                want,
                "{start:?} saved={saved}"
            );
        }
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
        app.capturing = Some(KeyTarget::Sound(app.sounds.lock().sounds[0].id));
        for (i, size) in [(920.0, 600.0), (1920.0, 1080.0)].into_iter().enumerate() {
            frame(&ctx, &mut app, size, i as f64, vec![]);
        }
    }
}
