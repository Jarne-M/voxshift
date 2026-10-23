//! Effect sliders, generated from the parameter table.

use super::theme::{ACCENT, card};
use crate::dsp::params::{Kind, P, Snapshot};
use egui::{RichText, Ui};

/// (title, enable toggle, controls)
const SECTIONS: [(&str, Option<P>, &[P]); 9] = [
    ("Gain", None, &[P::InputGain, P::OutputGain, P::DryWet]),
    ("Noise gate", Some(P::GateOn), &[P::GateThreshold]),
    ("Pitch", Some(P::PitchOn), &[P::PitchSemis, P::PitchFormant]),
    (
        "Tone",
        Some(P::EqOn),
        &[
            P::LowCut,
            P::HighCut,
            P::LowGain,
            P::MidGain,
            P::MidFreq,
            P::HighGain,
        ],
    ),
    ("Distortion", Some(P::DistOn), &[P::DistDrive, P::DistMix]),
    ("Robot", Some(P::RobotOn), &[P::RobotFreq, P::RobotMix]),
    (
        "Chorus",
        Some(P::ChorusOn),
        &[P::ChorusRate, P::ChorusDepth, P::ChorusMix],
    ),
    (
        "Echo",
        Some(P::EchoOn),
        &[P::EchoTime, P::EchoFeedback, P::EchoMix],
    ),
    (
        "Reverb",
        Some(P::ReverbOn),
        &[P::ReverbSize, P::ReverbDamping, P::ReverbMix],
    ),
];

/// One slider (or checkbox for toggles). Double-click resets to the default.
pub fn control(ui: &mut Ui, fx: &mut Snapshot, id: P) -> bool {
    let info = id.info();
    let mut v = fx.get(id);
    let changed = if info.kind == Kind::Toggle {
        let mut on = v >= 0.5;
        let r = ui.checkbox(&mut on, info.label);
        v = if on { 1.0 } else { 0.0 };
        r.changed()
    } else {
        let decimals = if info.max - info.min <= 2.0 {
            2
        } else if info.unit == "st" {
            1
        } else {
            0
        };
        let mut slider = egui::Slider::new(&mut v, info.min..=info.max)
            .text(info.label)
            .fixed_decimals(decimals)
            .logarithmic(info.kind == Kind::Log);
        if !info.unit.is_empty() {
            slider = slider.suffix(format!(" {}", info.unit));
        }
        if info.unit == "st" {
            slider = slider.step_by(0.1);
        }
        let r = ui.add(slider).on_hover_text("Double-click to reset");
        if r.double_clicked() {
            v = info.default;
        }
        r.changed() || r.double_clicked()
    };
    if changed {
        fx.set(id, v);
    }
    changed
}

fn section(ui: &mut Ui, fx: &mut Snapshot, title: &str, toggle: Option<P>, controls: &[P]) -> bool {
    let mut changed = false;
    card(ui, |ui| {
        ui.horizontal(|ui| {
            let on = toggle.is_none_or(|t| fx.on(t));
            let color = if on {
                ACCENT
            } else {
                ui.visuals().weak_text_color()
            };
            ui.label(RichText::new(title).heading().color(color));
            if let Some(t) = toggle {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut on = fx.on(t);
                    let text = if on { "ON" } else { "off" };
                    if ui.toggle_value(&mut on, text).changed() {
                        fx.set(t, if on { 1.0 } else { 0.0 });
                        changed = true;
                    }
                });
            }
        });
        let enabled = toggle.is_none_or(|t| fx.on(t));
        ui.add_enabled_ui(enabled, |ui| {
            for &id in controls {
                changed |= control(ui, fx, id);
            }
        });
        if title == "Pitch" && fx.on(P::PitchOn) {
            ui.label(RichText::new("Adds ~15 ms latency.").small());
        }
    });
    changed
}

/// Draws every effect; returns true if any value changed.
pub fn panel(ui: &mut Ui, fx: &mut Snapshot) -> bool {
    let columns = ((ui.available_width() / 330.0).floor() as usize).clamp(1, 3);
    let mut changed = false;
    ui.columns(columns, |cols| {
        for (i, (title, toggle, controls)) in SECTIONS.iter().enumerate() {
            let ui = &mut cols[i % columns];
            changed |= section(ui, fx, title, *toggle, controls);
            ui.add_space(8.0);
        }
    });
    changed
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
                .any(|(_, t, c)| *t == Some(info.id) || c.contains(&info.id))
                || super::super::melody::CONTROLS.contains(&info.id);
            assert!(shown, "{} has no control", info.key);
        }
    }
}
