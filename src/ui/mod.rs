//! The window: master toggle and meters on top, devices on the left, effects and soundboard in
//! the middle.

pub mod devices;
pub mod meters;
pub mod theme;
pub mod toast;

use crate::audio::device_manager::{DeviceStatus, Event, ManagerCmd, ManagerHandle};
use crate::audio::shared::{ActiveMic, Shared};
use crate::config::Config;
use crate::dsp::params::Snapshot;
use egui::{Color32, RichText, Ui};
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::time::Duration;
use theme::{BAD, OFF, ON, WARN};
use toast::Toasts;

/// How long the clip lamp stays lit after a clip.
const CLIP_HOLD_S: f64 = 1.5;

pub struct VoxApp {
    pub cfg: Config,
    shared: Arc<Shared>,
    manager: ManagerHandle,
    status: DeviceStatus,
    toasts: Toasts,
    clip_seen: u32,
    clip_until: f64,
    defaults_applied: bool,
}

/// Pushes the non-device parts of the config into the shared atomics.
fn apply_to_shared(cfg: &Config, shared: &Shared) {
    shared.voice_on.store(cfg.voice_on, Relaxed);
    shared.monitor_volume.set(cfg.monitor_volume);
    shared.monitor_voice.store(cfg.monitor_voice, Relaxed);
    shared.monitor_sounds.store(cfg.monitor_sounds, Relaxed);
    shared.params.store(&Snapshot::from_settings(&cfg.effects));
}

impl VoxApp {
    pub fn new(cfg: Config) -> Self {
        let shared = Arc::new(Shared::default());
        let manager = ManagerHandle::spawn(shared.clone(), cfg.devices.clone(), cfg.mic.clone());
        Self::with_manager(cfg, shared, manager)
    }

    fn with_manager(cfg: Config, shared: Arc<Shared>, manager: ManagerHandle) -> Self {
        apply_to_shared(&cfg, &shared);
        Self {
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
        if d.output.is_empty() {
            if let Some(cable) = &self.status.cable {
                d.output = cable.clone();
                changed = true;
            }
        }
        if d.primary.is_empty() && d.fallback.is_empty() {
            if let Some(mic) = &self.status.default_input {
                d.primary = mic.clone();
                changed = true;
            }
        }
        if changed {
            self.manager.send(ManagerCmd::Devices(d.clone()));
        }
    }

    pub fn draw(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.status = self.manager.status();
        self.first_run_defaults();
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
                        self.manager.send(ManagerCmd::Devices(self.cfg.devices.clone()));
                    }
                    if ch.mic {
                        self.manager.send(ManagerCmd::Mic(self.cfg.mic.clone()));
                    }
                    if ch.monitor {
                        apply_to_shared(&self.cfg, &self.shared);
                    }
                });
            });
        egui::CentralPanel::default().show(ui, |ui| {
            ui.label("Effects and soundboard follow.");
        });

        self.toasts.show(&ctx);
        ctx.request_repaint_after(Duration::from_millis(33));
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
            let button = egui::Button::new(RichText::new(text).size(20.0).strong().color(Color32::WHITE))
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
            ActiveMic::Primary if s.reason.is_empty() => (format!("Active: Primary mic - {}", device(0)), ON),
            ActiveMic::Primary => (format!("Active: Primary mic ({}) - {}", s.reason, device(0)), ON),
            ActiveMic::Fallback => (format!("Active: Fallback mic ({}) - {}", s.reason, device(1)), WARN),
            ActiveMic::None if !s.scanned => ("Starting audio...".to_string(), OFF),
            ActiveMic::None if s.output.device.is_none() => {
                ("No virtual mic output selected - nothing is being sent".to_string(), BAD)
            }
            ActiveMic::None => ("No microphone available - sending silence, still looking...".to_string(), BAD),
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
            meters::level_bar(ui, self.shared.input_level.get(), 180.0, None);
            ui.label("Out");
            meters::level_bar(ui, self.shared.output_level.get(), 180.0, None);
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
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::audio::device_manager::{Endpoint, LinkState};

    pub fn fake_status() -> DeviceStatus {
        DeviceStatus {
            scanned: true,
            inputs: vec!["Microphone (HyperX Cloud II Wireless)".into(), "Microphone (USB PnP Audio Device)".into()],
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
        VoxApp::with_manager(Config::default(), shared, ManagerHandle::detached(fake_status()))
    }

    /// Runs one real egui frame headlessly (layout, painting and input handling).
    pub fn frame(ctx: &egui::Context, app: &mut VoxApp, size: (f32, f32), t: f64, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(size.0, size.1))),
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
        for (i, size) in [(920.0, 600.0), (1240.0, 800.0), (1920.0, 1080.0), (3840.0, 2160.0)]
            .into_iter()
            .enumerate()
        {
            frame(&ctx, &mut app, size, i as f64 * 0.1, vec![]);
        }
        assert_eq!(app.cfg.devices.primary, "Microphone (USB PnP Audio Device)");
        assert!(app.cfg.devices.output.is_empty(), "no CABLE: output stays unselected");
    }
}
