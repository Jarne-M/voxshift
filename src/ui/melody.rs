//! The Melody Voice panel: talk, and it comes out sung.

use super::effects::control;
use super::theme::{ACCENT, WARN, card};
use crate::dsp::conductor::{KEY_NAMES, Mode, Scale};
use crate::dsp::params::{P, Snapshot};
use crate::dsp::pitch_detect::note_name;
use egui::{RichText, Ui};

/// Every parameter this panel shows (the generic effect sections skip them).
#[cfg(test)]
pub const CONTROLS: [P; 19] = [
    P::MelodyOn,
    P::MelodyMode,
    P::MelodyKey,
    P::MelodyScale,
    P::MelodyRetune,
    P::MelodyGlide,
    P::MelodyTranspose,
    P::MelodyFold,
    P::MelodyFormant,
    P::MelodyFormantShift,
    P::MelodyMix,
    P::MelodyBpm,
    P::MelodyLoop,
    P::MelodySensitivity,
    P::MelodyMinNote,
    P::MelodyResetAfter,
    P::MelodyWindow,
    P::MelodyMinHz,
    P::MelodyMaxHz,
];

/// Live values from the audio thread.
pub struct Status {
    /// Your pitch and the target note (MIDI numbers, NaN = none).
    pub detected: f32,
    pub target: f32,
    /// Delay Melody Voice adds, in ms.
    pub latency_ms: f32,
}

fn choice<T: Copy + PartialEq>(
    ui: &mut Ui,
    fx: &mut Snapshot,
    id: P,
    options: &[T],
    label: impl Fn(T) -> String,
    width: f32,
) -> bool {
    let current = fx.choice(id).min(options.len() - 1);
    let mut picked = current;
    egui::ComboBox::from_id_salt(id.info().key)
        .width(width)
        .selected_text(label(options[current]))
        .show_ui(ui, |ui| {
            for (i, &o) in options.iter().enumerate() {
                ui.selectable_value(&mut picked, i, label(o));
            }
        });
    if picked != current {
        fx.set(id, picked as f32);
        true
    } else {
        false
    }
}

/// "You: A3 → Target: C4".
pub fn status_text(s: &Status) -> String {
    let you = if s.detected.is_finite() {
        note_name(s.detected)
    } else {
        "-".to_string()
    };
    let target = if s.target.is_finite() {
        note_name(s.target)
    } else {
        "-".to_string()
    };
    format!("You: {you}  \u{2192}  Target: {target}")
}

pub fn panel(ui: &mut Ui, fx: &mut Snapshot, status: &Status) -> bool {
    let mut changed = false;
    card(ui, |ui| {
        let on = fx.on(P::MelodyOn);
        ui.horizontal(|ui| {
            let color = if on {
                ACCENT
            } else {
                ui.visuals().weak_text_color()
            };
            ui.label(RichText::new("Melody Voice").heading().color(color));
            ui.label(RichText::new("talk, and it comes out sung").small().weak());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut on = on;
                let text = if on { "ON" } else { "off" };
                if ui.toggle_value(&mut on, text).changed() {
                    fx.set(P::MelodyOn, if on { 1.0 } else { 0.0 });
                    changed = true;
                }
            });
        });
        if on {
            ui.label(RichText::new(status_text(status)).strong().size(16.0));
            ui.label(
                RichText::new(format!(
                    "Adds ~{:.0} ms of delay, more than any other effect.",
                    status.latency_ms
                ))
                .small()
                .color(WARN),
            );
        }
        ui.add_enabled_ui(on, |ui| {
            let columns = if ui.available_width() > 640.0 { 2 } else { 1 };
            ui.columns(columns, |cols| {
                let ui = &mut cols[0];
                ui.horizontal(|ui| {
                    ui.label("Mode");
                    changed |= choice(
                        ui,
                        fx,
                        P::MelodyMode,
                        &Mode::ALL,
                        |m| m.label().into(),
                        190.0,
                    );
                });
                let mode = Mode::from_index(fx.choice(P::MelodyMode));
                if mode == Mode::ScaleSnap {
                    ui.horizontal(|ui| {
                        ui.label("Key");
                        changed |= choice(ui, fx, P::MelodyKey, &KEY_NAMES, |k| k.into(), 50.0);
                        changed |= choice(
                            ui,
                            fx,
                            P::MelodyScale,
                            &Scale::ALL,
                            |s| s.label().into(),
                            140.0,
                        );
                    });
                }
                for id in [P::MelodyTranspose, P::MelodyRetune, P::MelodyGlide] {
                    changed |= control(ui, fx, id);
                }
                ui.label(
                    RichText::new("Retune speed: 0 = hard \"robot\" tune, 150+ = natural")
                        .small()
                        .weak(),
                );
                let ui = &mut cols[columns - 1];
                changed |= control(ui, fx, P::MelodyFormant);
                ui.add_enabled_ui(fx.on(P::MelodyFormant), |ui| {
                    changed |= control(ui, fx, P::MelodyFormantShift);
                });
                for id in [
                    P::MelodyMix,
                    P::MelodySensitivity,
                    P::MelodyWindow,
                    P::MelodyMinHz,
                    P::MelodyMaxHz,
                ] {
                    changed |= control(ui, fx, id);
                }
            });
        });
    });
    changed
}
