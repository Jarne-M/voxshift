//! The effect chain: one card per effect, in the order your voice passes through them.
//!
//! A card's header has the effect's place in the chain, its icon and name, one line saying
//! what it does, a real on/off switch and a menu with "Reset to defaults". An effect that is
//! off is just that header; switching it on opens its sliders. A click on the header (or the
//! arrow) opens or closes a card by hand.
//!
//! The sliders themselves are generated from the parameter table (`dsp::params`).

use super::icons;
use super::theme::{font, mix, palette, space};
use super::widgets::{self, Slider, card_frame};
use crate::dsp::params::{Kind, P, Snapshot};
use egui::collapsing_header::CollapsingState;
use egui::{Align, FontId, Id, Layout, Rect, RichText, Sense, Stroke, Ui, vec2};

/// One effect of the chain.
pub struct Section {
    pub name: &'static str,
    pub icon: &'static str,
    /// One line: what it does.
    pub about: &'static str,
    /// Its on/off parameter (`None`: always on).
    pub toggle: Option<P>,
    pub controls: &'static [P],
}

/// The chain, in processing order.
pub const SECTIONS: [Section; 9] = [
    Section {
        name: "Gain",
        icon: icons::FX_GAIN,
        about: "Levels in and out, and how much effect you hear",
        toggle: None,
        controls: &[P::InputGain, P::OutputGain, P::DryWet],
    },
    Section {
        name: "Noise gate",
        icon: icons::FX_GATE,
        about: "Mutes your mic while you are not talking",
        toggle: Some(P::GateOn),
        controls: &[P::GateThreshold],
    },
    Section {
        name: "Pitch",
        icon: icons::FX_PITCH,
        about: "Makes your voice higher or lower",
        toggle: Some(P::PitchOn),
        controls: &[P::PitchSemis, P::PitchFormant],
    },
    Section {
        name: "Tone",
        icon: icons::FX_TONE,
        about: "Low cut, high cut and a 3-band EQ",
        toggle: Some(P::EqOn),
        controls: &[
            P::LowCut,
            P::HighCut,
            P::LowGain,
            P::MidGain,
            P::MidFreq,
            P::HighGain,
        ],
    },
    Section {
        name: "Distortion",
        icon: icons::FX_DISTORTION,
        about: "A crunchy, overdriven sound",
        toggle: Some(P::DistOn),
        controls: &[P::DistDrive, P::DistMix],
    },
    Section {
        name: "Robot",
        icon: icons::FX_ROBOT,
        about: "A metallic, robotic voice",
        toggle: Some(P::RobotOn),
        controls: &[P::RobotFreq, P::RobotMix],
    },
    Section {
        name: "Chorus",
        icon: icons::FX_CHORUS,
        about: "A thicker voice, like several people at once",
        toggle: Some(P::ChorusOn),
        controls: &[P::ChorusRate, P::ChorusDepth, P::ChorusMix],
    },
    Section {
        name: "Echo",
        icon: icons::FX_ECHO,
        about: "Repeats of your voice",
        toggle: Some(P::EchoOn),
        controls: &[P::EchoTime, P::EchoFeedback, P::EchoMix],
    },
    Section {
        name: "Reverb",
        icon: icons::FX_REVERB,
        about: "Room or hall sound",
        toggle: Some(P::ReverbOn),
        controls: &[P::ReverbSize, P::ReverbDamping, P::ReverbMix],
    },
];

/// What a control does, for its tooltip.
pub fn tip(id: P) -> &'static str {
    match id {
        P::InputGain => "How loud your mic comes in, before any effect.",
        P::OutputGain => "How loud the result goes out. Lower it if the CLIP lamp lights up.",
        P::DryWet => "How much of the changed voice is heard next to your clean voice.",
        P::GateThreshold => {
            "Your mic is muted while it is quieter than this. Raise it until the background \
             noise is gone."
        }
        P::PitchSemis => "Semitones up or down; 12 is one octave.",
        P::PitchFormant | P::MelodyFormant => {
            "Keeps your voice's character, so a shifted voice still sounds like a person \
             instead of a chipmunk or a giant."
        }
        P::LowCut => "Removes everything below this frequency (rumble, pops).",
        P::HighCut => "Removes everything above this frequency (a muffled, telephone-like sound).",
        P::LowGain => "Bass: more or less around 200 Hz.",
        P::MidGain => "More or less around the Mid freq.",
        P::MidFreq => "The frequency the Mid slider works on.",
        P::HighGain => "Treble: more or less around 4 kHz.",
        P::DistDrive => "How hard the voice is overdriven.",
        P::RobotFreq => "The robot's buzz: low is a growl, high is metallic.",
        P::ChorusRate => "How fast the extra voices waver.",
        P::ChorusDepth => "How far the extra voices drift away from yours.",
        P::EchoTime => "The time between repeats.",
        P::EchoFeedback => "How long it keeps echoing: higher gives more repeats.",
        P::ReverbSize => "The size of the room, from a small room to a large hall.",
        P::ReverbDamping => "How quickly the high tones die away (a soft or a hard room).",
        P::DistMix | P::RobotMix | P::ChorusMix | P::EchoMix | P::ReverbMix => {
            "How much of this effect is mixed in."
        }
        P::MelodyRetune => {
            "How fast your voice is pulled onto the note. 0 ms is the hard, robotic tune; \
             150 ms and up sounds natural."
        }
        P::MelodyGlide => "How quickly it slides from one note to the next.",
        P::MelodyTranspose => "Moves the whole melody up or down.",
        P::MelodyFold => {
            "Moves the melody by whole octaves so it sits near your own voice, which sounds \
             much cleaner."
        }
        P::MelodyFormantShift => {
            "Changes the voice's character on purpose: lower is darker and bigger, higher is \
             brighter and smaller."
        }
        P::MelodyMix => "How much of the sung voice is heard next to your normal voice.",
        P::MelodyBpm => "The speed of the song in Tempo-synced mode, and of Play melody.",
        P::MelodyLoop => "At the end of the melody, start again from the first note.",
        P::MelodySensitivity => "How easily a new syllable is detected.",
        P::MelodyMinNote => "A note lasts at least this long, so a glitch cannot skip notes.",
        P::MelodyResetAfter => {
            "After this many seconds of silence the melody starts over. 0 = never."
        }
        P::MelodyWindow => {
            "How much sound the pitch detector looks at. Longer is steadier for low voices but \
             adds delay."
        }
        P::MelodyMinHz | P::MelodyMaxHz => {
            "The range your voice's pitch stays in. If Melody Voice jumps to the wrong octave, \
             narrow it to your own voice."
        }
        _ => "",
    }
}

/// Decimals shown for a slider value.
fn decimals(info: &crate::dsp::params::ParamInfo) -> usize {
    let range = info.max - info.min;
    if range <= 2.0 {
        2
    } else if range <= 12.0 || matches!(info.unit, "dB" | "st") {
        1
    } else {
        0
    }
}

/// One slider (or checkbox for toggles), built from the parameter table.
pub fn control(ui: &mut Ui, fx: &mut Snapshot, id: P) -> bool {
    control_with(ui, fx, id, 120.0)
}

/// [`control`] with room for a longer label.
pub fn control_with(ui: &mut Ui, fx: &mut Snapshot, id: P, label_width: f32) -> bool {
    let info = id.info();
    let mut v = fx.get(id);
    let changed = if info.kind == Kind::Toggle {
        let mut on = v >= 0.5;
        let r = ui.checkbox(&mut on, info.label);
        v = if on { 1.0 } else { 0.0 };
        let hint = tip(id);
        if hint.is_empty() {
            r
        } else {
            r.on_hover_text(hint)
        }
        .changed()
    } else {
        // Unitless 0..1 values are amounts: show them as a percentage.
        let percent = info.unit.is_empty() && info.min >= 0.0 && info.max <= 1.0;
        Slider::new(&mut v, info.min..=info.max)
            .label(info.label)
            .unit(info.unit)
            .default_value(info.default)
            .decimals(decimals(info))
            .logarithmic(info.kind == Kind::Log)
            .step(if info.unit == "st" { 0.1 } else { 0.0 })
            .percent(percent)
            .tip(tip(id))
            .label_width(label_width)
            .show(ui)
            .changed()
    };
    if changed {
        fx.set(id, v);
    }
    changed
}

/// Whether a card is open: what you chose by hand, otherwise open while the effect is on.
fn is_open(ui: &Ui, card: Id, enabled: bool) -> bool {
    ui.data(|d| d.get_temp::<bool>(card.with("by_hand")))
        .unwrap_or(enabled)
}

fn set_open_by_hand(ui: &Ui, card: Id, open: Option<bool>) {
    ui.data_mut(|d| match open {
        Some(open) => {
            d.insert_temp(card.with("by_hand"), open);
        }
        None => d.remove::<bool>(card.with("by_hand")),
    });
}

/// The id of an effect's card (also keys where it was drawn, for tests).
pub fn card_id(name: &str) -> Id {
    Id::new(("fx_card", name))
}

/// The step number in front of an effect's name.
fn step_badge(ui: &mut Ui, step: usize, on: bool) {
    let p = palette(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
    let (fill, text) = if on {
        (p.accent_soft, p.accent)
    } else {
        (p.surface_raised, p.text_faint)
    };
    ui.painter().circle_filled(rect.center(), 10.0, fill);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        step.to_string(),
        FontId::proportional(font::SMALL - 0.5),
        text,
    );
}

/// One effect card. Returns true if a value changed.
fn section(ui: &mut Ui, fx: &mut Snapshot, step: usize, s: &Section) -> bool {
    let p = palette(ui);
    let id = card_id(s.name);
    let enabled = s.toggle.is_none_or(|t| fx.on(t));
    let open = is_open(ui, id, enabled);
    let mut state = CollapsingState::load_with_default_open(ui.ctx(), id, open);
    state.set_open(open);
    let mut changed = false;

    let border = if enabled {
        mix(p.border, p.accent, 0.4)
    } else {
        p.border
    };
    let frame = card_frame(p)
        .stroke(Stroke::new(1.0, border))
        .inner_margin(egui::Margin::symmetric(14, 10));
    let shown = frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        // The whole header row opens and closes the card; the switch and the menu on it are
        // added after it, so they get their own clicks.
        let row = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), 28.0));
        let header = ui
            .interact(row, id.with("header"), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if header.clicked() {
            set_open_by_hand(ui, id, Some(!open));
        }
        ui.horizontal(|ui| {
            ui.set_height(28.0);
            ui.spacing_mut().item_spacing.x = space::S;
            step_badge(ui, step, enabled);
            let (icon_color, name_color) = if enabled {
                (p.accent, p.text)
            } else {
                (p.text_faint, p.text_muted)
            };
            ui.label(RichText::new(s.icon).size(18.0).color(icon_color));
            ui.label(
                RichText::new(s.name)
                    .size(font::SECTION)
                    .strong()
                    .color(name_color),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let arrow = if open { icons::COLLAPSE } else { icons::EXPAND };
                let tip = if open { "Close" } else { "Show the sliders" };
                if widgets::icon_button(ui, arrow, tip).clicked() {
                    set_open_by_hand(ui, id, Some(!open));
                }
                widgets::menu_icon_button(ui, icons::MORE, "", |ui| {
                    if ui
                        .button(format!("{}  Reset to defaults", icons::RESTORE))
                        .clicked()
                    {
                        for &c in s.controls {
                            fx.set(c, c.info().default);
                        }
                        changed = true;
                        ui.close();
                    }
                });
                if let Some(t) = s.toggle {
                    let mut on = fx.on(t);
                    let r = widgets::toggle(ui, &mut on);
                    #[cfg(test)]
                    widgets::remember(ui.ctx(), "fx_toggle", s.name, r.rect);
                    if r.changed() {
                        fx.set(t, if on { 1.0 } else { 0.0 });
                        changed = true;
                        // Switching it follows the rule again: on opens, off closes.
                        set_open_by_hand(ui, id, None);
                    }
                }
                // The description takes the room that is left, on one line.
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(s.about).small().color(p.text_muted))
                            .truncate(),
                    );
                });
            });
        });
        state.show_body_unindented(ui, |ui| {
            ui.add_space(space::XS);
            ui.add_enabled_ui(enabled, |ui| {
                for &c in s.controls {
                    changed |= control(ui, fx, c);
                }
            });
            if s.name == "Pitch" && enabled {
                widgets::hint(ui, "Adds about 15 ms of delay.");
            }
        });
    });
    #[cfg(test)]
    widgets::remember(ui.ctx(), "fx_card", s.name, shown.response.rect);
    let _ = shown;
    changed
}

/// How many columns of cards fit in `width`.
pub fn columns_for(width: f32) -> usize {
    if width < 700.0 {
        1
    } else if width < 1080.0 {
        2
    } else {
        3
    }
}

/// Where each column starts when `count` cards go into `columns` columns in chain order,
/// the first columns taking the extra ones: 9 cards in 2 columns is `[0, 5]`.
///
/// The split only depends on the number of columns, never on which cards are open, so a
/// card never jumps to another column while you work; each column simply packs upwards.
pub fn column_starts(count: usize, columns: usize) -> Vec<usize> {
    let columns = columns.clamp(1, count.max(1));
    let (base, extra) = (count / columns, count % columns);
    let mut starts = Vec::with_capacity(columns);
    let mut at = 0;
    for c in 0..columns {
        starts.push(at);
        at += base + usize::from(c < extra);
    }
    starts
}

/// What the chain reports back.
#[derive(Default)]
pub struct Outcome {
    /// A value changed.
    pub changed: bool,
    /// "Reset all effects" was clicked.
    pub reset_all: bool,
}

/// Draws the whole chain. `voice_on`: whether the voice changer is on; when it is off the
/// chain is dimmed (it can still be adjusted) and a note says what others hear.
/// A/B compare: hold it to hear your clean voice in the headphones instead of the effects.
/// `hearing`: the monitor is on and plays your voice (otherwise there is nothing to compare).
/// Returns true while it is held down.
pub fn ab_button(ui: &mut Ui, hearing: bool) -> bool {
    let p = palette(ui);
    let id = egui::Id::new("ab_compare");
    let held_before = ui.ctx().data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    let (text, color) = if held_before {
        ("Clean voice", p.on_accent)
    } else {
        ("Hold: A/B", p.text_muted)
    };
    let button =
        egui::Button::new(RichText::new(format!("{}  {text}", icons::COMPARE)).color(color))
            .sense(egui::Sense::click_and_drag());
    let button = if held_before {
        button.fill(p.accent)
    } else {
        button
    };
    let r = ui
        .add_enabled(hearing, button)
        .on_hover_text(
            "Hold to hear your clean voice in your headphones, let go for the effects. \
             Others keep hearing the effects.",
        )
        .on_disabled_hover_text(
            "Turn on the monitor and \"Hear myself\" under Audio setup to compare",
        );
    #[cfg(test)]
    widgets::remember(ui.ctx(), "voice", "ab", r.rect);
    let held = hearing && r.is_pointer_button_down_on();
    ui.ctx().data_mut(|d| d.insert_temp(id, held));
    held
}

/// `tools` adds buttons to the right of the title row, left of "Reset all effects".
pub fn chain(
    ui: &mut Ui,
    fx: &mut Snapshot,
    voice_on: bool,
    tools: impl FnOnce(&mut Ui),
) -> Outcome {
    let p = palette(ui);
    let mut out = Outcome::default();
    ui.horizontal(|ui| {
        widgets::section_title(ui, icons::FX_CHAIN, "Effect chain");
        if ui.available_width() > 760.0 {
            widgets::hint(ui, "Your voice passes through these in order, 1 to 9");
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let reset = ui
                .button(
                    RichText::new(format!("{}  Reset all effects", icons::RESTORE))
                        .color(p.text_muted),
                )
                .on_hover_text(
                    "Everything back to the defaults (the Clean preset). Your input gain is kept.",
                );
            if reset.clicked() {
                out.reset_all = true;
            }
            tools(ui);
        });
    });
    if !voice_on {
        widgets::notice(
            ui,
            icons::INFO,
            "Voice changer is off \u{2014} others hear your clean voice",
            widgets::Tone::Neutral,
        );
    }
    ui.add_space(space::XS);
    ui.scope(|ui| {
        if !voice_on {
            ui.multiply_opacity(0.55);
        }
        let columns = columns_for(ui.available_width());
        let starts = column_starts(SECTIONS.len(), columns);
        ui.spacing_mut().item_spacing.x = space::M;
        ui.columns(starts.len(), |cols| {
            for (c, ui) in cols.iter_mut().enumerate() {
                ui.spacing_mut().item_spacing.y = space::M;
                let end = starts.get(c + 1).copied().unwrap_or(SECTIONS.len());
                for (i, s) in SECTIONS.iter().enumerate().take(end).skip(starts[c]) {
                    // (Its own id scope: "Mix" is a slider in five cards.)
                    out.changed |= ui.push_id(s.name, |ui| section(ui, fx, i + 1, s)).inner;
                }
            }
        });
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::params::PARAMS;

    #[test]
    fn every_parameter_has_a_control() {
        for info in &PARAMS {
            let shown = SECTIONS
                .iter()
                .any(|s| s.toggle == Some(info.id) || s.controls.contains(&info.id))
                || super::super::melody::CONTROLS.contains(&info.id);
            assert!(shown, "{} has no control", info.key);
        }
    }

    #[test]
    fn every_slider_explains_itself() {
        for info in PARAMS.iter().filter(|i| i.kind != Kind::Choice) {
            let is_switch =
                SECTIONS.iter().any(|s| s.toggle == Some(info.id)) || info.id == P::MelodyOn;
            if !is_switch {
                assert!(!tip(info.id).is_empty(), "{} has no tooltip", info.key);
            }
        }
    }

    #[test]
    fn the_chain_is_in_processing_order_and_columns_keep_it() {
        let names: Vec<&str> = SECTIONS.iter().map(|s| s.name).collect();
        assert_eq!(
            names,
            [
                "Gain",
                "Noise gate",
                "Pitch",
                "Tone",
                "Distortion",
                "Robot",
                "Chorus",
                "Echo",
                "Reverb"
            ]
        );
        assert_eq!(column_starts(9, 1), [0]);
        assert_eq!(column_starts(9, 2), [0, 5]);
        assert_eq!(column_starts(9, 3), [0, 3, 6]);
        assert_eq!(column_starts(2, 3), [0, 1], "never more columns than cards");
        assert_eq!(columns_for(500.0), 1);
        assert_eq!(columns_for(900.0), 2);
        assert_eq!(columns_for(1280.0), 3);
    }
}
