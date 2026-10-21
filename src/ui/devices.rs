//! Device selection: virtual mic, the two mics with fallback, monitor and buffer size.

use super::meters::{MIC_FLOOR_DB, level_bar};
use super::theme::{BAD, OFF, ON, WARN, card};
use crate::audio::device_manager::{DeviceStatus, Endpoint, LinkState};
use crate::audio::matching::best_match;
use crate::audio::mic_select::MicMode;
use crate::audio::shared::{Shared, to_db};
use crate::config::Config;
use egui::{RichText, Ui};

pub const VB_CABLE_URL: &str = "https://vb-audio.com/Cable/";
pub const BUFFER_CHOICES: [u32; 6] = [0, 128, 256, 480, 512, 1024];

/// What the user changed this frame.
#[derive(Default)]
pub struct Changed {
    pub devices: bool,
    pub mic: bool,
    pub monitor: bool,
    pub rescan: bool,
}

/// Dropdown of device names. Keeps showing a configured device that is not connected right now
/// (the setting is kept for when it comes back).
fn device_combo(
    ui: &mut Ui,
    id: &str,
    current: &mut String,
    options: &[String],
    none_label: &str,
) -> bool {
    let connected = best_match(current, options).is_some();
    let shown = if current.is_empty() {
        none_label.to_string()
    } else if connected {
        current.clone()
    } else {
        format!("{current} (not connected)")
    };
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .width(ui.available_width().min(320.0))
        .selected_text(shown)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(current.is_empty(), none_label)
                .clicked()
            {
                changed |= !current.is_empty();
                current.clear();
            }
            for name in options {
                if ui.selectable_label(current == name, name).clicked() && current != name {
                    *current = name.clone();
                    changed = true;
                }
            }
        });
    changed
}

fn state_line(ui: &mut Ui, ep: &Endpoint) {
    let (text, color) = match &ep.state {
        LinkState::Off => ("not selected".to_string(), OFF),
        LinkState::Missing => ("not connected - waiting for it".to_string(), WARN),
        LinkState::Waiting => (
            "connected - starts once the virtual mic output runs".to_string(),
            OFF,
        ),
        LinkState::Running => {
            let ms = ep.period as f32 * 1000.0 / ep.rate.max(1) as f32;
            (format!("running, {} Hz, {:.1} ms buffer", ep.rate, ms), ON)
        }
        LinkState::Silent => ("connected but silent (mic unplugged?)".to_string(), WARN),
        LinkState::Error(e) => (format!("error: {e} - retrying"), BAD),
    };
    ui.label(RichText::new(text).small().color(color));
}

pub fn cable_banner(ui: &mut Ui) {
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(70, 50, 20))
        .stroke(egui::Stroke::new(1.0, WARN))
        .corner_radius(6)
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.label(RichText::new("VB-CABLE not found").strong().color(WARN));
            ui.label(
                "VoxShift plays your changed voice into a virtual audio cable, which Discord, \
                 OBS and games then use as their microphone. Install the free VB-CABLE driver, \
                 reboot, and VoxShift will pick \"CABLE Input\" automatically.",
            );
            ui.hyperlink_to("Download VB-CABLE (vb-audio.com)", VB_CABLE_URL);
            ui.label(RichText::new("Until then you can pick any output device below.").small());
        });
}

pub fn panel(ui: &mut Ui, cfg: &mut Config, status: &DeviceStatus, shared: &Shared) -> Changed {
    let mut ch = Changed::default();
    let d = &mut cfg.devices;

    if status.scanned && status.cable.is_none() {
        cable_banner(ui);
        ui.add_space(6.0);
    }

    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.heading("Virtual mic (output)");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ch.rescan |= ui
                    .small_button("Rescan")
                    .on_hover_text("Devices are rescanned every 1.5 s anyway; this does it now")
                    .clicked();
            });
        });
        ui.label(
            RichText::new("Normally \"CABLE Input\". Apps then use \"CABLE Output\" as their mic.")
                .small(),
        );
        ch.devices |= device_combo(ui, "out_dev", &mut d.output, &status.outputs, "(none)");
        state_line(ui, &status.output);
    });
    ui.add_space(6.0);

    card(ui, |ui| {
        ui.heading("Microphones");
        for (slot, label) in [(0, "Primary mic"), (1, "Fallback mic")] {
            ui.label(RichText::new(label).strong());
            let name = if slot == 0 {
                &mut d.primary
            } else {
                &mut d.fallback
            };
            ch.devices |= device_combo(ui, label, name, &status.inputs, "(none)");
            ui.horizontal(|ui| {
                let level = shared.mic_level[slot].get();
                let marker = (slot == 0 && cfg.mic.silence_detect).then_some(cfg.mic.silence_db);
                level_bar(ui, level, 140.0, MIC_FLOOR_DB, marker);
                ui.label(
                    RichText::new(format!("{:>4.0} dB", to_db(level)))
                        .monospace()
                        .small(),
                );
            });
            // On its own line so long messages wrap instead of widening the panel.
            state_line(ui, &status.mics[slot]);
            ui.add_space(4.0);
        }
        ui.separator();
        ui.horizontal(|ui| {
            for (mode, label) in [
                (MicMode::Auto, "Auto"),
                (MicMode::ForcePrimary, "Force primary"),
                (MicMode::ForceFallback, "Force fallback"),
            ] {
                ch.mic |= ui.radio_value(&mut cfg.mic.mode, mode, label).changed();
            }
        });
        ch.mic |= ui
            .checkbox(
                &mut cfg.mic.switch_back,
                "Switch back to the primary automatically",
            )
            .changed();
        ch.mic |= ui
            .checkbox(
                &mut cfg.mic.silence_detect,
                "Treat a silent primary as unplugged",
            )
            .on_hover_text(
                "Headsets with a detachable mic often stay listed when the mic is pulled out, \
                 and just deliver silence. With this on, a primary that stays below the \
                 threshold counts as unavailable.",
            )
            .changed();
        ui.add_enabled_ui(cfg.mic.silence_detect, |ui| {
            ui.horizontal(|ui| {
                ui.label("after");
                ch.mic |= ui
                    .add(
                        egui::DragValue::new(&mut cfg.mic.silence_secs)
                            .range(1.0..=60.0)
                            .suffix(" s")
                            .speed(0.1),
                    )
                    .changed();
                ui.label("below");
                ch.mic |= ui
                    .add(
                        egui::DragValue::new(&mut cfg.mic.silence_db)
                            .range(-100.0..=-30.0)
                            .suffix(" dB")
                            .speed(0.5),
                    )
                    .changed();
            });
            ui.label(
                RichText::new(
                    "Check the primary's dB value while it is plugged in and you are quiet: the \
                     threshold must be well below it, but above the value with the mic pulled out.",
                )
                .small(),
            );
        });
    });
    ui.add_space(6.0);

    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.heading("Monitor");
            ch.devices |= ui.checkbox(&mut d.monitor_enabled, "on").changed();
        });
        ui.label(RichText::new("Hear the result on your headphones.").small());
        ch.devices |= device_combo(ui, "mon_dev", &mut d.monitor, &status.outputs, "(none)");
        state_line(ui, &status.monitor);
        ch.monitor |= ui
            .add(egui::Slider::new(&mut cfg.monitor_volume, 0.0..=1.0).text("volume"))
            .changed();
        ui.horizontal(|ui| {
            ch.monitor |= ui.checkbox(&mut cfg.monitor_voice, "my voice").changed();
            ch.monitor |= ui.checkbox(&mut cfg.monitor_sounds, "soundboard").changed();
        });
    });
    ui.add_space(6.0);

    card(ui, |ui| {
        ui.heading("Latency");
        ui.horizontal(|ui| {
            ui.label("Buffer");
            let label = |n: u32| {
                if n == 0 {
                    "device default".to_string()
                } else {
                    format!("{n} frames")
                }
            };
            egui::ComboBox::from_id_salt("buffer")
                .selected_text(label(d.buffer_frames))
                .show_ui(ui, |ui| {
                    for n in BUFFER_CHOICES {
                        if ui
                            .selectable_label(d.buffer_frames == n, label(n))
                            .clicked()
                            && d.buffer_frames != n
                        {
                            d.buffer_frames = n;
                            ch.devices = true;
                        }
                    }
                });
        });
        match status.latency_ms {
            Some(ms) => {
                let color = if ms <= 30.0 { ON } else { WARN };
                ui.label(
                    RichText::new(format!("Estimated mic -> virtual mic: {ms:.0} ms")).color(color),
                );
            }
            None => {
                ui.label("Estimated latency: - (no mic running)");
            }
        }
        ui.label(
            RichText::new("Windows shared mode usually fixes buffers at 10 ms; VB-CABLE adds its own buffer on top.")
                .small(),
        );
    });
    ch
}
