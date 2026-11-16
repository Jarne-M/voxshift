//! The Audio setup page, top to bottom in the order the sound travels: the signal path at a
//! glance (Mic → VoxShift → Virtual mic → other apps, each with a status), the virtual mic,
//! the microphones with live meters, the monitor and the latency.

use super::icons;
use super::meters::{MIC_FLOOR_DB, level_meter};
use super::theme::{RADIUS_M, font, mix, palette, space};
use super::widgets::{self, Slider, Tone, card, section_title};
use crate::audio::device_manager::{DeviceStatus, Endpoint, LatencyParts, LinkState};
use crate::audio::matching::best_match;
use crate::audio::mic_select::MicMode;
use crate::audio::shared::{ActiveMic, Shared, to_db};
use crate::config::Config;
use egui::{Align, Frame, Layout, Margin, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use std::sync::atomic::Ordering::Relaxed;

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

/// What the page shows besides the settings it edits.
pub struct View<'a> {
    pub status: &'a DeviceStatus,
    pub shared: &'a Shared,
    /// The loaded preset, for the "VoxShift" step.
    pub preset: Option<&'a str>,
    pub melody_on: bool,
}

// ------------------------------------------------------------------------------------------
// The signal path
// ------------------------------------------------------------------------------------------

/// One step of the signal path: its status and a line about it.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub tone: Tone,
    pub line: String,
}

/// The status of the four steps: mic, VoxShift, virtual mic, other apps.
pub fn path_steps(cfg: &Config, v: &View) -> [Step; 4] {
    let step = |tone, line: String| Step { tone, line };
    let s = v.status;
    let short = |name: &str| {
        // "Microphone (HyperX Cloud II Wireless)" -> "HyperX Cloud II Wireless"
        match (name.find('('), name.rfind(')')) {
            (Some(a), Some(b)) if b > a + 1 => name[a + 1..b].to_string(),
            _ => name.to_string(),
        }
    };
    let mic_name = |slot: usize| {
        s.mics[slot]
            .device
            .as_deref()
            .map(short)
            .unwrap_or_else(|| "?".into())
    };
    let mic = if v.shared.mic_muted.load(Relaxed) {
        step(Tone::Warning, "Muted: others hear silence".into())
    } else {
        match s.active {
            ActiveMic::Primary => step(Tone::Success, format!("Primary: {}", mic_name(0))),
            ActiveMic::Fallback => step(
                Tone::Warning,
                format!("Fallback: {} ({})", mic_name(1), s.reason),
            ),
            ActiveMic::None if !s.scanned => step(Tone::Neutral, "Looking for mics\u{2026}".into()),
            ActiveMic::None => step(Tone::Danger, format!("No mic: {}", s.reason)),
        }
    };
    let voxshift = if v.shared.safe_mode.load(Relaxed) {
        step(
            Tone::Danger,
            "Safe mode: clean voice, no clips (press Restore)".into(),
        )
    } else if v.shared.voice_on.load(Relaxed) {
        let what = match (v.preset, v.melody_on) {
            (Some(p), true) => format!("Effects on \u{b7} {p} \u{b7} Melody Voice"),
            (Some(p), false) => format!("Effects on \u{b7} {p}"),
            (None, true) => "Effects on \u{b7} Melody Voice".into(),
            (None, false) => "Effects on".into(),
        };
        step(Tone::Success, what)
    } else {
        step(Tone::Success, "Effects off: your clean voice".into())
    };
    let is_cable = s
        .output
        .device
        .as_deref()
        .is_some_and(|d| s.cable.as_deref() == Some(d));
    let out_name = s.output.device.as_deref().map(short).unwrap_or_default();
    let (virtual_mic, apps) = match (&s.output.state, is_cable) {
        (LinkState::Running, true) => (
            step(Tone::Success, format!("Playing into {out_name}")),
            step(
                Tone::Success,
                "Pick \u{201c}CABLE Output\u{201d} as the mic in Discord, OBS or a game".into(),
            ),
        ),
        (LinkState::Running, false) => (
            step(
                Tone::Warning,
                format!("Playing into {out_name}: not VB-CABLE"),
            ),
            step(
                Tone::Warning,
                "Apps can only use VoxShift as a mic through VB-CABLE".into(),
            ),
        ),
        (LinkState::Off, _) if cfg.devices.output.is_empty() => (
            step(Tone::Danger, "No output chosen".into()),
            step(Tone::Danger, "Nothing reaches other apps yet".into()),
        ),
        (state, _) => (
            step(Tone::Danger, link_text(state, &s.output).0),
            step(Tone::Danger, "Nothing reaches other apps right now".into()),
        ),
    };
    [mic, voxshift, virtual_mic, apps]
}

const STEP_TITLES: [(&str, &str); 4] = [
    (icons::MIC, "Your mic"),
    (icons::LOGO, "VoxShift"),
    (icons::VIRTUAL_MIC, "Virtual mic"),
    (icons::USERS, "Other apps"),
];

fn step_card(ui: &mut Ui, index: usize, s: &Step, width: f32) {
    let p = palette(ui);
    let (icon, title) = STEP_TITLES[index];
    let tone = s.tone.color(p);
    Frame::new()
        .fill(mix(p.surface, tone, 0.06))
        .stroke(Stroke::new(1.0, mix(p.border, tone, 0.35)))
        .corner_radius(RADIUS_M)
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(width - 24.0);
            ui.set_min_height(58.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::S;
                ui.label(RichText::new(icon).size(17.0).color(p.text_muted));
                ui.label(RichText::new(title).strong().color(p.text));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    widgets::status_dot(ui, s.tone, 10.0);
                });
            });
            ui.add(egui::Label::new(RichText::new(&s.line).small().color(p.text_muted)).wrap());
        });
    #[cfg(test)]
    widgets::remember(ui.ctx(), "path_step", index, ui.min_rect());
}

/// Mic → VoxShift → Virtual mic → other apps, side by side (stacked when narrow).
fn signal_path(ui: &mut Ui, steps: &[Step; 4]) {
    let p = palette(ui);
    let arrow = 22.0;
    let available = ui.available_width();
    if available < 600.0 {
        for (i, s) in steps.iter().enumerate() {
            step_card(ui, i, s, available);
        }
        return;
    }
    let width = (available - 3.0 * arrow) / 4.0;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (i, s) in steps.iter().enumerate() {
            if i > 0 {
                let (rect, _) = ui.allocate_exact_size(vec2(arrow, 58.0), Sense::hover());
                // (egui's default font has no arrow glyph: draw it.)
                let c = rect.center();
                let stroke = Stroke::new(1.5, p.text_faint);
                ui.painter()
                    .line_segment([c - vec2(6.0, 0.0), c + vec2(5.0, 0.0)], stroke);
                ui.painter()
                    .line_segment([c + vec2(1.0, -4.0), c + vec2(5.0, 0.0)], stroke);
                ui.painter()
                    .line_segment([c + vec2(1.0, 4.0), c + vec2(5.0, 0.0)], stroke);
            }
            ui.vertical(|ui| step_card(ui, i, s, width));
        }
    });
}

// ------------------------------------------------------------------------------------------
// Devices
// ------------------------------------------------------------------------------------------

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
        .width(ui.available_width().min(380.0))
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

/// What a device's state means, and its tone.
fn link_text(state: &LinkState, ep: &Endpoint) -> (String, Tone) {
    match state {
        LinkState::Off => ("Not selected".into(), Tone::Neutral),
        LinkState::Missing => ("Not connected: waiting for it".into(), Tone::Warning),
        LinkState::Waiting => (
            "Connected: starts once the virtual mic runs".into(),
            Tone::Neutral,
        ),
        LinkState::Running => {
            let ms = ep.period as f32 * 1000.0 / ep.rate.max(1) as f32;
            (
                format!("Running \u{b7} {} Hz \u{b7} {ms:.1} ms buffer", ep.rate),
                Tone::Success,
            )
        }
        LinkState::Silent => (
            "Connected but silent (mic unplugged?)".into(),
            Tone::Warning,
        ),
        LinkState::Error(e) => (format!("Error: {e}. Retrying."), Tone::Danger),
    }
}

fn state_line(ui: &mut Ui, ep: &Endpoint) {
    let p = palette(ui);
    let (text, tone) = link_text(&ep.state, ep);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        widgets::status_dot(ui, tone, 8.0);
        ui.add(egui::Label::new(RichText::new(text).small().color(p.text_muted)).wrap());
    });
}

fn cable_banner(ui: &mut Ui) {
    let p = palette(ui);
    Frame::new()
        .fill(mix(p.surface, p.warning, 0.10))
        .stroke(Stroke::new(1.0, mix(p.border, p.warning, 0.5)))
        .corner_radius(RADIUS_M)
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(icons::WARNING).size(18.0).color(p.warning));
                ui.label(
                    RichText::new("VB-CABLE not found")
                        .size(font::SECTION)
                        .strong()
                        .color(p.text),
                );
            });
            ui.label(
                "VoxShift plays your changed voice into a virtual audio cable, which Discord, \
                 OBS and games then use as their microphone. Install the free VB-CABLE driver, \
                 restart Windows, and VoxShift picks \u{201c}CABLE Input\u{201d} by itself.",
            );
            ui.hyperlink_to("Download VB-CABLE (vb-audio.com)", VB_CABLE_URL);
            widgets::hint(ui, "Until then you can pick any output device below.");
        });
}

fn virtual_mic_card(ui: &mut Ui, cfg: &mut Config, s: &DeviceStatus, ch: &mut Changed) {
    card(ui, |ui| {
        ui.horizontal(|ui| {
            section_title(ui, icons::VIRTUAL_MIC, "Virtual mic");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ch.rescan |= ui
                    .button(format!("{}  Rescan", icons::RESTORE))
                    .on_hover_text("Devices are rescanned every 1.5 s anyway; this does it now")
                    .clicked();
            });
        });
        widgets::hint(
            ui,
            "Normally \u{201c}CABLE Input\u{201d}. Other apps then use \u{201c}CABLE \
             Output\u{201d} as their mic.",
        );
        ch.devices |= device_combo(ui, "out_dev", &mut cfg.devices.output, &s.outputs, "(none)");
        state_line(ui, &s.output);
    });
}

/// One mic: its device, a live meter with its dB value (and, for the primary, the silence
/// threshold drawn on it) and its state.
fn mic_row(ui: &mut Ui, cfg: &mut Config, v: &View, slot: usize, ch: &mut Changed) {
    let p = palette(ui);
    let active = v.status.active.slot() == Some(slot);
    ui.horizontal(|ui| {
        let label = if slot == 0 {
            "Primary mic"
        } else {
            "Fallback mic"
        };
        ui.label(RichText::new(label).strong().color(p.text));
        if active {
            widgets::badge(ui, "IN USE", Tone::Success);
        }
    });
    let name = if slot == 0 {
        &mut cfg.devices.primary
    } else {
        &mut cfg.devices.fallback
    };
    ch.devices |= device_combo(ui, &format!("mic{slot}"), name, &v.status.inputs, "(none)");
    ui.horizontal(|ui| {
        let level = v.shared.mic_level[slot].get();
        let marker = (slot == 0 && cfg.mic.silence_detect).then_some(cfg.mic.silence_db);
        let width = (ui.available_width() - 70.0).clamp(120.0, 380.0);
        let r = level_meter(ui, level, vec2(width, 12.0), MIC_FLOOR_DB, marker);
        if marker.is_some() {
            r.on_hover_text(format!(
                "The line is the silence threshold ({:.0} dB)",
                cfg.mic.silence_db
            ));
        }
        let db = to_db(level).max(MIC_FLOOR_DB);
        ui.label(
            RichText::new(format!("{db:>4.0} dB"))
                .monospace()
                .color(p.text_muted),
        );
    });
    state_line(ui, &v.status.mics[slot]);
}

fn mics_card(ui: &mut Ui, cfg: &mut Config, v: &View, ch: &mut Changed) {
    card(ui, |ui| {
        section_title(ui, icons::MIC, "Microphones");
        widgets::hint(
            ui,
            "VoxShift uses the primary mic, and the fallback when the primary is gone.",
        );
        ui.add_space(space::XS);
        mic_row(ui, cfg, v, 0, ch);
        ui.add_space(space::M);
        mic_row(ui, cfg, v, 1, ch);
        ui.add_space(space::S);
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Use");
            ch.mic |= widgets::segmented(
                ui,
                &mut cfg.mic.mode,
                &[
                    (MicMode::Auto, "Automatic"),
                    (MicMode::ForcePrimary, "Always primary"),
                    (MicMode::ForceFallback, "Always fallback"),
                ],
            );
        });
        let m = &mut cfg.mic;
        ui.horizontal(|ui| {
            ch.mic |= widgets::toggle(ui, &mut m.switch_back).changed();
            ui.label("Switch back to the primary when it works again");
        });
        ui.horizontal(|ui| {
            ch.mic |= widgets::toggle(ui, &mut m.silence_detect).changed();
            ui.label("Treat a silent primary as unplugged")
                .on_hover_text(
                    "Headsets with a detachable mic often stay listed when the mic is pulled out, \
                 and just deliver silence. With this on, a primary that stays below the \
                 threshold counts as unavailable.",
                );
        });
        if m.silence_detect {
            ch.mic |= Slider::new(&mut m.silence_db, -100.0..=-30.0)
                .label("Threshold")
                .unit("dB")
                .step(1.0)
                .default_value(-85.0)
                .tip(
                    "Check the primary's dB value while it is plugged in and you are quiet: the \
                     threshold (the line on its meter) must be well below it, but above the \
                     value with the mic pulled out.",
                )
                .show(ui)
                .changed();
            ch.mic |= Slider::new(&mut m.silence_secs, 1.0..=60.0)
                .label("After")
                .unit("s")
                .decimals(1)
                .step(0.5)
                .logarithmic(true)
                .default_value(5.0)
                .tip("How long the primary must stay below the threshold")
                .show(ui)
                .changed();
        }
    });
}

fn monitor_card(ui: &mut Ui, cfg: &mut Config, s: &DeviceStatus, ch: &mut Changed) {
    card(ui, |ui| {
        ui.horizontal(|ui| {
            section_title(ui, icons::HEADPHONES, "Monitor");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let r = widgets::toggle(ui, &mut cfg.devices.monitor_enabled);
                #[cfg(test)]
                widgets::remember(ui.ctx(), "audio", "monitor", r.rect);
                if r.changed() {
                    ch.devices = true;
                    // Switched on with nothing to hear: hear yourself.
                    if cfg.devices.monitor_enabled && !cfg.monitor_voice && !cfg.monitor_sounds {
                        cfg.monitor_voice = true;
                        ch.monitor = true;
                    }
                }
            });
        });
        widgets::hint(
            ui,
            "Hear the result in your headphones. It never goes into the virtual mic.",
        );
        if !cfg.devices.monitor_enabled {
            return;
        }
        ch.devices |= device_combo(
            ui,
            "mon_dev",
            &mut cfg.devices.monitor,
            &s.outputs,
            "(none)",
        );
        state_line(ui, &s.monitor);
        ch.monitor |= Slider::new(&mut cfg.monitor_volume, 0.0..=1.0)
            .label("Volume")
            .percent(true)
            .default_value(0.8)
            .tip("How loud the monitor is in your headphones")
            .show(ui)
            .changed();
        ui.horizontal(|ui| {
            ui.label("Play");
            ch.monitor |= ui
                .checkbox(&mut cfg.monitor_voice, "my voice")
                .on_hover_text("Hear yourself as others hear you")
                .changed();
            ch.monitor |= ui
                .checkbox(&mut cfg.monitor_sounds, "soundboard")
                .on_hover_text("Hear the clips you play")
                .changed();
        });
        if !cfg.monitor_voice && !cfg.monitor_sounds {
            widgets::notice(
                ui,
                icons::WARNING,
                "You won't hear anything: tick \u{201c}my voice\u{201d} or \
                 \u{201c}soundboard\u{201d} (only the switch beeps play)",
                Tone::Warning,
            );
        }
    });
}

// ------------------------------------------------------------------------------------------
// Latency
// ------------------------------------------------------------------------------------------

/// One part of the latency bar: its name, ms (`None` = can't be measured) and colour.
pub fn latency_segments(parts: &LatencyParts, melody_on: bool) -> Vec<(&'static str, Option<f32>)> {
    let chain = if melody_on { "Melody Voice" } else { "Effects" };
    vec![
        ("Windows buffers", Some(parts.windows)),
        ("VoxShift buffering", Some(parts.buffering)),
        (chain, Some(parts.chain)),
        ("VB-CABLE", None),
    ]
}

fn latency_bar(ui: &mut Ui, parts: &LatencyParts, melody_on: bool) {
    let p = palette(ui);
    let segments = latency_segments(parts, melody_on);
    let colors = [
        mix(p.surface, p.accent, 0.45),
        mix(p.surface, p.accent, 0.75),
        if melody_on { p.warning } else { p.accent },
        p.border_strong,
    ];
    // The unmeasured part gets a fixed small width (and no number).
    let unknown_w = 36.0;
    let total = parts.total().max(1.0);
    let width = ui.available_width().min(560.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 16.0), Sense::hover());
    let known_w = width - unknown_w;
    let mut x = rect.left();
    for ((_, ms), color) in segments.iter().zip(colors) {
        let w = match ms {
            Some(ms) => known_w * ms / total,
            None => unknown_w,
        };
        if w < 0.5 {
            continue;
        }
        let seg = Rect::from_min_max(pos2(x, rect.top()), pos2(x + w, rect.bottom()));
        ui.painter()
            .rect_filled(seg.shrink2(vec2(0.5, 0.0)), 3.0, color);
        if ms.is_none() {
            // Hatched: it is there, but its size is unknown.
            let painter = ui.painter().with_clip_rect(seg);
            let mut hx = seg.left() - seg.height();
            while hx < seg.right() {
                painter.line_segment(
                    [pos2(hx, seg.bottom()), pos2(hx + seg.height(), seg.top())],
                    Stroke::new(1.0, p.surface),
                );
                hx += 5.0;
            }
        }
        x += w;
    }
    ui.horizontal_wrapped(|ui| {
        for ((name, ms), color) in segments.iter().zip(colors) {
            let (dot, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
            ui.painter().rect_filled(dot, 2.0, color);
            let value = match ms {
                Some(ms) => format!("{name} {ms:.0} ms"),
                None => format!("{name} (not measurable)"),
            };
            ui.label(RichText::new(value).small().color(p.text_muted));
            ui.add_space(space::S);
        }
    });
}

fn latency_card(ui: &mut Ui, cfg: &mut Config, v: &View, ch: &mut Changed) {
    let p = palette(ui);
    card(ui, |ui| {
        ui.horizontal(|ui| {
            section_title(ui, icons::LATENCY, "Latency");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let label = |n: u32| {
                    if n == 0 {
                        "Device default".to_string()
                    } else {
                        format!("{n} frames")
                    }
                };
                let d = &mut cfg.devices;
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
                    })
                    .response
                    .on_hover_text(
                        "Smaller buffers are faster but may crackle on a busy PC. Windows \
                         shared mode often uses 10 ms whatever you pick.",
                    );
                ui.label(RichText::new("Buffer").color(p.text_muted));
            });
        });
        match (v.status.latency_ms, &v.status.latency_parts) {
            (Some(ms), Some(parts)) => {
                let tone = if ms <= 30.0 {
                    Tone::Success
                } else {
                    Tone::Warning
                };
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{ms:.0} ms"))
                            .size(font::TITLE)
                            .strong()
                            .color(tone.color(p)),
                    );
                    widgets::hint(ui, "from your mic to the virtual mic, plus VB-CABLE's own");
                });
                latency_bar(ui, parts, v.melody_on);
            }
            _ => {
                widgets::hint(ui, "Shown once a mic and the virtual mic are running.");
            }
        }
    });
}

// ------------------------------------------------------------------------------------------
// The page
// ------------------------------------------------------------------------------------------

pub fn page(ui: &mut Ui, cfg: &mut Config, v: &View) -> Changed {
    let mut ch = Changed::default();
    let steps = path_steps(cfg, v);
    signal_path(ui, &steps);
    ui.add_space(space::M);
    if v.status.scanned && v.status.cable.is_none() {
        cable_banner(ui);
        ui.add_space(space::M);
    }
    virtual_mic_card(ui, cfg, v.status, &mut ch);
    ui.add_space(space::M);
    mics_card(ui, cfg, v, &mut ch);
    ui.add_space(space::M);
    monitor_card(ui, cfg, v.status, &mut ch);
    ui.add_space(space::M);
    latency_card(ui, cfg, v, &mut ch);
    ch
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::device_manager::Endpoint;

    fn running(name: &str) -> Endpoint {
        Endpoint {
            device: Some(name.into()),
            state: LinkState::Running,
            rate: 48_000,
            period: 480,
        }
    }

    #[test]
    fn the_signal_path_says_what_works_and_what_does_not() {
        let shared = Shared::default();
        let cable = "CABLE Input (VB-Audio Virtual Cable)";
        let mut status = DeviceStatus {
            scanned: true,
            cable: Some(cable.into()),
            output: running(cable),
            mics: [running("Microphone (HyperX)"), Endpoint::default()],
            active: ActiveMic::Primary,
            ..Default::default()
        };
        let cfg = Config::default();
        let tones = |status: &DeviceStatus, shared: &Shared| {
            let v = View {
                status,
                shared,
                preset: Some("Cave"),
                melody_on: false,
            };
            path_steps(&cfg, &v)
        };
        let s = tones(&status, &shared);
        assert!(s.iter().all(|s| s.tone == Tone::Success), "{s:?}");
        assert_eq!(s[0].line, "Primary: HyperX");
        assert!(s[1].line.contains("Cave"));

        status.active = ActiveMic::Fallback;
        status.reason = "primary mic is silent".into();
        assert_eq!(tones(&status, &shared)[0].tone, Tone::Warning);
        status.active = ActiveMic::None;
        assert_eq!(tones(&status, &shared)[0].tone, Tone::Danger);

        // Into the speakers instead of VB-CABLE: works, but no app can use it as a mic.
        status.output = running("Speakers (Realtek)");
        let s = tones(&status, &shared);
        assert_eq!((s[2].tone, s[3].tone), (Tone::Warning, Tone::Warning));
        status.output.state = LinkState::Missing;
        assert_eq!(tones(&status, &shared)[3].tone, Tone::Danger);

        shared.safe_mode.store(true, Relaxed);
        assert_eq!(tones(&status, &shared)[1].tone, Tone::Danger);
    }

    #[test]
    fn switching_the_monitor_on_with_nothing_ticked_turns_my_voice_on() {
        let ctx = egui::Context::default();
        let mut cfg = Config {
            monitor_voice: false,
            monitor_sounds: false,
            ..Config::default()
        };
        cfg.devices.monitor_enabled = false;
        let status = DeviceStatus::default();
        let mut changed = Changed::default();
        let mut run = |cfg: &mut Config, events: Vec<egui::Event>, t: f64| {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(700.0, 500.0))),
                time: Some(t),
                events,
                ..Default::default()
            };
            ctx.run_ui(input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    monitor_card(ui, cfg, &status, &mut changed);
                });
            })
            .textures_delta
            .clear();
        };
        run(&mut cfg, vec![], 0.0);
        let at = widgets::rect_of(&ctx, "audio", "monitor").center();
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        run(&mut cfg, vec![egui::Event::PointerMoved(at)], 0.1);
        run(&mut cfg, vec![press(true)], 0.2);
        run(&mut cfg, vec![press(false)], 0.3);
        assert!(cfg.devices.monitor_enabled, "switched on");
        assert!(cfg.monitor_voice, "and you hear yourself");
        assert!(changed.devices && changed.monitor);
    }

    #[test]
    fn the_latency_parts_name_what_the_chain_is() {
        let parts = LatencyParts {
            windows: 20.0,
            buffering: 6.0,
            chain: 15.0,
        };
        assert_eq!(parts.total(), 41.0);
        let s = latency_segments(&parts, true);
        assert_eq!(s[2], ("Melody Voice", Some(15.0)));
        assert_eq!(s[3], ("VB-CABLE", None), "never an invented number");
        assert_eq!(latency_segments(&parts, false)[2].0, "Effects");
    }
}
