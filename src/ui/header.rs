//! The header: always visible, and where the app's state is read at a glance.
//!
//! Left: the voice changer as one big pill that is clearly a switch (filled accent "ON" with
//! the preset, outlined "OFF · clean voice"). Middle: the active mic, the In / Out meters,
//! the CLIP lamp and the latency, followed by a chip for every problem (each one jumps to its
//! fix). Right: Mute and PANIC. In safe mode the whole header turns into a red banner with
//! Restore.
//!
//! Drawing only reads a [`View`]; what the user clicked comes back as [`Action`]s.

use super::icons;
use super::meters::{self, PROGRAM_FLOOR_DB};
use super::theme::{Palette, font, mix, palette, space};
use super::widgets::{self, Tone};
use crate::audio::device_manager::DeviceStatus;
use crate::audio::shared::ActiveMic;
use crate::config::Page;
use egui::{
    Align, Color32, CornerRadius, CursorIcon, FontId, Layout, Response, RichText, Sense, Stroke,
    StrokeKind, Ui, pos2, vec2,
};

pub const HEIGHT: f32 = 64.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    ToggleVoice,
    ToggleMute,
    /// Safe mode on.
    Panic,
    /// Safe mode off.
    Restore,
    Goto(Page),
}

pub struct View<'a> {
    pub voice_on: bool,
    pub safe: bool,
    pub muted: bool,
    /// The loaded preset and whether the sliders moved since.
    pub preset: Option<&'a str>,
    pub modified: bool,
    pub status: &'a DeviceStatus,
    pub input_level: f32,
    pub output_level: f32,
    pub clipping: bool,
    pub hotkeys_available: bool,
    pub panic_mutes_monitor: bool,
    /// The hotkeys of the three switches, for their tooltips.
    pub toggle_key: Option<String>,
    pub mute_key: Option<String>,
    pub panic_key: Option<String>,
}

/// Something that keeps VoxShift from doing its job, shown as a chip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    /// No virtual mic output is running: nothing reaches other apps.
    NoOutput,
    /// VB-CABLE is not installed (another output may still be in use).
    CableMissing,
    NoMic,
    /// The fallback mic is in use.
    Fallback,
    Muted,
    /// The global key listener could not start.
    NoHotkeys,
}

impl Problem {
    pub fn text(self) -> &'static str {
        match self {
            Problem::NoOutput => "No virtual mic output",
            Problem::CableMissing => "VB-CABLE not found",
            Problem::NoMic => "No microphone",
            Problem::Fallback => "Using fallback mic",
            Problem::Muted => "Mic muted",
            Problem::NoHotkeys => "Hotkeys unavailable",
        }
    }

    fn tone(self) -> Tone {
        match self {
            Problem::NoOutput | Problem::NoMic | Problem::Muted => Tone::Danger,
            Problem::CableMissing | Problem::Fallback | Problem::NoHotkeys => Tone::Warning,
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Problem::NoOutput | Problem::CableMissing => icons::VIRTUAL_MIC,
            Problem::NoMic | Problem::Fallback => icons::MIC,
            Problem::Muted => icons::MIC_OFF,
            Problem::NoHotkeys => icons::KEYBOARD,
        }
    }

    fn tip(self, reason: &str) -> String {
        match self {
            Problem::NoOutput => {
                "Nothing is being sent to other apps. Click to pick the virtual mic output."
                    .to_string()
            }
            Problem::CableMissing => {
                "Other apps listen to VoxShift through the free VB-CABLE driver. Click for \
                 the download link."
                    .to_string()
            }
            Problem::NoMic => {
                "No microphone is available, so silence is sent. Click to pick one.".to_string()
            }
            Problem::Fallback if reason.is_empty() => {
                "The fallback mic is in use. Click to see why.".to_string()
            }
            Problem::Fallback => {
                format!("The fallback mic is in use ({reason}). Click to see the mics.")
            }
            Problem::Muted => {
                "Your mic is muted; only the soundboard is sent. Click to unmute.".to_string()
            }
            Problem::NoHotkeys => {
                "Global hotkeys could not be started on this system. Click for the hotkeys."
                    .to_string()
            }
        }
    }

    /// What clicking the chip does: go to the fix.
    fn action(self) -> Action {
        match self {
            Problem::NoOutput | Problem::CableMissing | Problem::NoMic | Problem::Fallback => {
                Action::Goto(Page::Audio)
            }
            Problem::Muted => Action::ToggleMute,
            Problem::NoHotkeys => Action::Goto(Page::Hotkeys),
        }
    }
}

/// The problems to show, the most serious first. None while audio is still starting.
pub fn problems(status: &DeviceStatus, muted: bool, hotkeys_available: bool) -> Vec<Problem> {
    let mut out = Vec::new();
    if status.scanned {
        let output = status.output.device.is_some();
        if !output {
            out.push(Problem::NoOutput);
        }
        if status.cable.is_none() {
            out.push(Problem::CableMissing);
        }
        match status.active {
            ActiveMic::None if output => out.push(Problem::NoMic),
            ActiveMic::Fallback => out.push(Problem::Fallback),
            _ => {}
        }
    }
    if muted {
        out.push(Problem::Muted);
    }
    if !hotkeys_available {
        out.push(Problem::NoHotkeys);
    }
    out
}

/// "Microphone (HyperX Cloud II Wireless)" reads better as "HyperX Cloud II Wireless".
pub fn short_device_name(name: &str) -> &str {
    match (name.find('('), name.rfind(')')) {
        (Some(open), Some(close)) if close > open + 1 => name[open + 1..close].trim(),
        _ => name.trim(),
    }
}

/// The active mic as (name, tone, tooltip).
fn mic_summary(v: &View) -> (String, Tone, String) {
    let s = v.status;
    let device = |slot: usize| s.mics[slot].device.clone().unwrap_or_default();
    let why = |what: &str, slot: usize| {
        if s.reason.is_empty() {
            format!("{what}: {}", device(slot))
        } else {
            format!("{what}: {}\n{}", device(slot), s.reason)
        }
    };
    match s.active {
        ActiveMic::Primary => (
            short_device_name(&device(0)).to_string(),
            Tone::Success,
            why("Primary mic", 0),
        ),
        ActiveMic::Fallback => (
            short_device_name(&device(1)).to_string(),
            Tone::Warning,
            why("Fallback mic", 1),
        ),
        ActiveMic::None if !s.scanned => (
            "Starting audio\u{2026}".to_string(),
            Tone::Neutral,
            "Looking for your audio devices.".to_string(),
        ),
        ActiveMic::None if s.output.device.is_none() => (
            "Nothing is sent".to_string(),
            Tone::Danger,
            "No virtual mic output is running, so nothing is being sent.".to_string(),
        ),
        ActiveMic::None => (
            "No microphone".to_string(),
            Tone::Danger,
            "No microphone is available: silence is sent while VoxShift keeps looking.".to_string(),
        ),
    }
}

/// The voice changer switch: a pill with a switch drawn in it, so it reads as "click me"
/// in both states.
fn voice_pill(ui: &mut Ui, on: bool, text: &str, modified: bool) -> Response {
    let p = palette(ui);
    let font = FontId::proportional(15.0);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, p.text);
    let switch = vec2(30.0, 18.0);
    let dot = if modified { 12.0 } else { 0.0 };
    let width = 12.0 + switch.x + 10.0 + galley.size().x + dot + 16.0;
    let (rect, r) = ui.allocate_exact_size(vec2(width, 38.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_responsive(r.id, on);
        let hovered = r.hovered();
        let fill = mix(
            if hovered { p.surface_raised } else { p.surface },
            if hovered { p.accent_hover } else { p.accent },
            t,
        );
        let outline = mix(
            if hovered {
                p.text_muted
            } else {
                p.border_strong
            },
            p.accent,
            t,
        );
        let painter = ui.painter();
        painter.rect(
            rect,
            rect.height() / 2.0,
            fill,
            Stroke::new(1.5, outline),
            StrokeKind::Inside,
        );
        // The switch inside the pill.
        let track = egui::Rect::from_min_size(
            pos2(rect.left() + 12.0, rect.center().y - switch.y / 2.0),
            switch,
        );
        let track_fill = mix(p.border_strong, Color32::from_white_alpha(70), t);
        painter.rect_filled(track, switch.y / 2.0, track_fill);
        let knob_x = egui::lerp(
            (track.left() + switch.y / 2.0)..=(track.right() - switch.y / 2.0),
            t,
        );
        let knob = mix(p.text_muted, Color32::WHITE, t);
        painter.circle_filled(pos2(knob_x, track.center().y), switch.y / 2.0 - 3.0, knob);
        let text_color = mix(p.text, p.on_accent, t);
        let at = pos2(
            track.right() + 10.0,
            rect.center().y - galley.size().y / 2.0,
        );
        let text_right = at.x + galley.size().x;
        painter.galley_with_override_text_color(at, galley, text_color);
        if modified {
            // Sliders moved since the preset was loaded.
            painter.circle_filled(pos2(text_right + 8.0, rect.center().y), 3.0, text_color);
        }
        if r.has_focus() {
            painter.rect_stroke(
                rect.expand(2.0),
                rect.height() / 2.0 + 2.0,
                Stroke::new(1.5, p.accent),
                StrokeKind::Outside,
            );
        }
    }
    r.on_hover_cursor(CursorIcon::PointingHand)
}

fn with_key(tip: &str, key: &Option<String>) -> String {
    match key {
        Some(k) => format!("{tip}\nHotkey: {k}"),
        None => tip.to_string(),
    }
}

fn mic_block(ui: &mut Ui, v: &View, show_name: bool, max_name: f32) {
    let p = palette(ui);
    let (name, tone, tip) = mic_summary(v);
    let color = tone.color(p);
    let r = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.label(RichText::new(icons::MIC).size(18.0).color(color));
            if show_name {
                let galley = widgets::elided(
                    ui,
                    &name,
                    FontId::proportional(font::BODY),
                    p.text,
                    max_name,
                    1,
                );
                let (rect, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
                ui.painter().galley(rect.min, galley, p.text);
            }
        })
        .response;
    r.on_hover_text(tip);
}

fn meters_block(ui: &mut Ui, v: &View, width: f32, latency: bool) {
    let p = palette(ui);
    // Two meter rows laid out by hand: a `horizontal` row would be a button's height each,
    // and the header has no room for that.
    let (row, gap, label_w) = (9.0, 6.0, 26.0);
    let (rect, _) = ui.allocate_exact_size(vec2(label_w + width, 2.0 * row + gap), Sense::hover());
    for (i, (label, level)) in [("In", v.input_level), ("Out", v.output_level)]
        .into_iter()
        .enumerate()
    {
        let top = rect.top() + i as f32 * (row + gap);
        ui.painter().text(
            pos2(rect.left(), top + row / 2.0),
            egui::Align2::LEFT_CENTER,
            label,
            FontId::proportional(font::SMALL - 0.5),
            p.text_muted,
        );
        let meter = egui::Rect::from_min_size(pos2(rect.left() + label_w, top), vec2(width, row));
        meters::level_meter_at(
            ui,
            meter,
            ("header_meter", label),
            level,
            PROGRAM_FLOOR_DB,
            None,
        );
    }
    meters::clip_lamp(ui, v.clipping)
        .on_hover_text("Lights when the output is too loud and distorts. Lower the output gain.");
    if latency {
        let text = match v.status.latency_ms {
            Some(ms) => format!("{ms:.0} ms"),
            None => "\u{2013} ms".to_string(),
        };
        ui.label(RichText::new(text).size(font::SMALL).color(p.text_muted))
            .on_hover_text(
                "Estimated delay from your mic to the virtual mic. Audio setup shows what it \
                 is made of.",
            );
    }
}

fn mute_button(ui: &mut Ui, v: &View, compact: bool) -> Response {
    let p = palette(ui);
    let (icon, label, fill, fg, stroke) = if v.muted {
        (icons::MIC_OFF, "Muted", p.danger, p.on_accent, p.danger)
    } else {
        (icons::MIC, "Mute", p.surface, p.text, p.border_strong)
    };
    let text = if compact {
        icon.to_string()
    } else {
        format!("{icon}  {label}")
    };
    let button = egui::Button::new(RichText::new(text).size(15.0).color(fg))
        .fill(fill)
        .stroke(Stroke::new(1.0, stroke))
        .corner_radius(CornerRadius::same(19))
        .min_size(vec2(if compact { 38.0 } else { 84.0 }, 38.0));
    let tip = if v.muted {
        "Your mic is muted; soundboard clips are still sent. Click to unmute."
    } else {
        "Silences your mic. Soundboard clips still play."
    };
    ui.add(button).on_hover_text(with_key(tip, &v.mute_key))
}

fn panic_button(ui: &mut Ui, v: &View) -> Response {
    let p = palette(ui);
    let button = egui::Button::new(
        RichText::new("PANIC")
            .size(15.0)
            .strong()
            .color(p.on_accent),
    )
    .fill(p.panic)
    .corner_radius(CornerRadius::same(19))
    .min_size(vec2(88.0, 38.0));
    ui.add(button).on_hover_text(with_key(
        "Safe mode: stops every sound and sends only your clean mic (voice changer, effects \
         and Melody Voice bypassed). No setting is changed; Restore puts everything back.",
        &v.panic_key,
    ))
}

/// The red banner shown instead of the normal header while safe mode is on.
fn safe_banner(ui: &mut Ui, v: &View, p: &Palette, actions: &mut Vec<Action>) {
    let wide = ui.available_width() > 760.0;
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = space::M;
        ui.label(RichText::new(icons::SAFE).size(24.0).color(p.on_accent));
        ui.label(
            RichText::new("SAFE MODE")
                .size(18.0)
                .strong()
                .color(p.on_accent),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let restore = egui::Button::new(
                RichText::new(format!("{}  Restore", icons::RESTORE))
                    .size(15.0)
                    .strong()
                    .color(p.panic),
            )
            .fill(Color32::WHITE)
            .corner_radius(CornerRadius::same(19))
            .min_size(vec2(110.0, 38.0));
            let r = ui.add(restore).on_hover_text(with_key(
                "Turn everything back on exactly as it was.",
                &v.panic_key,
            ));
            #[cfg(test)]
            widgets::remember(ui.ctx(), "header", "panic", r.rect);
            if r.clicked() {
                actions.push(Action::Restore);
            }
            if wide {
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    let headphones = if v.panic_mutes_monitor {
                        ", headphones muted"
                    } else {
                        ""
                    };
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!(
                                "Only your clean mic is sent: no effects, no Melody Voice, no \
                                 sounds{headphones}. Nothing was changed."
                            ))
                            .color(p.on_accent),
                        )
                        .truncate(),
                    );
                });
            }
        });
    });
}

/// The fill of the header panel (red in safe mode).
pub fn fill(p: &Palette, safe: bool) -> Color32 {
    if safe { p.panic } else { p.surface }
}

pub fn show(ui: &mut Ui, v: &View) -> Vec<Action> {
    let p = palette(ui);
    let mut actions = Vec::new();
    if v.safe {
        safe_banner(ui, v, p, &mut actions);
        return actions;
    }
    // What fits: the three switches always; the rest by how much room there is.
    let width = ui.available_width();
    let show_logo_text = width >= 1080.0;
    let show_mic_name = width >= 900.0;
    let show_latency = width >= 640.0;
    let chip_text = width >= 1180.0;
    let compact_mute = width < 720.0;
    let meter_width = ((width - 560.0) * 0.22).clamp(56.0, 150.0);

    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = space::M;
        ui.label(RichText::new(icons::LOGO).size(24.0).color(p.accent));
        if show_logo_text {
            ui.label(RichText::new("VoxShift").size(17.0).strong().color(p.text));
        }

        let preset = v.preset.unwrap_or("Custom");
        let text = if v.voice_on {
            format!("ON  \u{b7}  {preset}")
        } else {
            "OFF  \u{b7}  clean voice".to_string()
        };
        let tip = if v.voice_on {
            "The voice changer is on: others hear your changed voice. Click to send your \
             clean voice instead."
        } else {
            "The voice changer is off: others hear your clean voice. Click to turn the \
             effects on."
        };
        let pill = voice_pill(ui, v.voice_on, &text, v.voice_on && v.modified)
            .on_hover_text(with_key(tip, &v.toggle_key));
        #[cfg(test)]
        widgets::remember(ui.ctx(), "header", "voice", pill.rect);
        if pill.clicked() {
            actions.push(Action::ToggleVoice);
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let r = panic_button(ui, v);
            #[cfg(test)]
            widgets::remember(ui.ctx(), "header", "panic", r.rect);
            if r.clicked() {
                actions.push(Action::Panic);
            }
            let r = mute_button(ui, v, compact_mute);
            #[cfg(test)]
            widgets::remember(ui.ctx(), "header", "mute", r.rect);
            if r.clicked() {
                actions.push(Action::ToggleMute);
            }

            // The middle takes what is left between the pill and the buttons.
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.add_space(space::S);
                mic_block(ui, v, show_mic_name, 190.0);
                meters_block(ui, v, meter_width, show_latency);
                for problem in problems(v.status, v.muted, v.hotkeys_available) {
                    let text = if chip_text { problem.text() } else { "" };
                    let r = widgets::chip(ui, problem.icon(), text, problem.tone());
                    #[cfg(test)]
                    widgets::remember(ui.ctx(), "header_chip", format!("{problem:?}"), r.rect);
                    let tip = problem.tip(&v.status.reason);
                    let tip = if chip_text {
                        tip
                    } else {
                        format!("{}\n{tip}", problem.text())
                    };
                    if r.on_hover_text(tip).clicked() {
                        actions.push(problem.action());
                    }
                }
            });
        });
    });
    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::device_manager::Endpoint;

    fn status(output: bool, cable: bool, active: ActiveMic) -> DeviceStatus {
        DeviceStatus {
            scanned: true,
            cable: cable.then(|| "CABLE Input (VB-Audio Virtual Cable)".to_string()),
            output: Endpoint {
                device: output.then(|| "CABLE Input (VB-Audio Virtual Cable)".to_string()),
                ..Default::default()
            },
            active,
            ..Default::default()
        }
    }

    #[test]
    fn chips_only_show_real_problems_most_serious_first() {
        use Problem::*;
        let all_good = status(true, true, ActiveMic::Primary);
        assert!(
            problems(&all_good, false, true).is_empty(),
            "nothing to report"
        );
        assert_eq!(
            problems(&status(true, true, ActiveMic::Fallback), false, true),
            [Fallback]
        );
        assert_eq!(
            problems(&status(true, false, ActiveMic::Primary), false, true),
            [CableMissing],
            "another output is in use, but the cable is what other apps need"
        );
        assert_eq!(
            problems(&status(false, false, ActiveMic::None), true, false),
            [NoOutput, CableMissing, Muted, NoHotkeys],
            "no output: the missing mic is not the news"
        );
        assert_eq!(
            problems(&status(true, true, ActiveMic::None), false, true),
            [NoMic]
        );
        // While audio is still starting nothing is flagged yet.
        let starting = DeviceStatus::default();
        assert!(problems(&starting, false, true).is_empty());
        assert_eq!(problems(&starting, true, true), [Muted]);
    }

    #[test]
    fn every_chip_leads_to_its_fix() {
        assert_eq!(Problem::CableMissing.action(), Action::Goto(Page::Audio));
        assert_eq!(Problem::Fallback.action(), Action::Goto(Page::Audio));
        assert_eq!(Problem::Muted.action(), Action::ToggleMute);
        assert_eq!(Problem::NoHotkeys.action(), Action::Goto(Page::Hotkeys));
        assert!(
            Problem::Fallback
                .tip("primary mic is silent")
                .contains("silent")
        );
    }

    #[test]
    fn device_names_lose_their_wrapper() {
        assert_eq!(
            short_device_name("Microphone (HyperX Cloud II Wireless)"),
            "HyperX Cloud II Wireless"
        );
        assert_eq!(
            short_device_name("Microphone (2- USB PnP Audio Device)"),
            "2- USB PnP Audio Device"
        );
        assert_eq!(short_device_name("Line In"), "Line In");
        assert_eq!(short_device_name("Odd ()"), "Odd ()");
        assert_eq!(short_device_name(""), "");
    }
}
