//! The Melody Voice panel: talk, and it comes out sung.

use super::effects::control;
use super::theme::{ACCENT, WARN, card};
use crate::dsp::conductor::{KEY_NAMES, Mode, Scale};
use crate::dsp::params::{P, Snapshot};
use crate::dsp::pitch_detect::note_name;
use crate::melody::{Loaded, MelodySource, file_label, library};
use egui::{RichText, Ui};
use std::path::PathBuf;

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

/// What the panel shows besides the sliders.
pub struct View<'a> {
    /// Your pitch and the target note (MIDI numbers, NaN = none).
    pub detected: f32,
    pub target: f32,
    /// Delay Melody Voice adds, in ms.
    pub latency_ms: f32,
    /// Current note (None = before the first).
    pub note: Option<usize>,
    pub finished: bool,
    pub melody: Option<&'a Loaded>,
    pub source: &'a MelodySource,
    /// Imported MIDI files.
    pub files: &'a [PathBuf],
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Select(MelodySource),
    /// Open a file dialog for a MIDI file.
    Import,
    Restart,
}

#[derive(Default)]
pub struct Response {
    /// A parameter changed.
    pub changed: bool,
    pub actions: Vec<Action>,
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
pub fn status_text(detected: f32, target: f32) -> String {
    let name = |m: f32| {
        if m.is_finite() {
            note_name(m)
        } else {
            "-".to_string()
        }
    };
    format!(
        "You: {}  \u{2192}  Target: {}",
        name(detected),
        name(target)
    )
}

/// "Note 3 of 42", or what to do at the end.
pub fn position_text(note: Option<usize>, finished: bool, total: usize) -> String {
    match (note, finished) {
        (_, true) => "End of the melody: press Restart (or turn on Loop)".to_string(),
        (Some(i), false) => format!("Note {} of {total}", i + 1),
        (None, false) => format!("Ready: {total} notes, waiting for you to start"),
    }
}

fn melody_picker(ui: &mut Ui, v: &View, actions: &mut Vec<Action>) {
    let selected = v.source.label();
    egui::ComboBox::from_id_salt("melody_source")
        .width(240.0)
        .selected_text(&selected)
        .show_ui(ui, |ui| {
            ui.label(RichText::new("Built-in").small().weak());
            for b in &library::BUILT_IN {
                let source = MelodySource::BuiltIn(b.name.to_string());
                if ui.selectable_label(*v.source == source, b.name).clicked() {
                    actions.push(Action::Select(source));
                }
            }
            if !v.files.is_empty() {
                ui.separator();
                ui.label(RichText::new("Your MIDI files").small().weak());
                for path in v.files {
                    let current =
                        matches!(v.source, MelodySource::File { path: p, .. } if p == path);
                    if ui
                        .selectable_label(current, file_label(path))
                        .on_hover_text(path.display().to_string())
                        .clicked()
                        && !current
                    {
                        actions.push(Action::Select(MelodySource::File {
                            path: path.clone(),
                            part: None,
                        }));
                    }
                }
            }
        });
}

/// For MIDI files with several parts: which one to sing.
fn part_picker(ui: &mut Ui, v: &View, actions: &mut Vec<Action>) {
    let (Some(loaded), MelodySource::File { path, .. }) = (v.melody, v.source) else {
        return;
    };
    let (Some(song), Some(current)) = (&loaded.song, loaded.part) else {
        return;
    };
    if song.parts.len() < 2 {
        return;
    }
    ui.horizontal(|ui| {
        ui.label("Part");
        let describe = |i: usize| {
            let mark = if i == song.default_part {
                "  \u{2605} melody?"
            } else {
                ""
            };
            format!("{}{mark}", song.parts[i].describe())
        };
        egui::ComboBox::from_id_salt("melody_part")
            .width(360.0)
            .selected_text(describe(current))
            .show_ui(ui, |ui| {
                for i in 0..song.parts.len() {
                    if ui.selectable_label(i == current, describe(i)).clicked() && i != current {
                        actions.push(Action::Select(MelodySource::File {
                            path: path.clone(),
                            part: Some(i),
                        }));
                    }
                }
            });
    })
    .response
    .on_hover_text(
        "\u{2605} = the part VoxShift guessed is the tune (the highest one that plays one note \
         at a time). Chords are reduced to their top note.",
    );
}

pub fn panel(ui: &mut Ui, fx: &mut Snapshot, v: &View) -> Response {
    let mut r = Response::default();
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
                    r.changed = true;
                }
            });
        });
        if on {
            ui.label(
                RichText::new(status_text(v.detected, v.target))
                    .strong()
                    .size(16.0),
            );
            ui.label(
                RichText::new(format!(
                    "Adds ~{:.0} ms of delay, more than any other effect.",
                    v.latency_ms
                ))
                .small()
                .color(WARN),
            );
        }
        let mode = Mode::from_index(fx.choice(P::MelodyMode));
        ui.add_enabled_ui(on, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Mode");
                r.changed |= choice(
                    ui,
                    fx,
                    P::MelodyMode,
                    &Mode::ALL,
                    |m| m.label().into(),
                    190.0,
                );
                if mode == Mode::ScaleSnap {
                    ui.label("Key");
                    r.changed |= choice(ui, fx, P::MelodyKey, &KEY_NAMES, |k| k.into(), 50.0);
                    r.changed |= choice(
                        ui,
                        fx,
                        P::MelodyScale,
                        &Scale::ALL,
                        |s| s.label().into(),
                        140.0,
                    );
                } else {
                    ui.label("Melody");
                    melody_picker(ui, v, &mut r.actions);
                    if ui
                        .button("Import MIDI\u{2026}")
                        .on_hover_text("Use your own .mid file (or drop one on the window)")
                        .clicked()
                    {
                        r.actions.push(Action::Import);
                    }
                    if ui
                        .button("Restart")
                        .on_hover_text("Back to the first note")
                        .clicked()
                    {
                        r.actions.push(Action::Restart);
                    }
                }
            });
            if mode != Mode::ScaleSnap {
                part_picker(ui, v, &mut r.actions);
            }
            if mode != Mode::ScaleSnap
                && let Some(m) = v.melody
            {
                ui.label(
                    RichText::new(position_text(v.note, v.finished, m.melody.notes.len())).small(),
                );
            }
            let columns = if ui.available_width() > 640.0 { 2 } else { 1 };
            ui.columns(columns, |cols| {
                let ui = &mut cols[0];
                for id in [P::MelodyTranspose, P::MelodyRetune, P::MelodyGlide] {
                    r.changed |= control(ui, fx, id);
                }
                ui.label(
                    RichText::new("Retune speed: 0 = hard \"robot\" tune, 150+ = natural")
                        .small()
                        .weak(),
                );
                if mode != Mode::ScaleSnap {
                    for id in [
                        P::MelodyLoop,
                        P::MelodyFold,
                        P::MelodySensitivity,
                        P::MelodyMinNote,
                        P::MelodyResetAfter,
                    ] {
                        r.changed |= control(ui, fx, id);
                    }
                }
                let ui = &mut cols[columns - 1];
                r.changed |= control(ui, fx, P::MelodyFormant);
                ui.add_enabled_ui(fx.on(P::MelodyFormant), |ui| {
                    r.changed |= control(ui, fx, P::MelodyFormantShift);
                });
                for id in [
                    P::MelodyMix,
                    P::MelodyWindow,
                    P::MelodyMinHz,
                    P::MelodyMaxHz,
                ] {
                    r.changed |= control(ui, fx, id);
                }
            });
        });
    });
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts() {
        assert_eq!(status_text(57.2, 60.0), "You: A3  \u{2192}  Target: C4");
        assert_eq!(
            status_text(f32::NAN, f32::NAN),
            "You: -  \u{2192}  Target: -"
        );
        assert_eq!(position_text(Some(2), false, 42), "Note 3 of 42");
        assert!(position_text(None, false, 42).starts_with("Ready"));
        assert!(position_text(Some(41), true, 42).contains("Restart"));
    }
}
