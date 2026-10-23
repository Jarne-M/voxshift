//! The Melody Voice panel: talk, and it comes out sung.

use super::effects::control;
use super::theme::{ACCENT, ON, WARN, card};
use crate::dsp::conductor::{KEY_NAMES, Mode, Scale};
use crate::dsp::params::{P, Snapshot};
use crate::dsp::pitch_detect::note_name;
use crate::melody::{Loaded, Melody, MelodySource, file_label, library};
use egui::{Color32, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
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
    /// Melody Voice's playhead in beats.
    pub beat: f32,
    /// "Play melody" playhead while it plays.
    pub preview: Option<f32>,
    /// The monitor (headphones) output is on, so "Play melody" can be heard.
    pub monitor_on: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Select(MelodySource),
    /// Open a file dialog for a MIDI file.
    Import,
    Restart,
    Next,
    /// Start (true) or stop "Play melody" in the headphones.
    Preview(bool),
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

/// Beats shown at once in the piano roll (shorter melodies are shown whole).
const ROLL_BEATS: f32 = 24.0;

/// The melody as a strip of note bars, the current note lit, with Melody Voice's playhead
/// (white) and "Play melody"'s (green).
fn piano_roll(ui: &mut Ui, m: &Melody, current: Option<usize>, head: f32, preview: Option<f32>) {
    let total = m.beats().max(1.0);
    let span = total.min(ROLL_BEATS);
    let focus = preview.unwrap_or(head);
    let start = if total <= span {
        0.0
    } else {
        (focus - span * 0.25).clamp(0.0, total - span)
    };
    let lo = f32::from(m.lowest()) - 1.0;
    let hi = (f32::from(m.highest()) + 1.0).max(lo + 12.0);
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 72.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, Color32::from_gray(18));
    let row = rect.height() / (hi - lo + 1.0);
    let x = |b: f32| rect.left() + (b - start) / span * rect.width();
    let y = |k: f32| rect.bottom() - (k - lo + 0.5) * row;
    // A faint line at every C, to read the octaves.
    for c in (lo.ceil() as i32..=hi as i32).filter(|k| k % 12 == 0) {
        let yc = y(c as f32);
        painter.hline(rect.x_range(), yc, Stroke::new(1.0, Color32::from_gray(34)));
    }
    for (i, n) in m.notes.iter().enumerate() {
        if n.end() < start || n.start > start + span {
            continue;
        }
        let k = f32::from(n.key);
        let r = Rect::from_min_max(
            pos2(x(n.start), y(k) - row * 0.45),
            pos2((x(n.end()) - 1.5).max(x(n.start) + 2.0), y(k) + row * 0.45),
        )
        .intersect(rect);
        let fill = if current == Some(i) {
            ACCENT
        } else {
            Color32::from_gray(105)
        };
        painter.rect_filled(r, 2.0, fill);
    }
    for (beat, color) in [(Some(head), Color32::WHITE), (preview, ON)] {
        if let Some(b) = beat
            && (start..=start + span).contains(&b)
        {
            painter.vline(x(b), rect.y_range(), Stroke::new(1.5, color));
        }
    }
    response.on_hover_text(format!(
        "{}: {} notes, {:.0} beats, {} to {}. White line: where Melody Voice is. Green: \"Play \
         melody\".",
        m.name,
        m.notes.len(),
        total,
        note_name(f32::from(m.lowest())),
        note_name(f32::from(m.highest()))
    ));
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
                    if ui
                        .button("Next note")
                        .on_hover_text(
                            "Step to the next note (in manual mode this is how it moves)",
                        )
                        .clicked()
                    {
                        r.actions.push(Action::Next);
                    }
                }
            });
            if mode == Mode::Tempo {
                r.changed |= control(ui, fx, P::MelodyBpm);
                ui.label(
                    RichText::new(
                        "The song starts when you start singing and plays on at this tempo; \
                         notes you are silent for pass by.",
                    )
                    .small()
                    .weak(),
                );
            }
            if mode != Mode::ScaleSnap {
                part_picker(ui, v, &mut r.actions);
                if let Some(loaded) = v.melody {
                    piano_roll(ui, &loaded.melody, v.note, v.beat, v.preview);
                }
                ui.horizontal(|ui| {
                    let (text, play) = if v.preview.is_some() {
                        ("Stop melody", false)
                    } else {
                        ("Play melody", true)
                    };
                    if ui
                        .button(text)
                        .on_hover_text(
                            "Hear the tune (at the tempo slider's speed) in your headphones \
                             only; it never goes into your mic",
                        )
                        .clicked()
                    {
                        r.actions.push(Action::Preview(play));
                    }
                    if !v.monitor_on {
                        ui.label(
                            RichText::new("Turn on the monitor (left panel) to hear it.")
                                .small()
                                .color(WARN),
                        );
                    }
                });
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
                    for id in [P::MelodyLoop, P::MelodyFold, P::MelodyResetAfter] {
                        r.changed |= control(ui, fx, id);
                    }
                }
                if mode == Mode::PerSyllable {
                    for id in [P::MelodySensitivity, P::MelodyMinNote] {
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
