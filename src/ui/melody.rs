//! Melody Voice: talk, and it comes out sung.
//!
//! One collapsible section at the top of the Voice page. While it is off it is a single line
//! ("Melody Voice · off · Happy Birthday · Per syllable"); switched on it opens to the
//! controls you use (mode, melody, part, transpose, retune speed, glide), the piano roll with
//! the "You: A3 » Target: C4" readout, and an Advanced fold-out for the rest.

use super::effects::control_with;
use super::icons;
use super::theme::{Palette, font, mix, palette, space};
use super::widgets::{self, card_frame};
use crate::dsp::conductor::{KEY_NAMES, Mode, Scale};
use crate::dsp::params::{P, Snapshot};
use crate::dsp::pitch_detect::note_name;
use crate::melody::{Loaded, Melody, MelodySource, file_label, library};
use egui::collapsing_header::CollapsingState;
use egui::{Align, Align2, FontId, Id, Layout, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
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

/// A slider or checkbox of this panel (its labels are longer than the effect cards').
fn control(ui: &mut Ui, fx: &mut Snapshot, id: P) -> bool {
    control_with(ui, fx, id, 150.0)
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

/// "You: A3 » Target: C4".
pub fn status_text(detected: f32, target: f32) -> String {
    let name = |m: f32| {
        if m.is_finite() {
            note_name(m)
        } else {
            "-".to_string()
        }
    };
    format!("You: {}  \u{bb}  Target: {}", name(detected), name(target))
}

/// "Note 3 of 42", or what to do at the end.
pub fn position_text(note: Option<usize>, finished: bool, total: usize) -> String {
    match (note, finished) {
        (_, true) => "End of the melody: press Restart (or turn on Loop)".to_string(),
        (Some(i), false) => format!("Note {} of {total}", i + 1),
        (None, false) => format!("Ready: {total} notes, waiting for you to start"),
    }
}

/// The one-line summary shown next to the title: "off · Happy Birthday · Per syllable".
pub fn summary(fx: &Snapshot, source: &MelodySource) -> String {
    let state = if fx.on(P::MelodyOn) { "on" } else { "off" };
    let mode = Mode::from_index(fx.choice(P::MelodyMode));
    let what = if mode == Mode::ScaleSnap {
        let key = KEY_NAMES[fx.choice(P::MelodyKey).min(KEY_NAMES.len() - 1)];
        let scale = Scale::ALL[fx.choice(P::MelodyScale).min(Scale::ALL.len() - 1)];
        format!("{key} {}", scale.label())
    } else {
        source.label()
    };
    format!("{state}  \u{b7}  {what}  \u{b7}  {}", mode.label())
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
/// Room on the left for the octave names.
const ROLL_GUTTER: f32 = 30.0;

fn is_black_key(key: i32) -> bool {
    matches!(key.rem_euclid(12), 1 | 3 | 6 | 8 | 10)
}

/// The melody on a grid of notes and beats: sung notes dimmed, the current one lit in the
/// accent colour, Melody Voice's playhead (a line with a marker) and "Play melody"'s (green).
fn piano_roll(
    ui: &mut Ui,
    p: &Palette,
    m: &Melody,
    current: Option<usize>,
    head: f32,
    preview: Option<f32>,
) {
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
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 108.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6.0, p.sunken);
    let grid = Rect::from_min_max(
        pos2(rect.left() + ROLL_GUTTER, rect.top() + 4.0),
        pos2(rect.right() - 4.0, rect.bottom() - 4.0),
    );
    let row = grid.height() / (hi - lo + 1.0);
    let x = |b: f32| grid.left() + (b - start) / span * grid.width();
    let y = |k: f32| grid.bottom() - (k - lo + 0.5) * row;

    // Rows: the black keys a shade apart, a line and a name at every C.
    for key in lo.floor() as i32..=hi.ceil() as i32 {
        let k = key as f32;
        if k < lo || k > hi {
            continue;
        }
        let band = Rect::from_min_max(
            pos2(grid.left(), y(k) - row / 2.0),
            pos2(grid.right(), y(k) + row / 2.0),
        );
        if is_black_key(key) {
            painter.rect_filled(band, 0.0, mix(p.sunken, p.text, 0.035));
        }
        if key.rem_euclid(12) == 0 {
            painter.hline(
                grid.x_range(),
                band.bottom(),
                Stroke::new(1.0, mix(p.sunken, p.text, 0.16)),
            );
            painter.text(
                pos2(rect.left() + 6.0, y(k)),
                Align2::LEFT_CENTER,
                note_name(k),
                FontId::proportional(font::SMALL - 2.0),
                p.text_faint,
            );
        }
    }
    // Beats: a faint line each, a clearer one every four.
    let grid_painter = painter.with_clip_rect(grid);
    for beat in start.floor() as i32..=(start + span).ceil() as i32 {
        let strength = if beat.rem_euclid(4) == 0 { 0.16 } else { 0.06 };
        grid_painter.vline(
            x(beat as f32),
            grid.y_range(),
            Stroke::new(1.0, mix(p.sunken, p.text, strength)),
        );
    }
    for (i, n) in m.notes.iter().enumerate() {
        if n.end() < start || n.start > start + span {
            continue;
        }
        let k = f32::from(n.key);
        let half = (row * 0.42).max(1.5);
        let bar = Rect::from_min_max(
            pos2(x(n.start), y(k) - half),
            pos2((x(n.end()) - 1.5).max(x(n.start) + 2.0), y(k) + half),
        );
        let sung = current.is_some_and(|c| i < c);
        if current == Some(i) {
            grid_painter.rect_filled(bar.expand(2.5), 4.0, p.accent.gamma_multiply(0.28));
            grid_painter.rect_filled(bar, 2.0, p.accent);
        } else {
            let fill = mix(p.sunken, p.text, if sung { 0.22 } else { 0.5 });
            grid_painter.rect_filled(bar, 2.0, fill);
        }
    }
    for (beat, color) in [(Some(head), p.text), (preview, p.success)] {
        if let Some(b) = beat
            && (start..=start + span).contains(&b)
        {
            let px = x(b);
            grid_painter.vline(px, grid.y_range(), Stroke::new(1.5, color));
            // A marker on top, so the playhead is found at a glance.
            grid_painter.add(egui::Shape::convex_polygon(
                vec![
                    pos2(px - 4.0, grid.top()),
                    pos2(px + 4.0, grid.top()),
                    pos2(px, grid.top() + 6.0),
                ],
                color,
                Stroke::NONE,
            ));
        }
    }
    painter.rect_stroke(
        rect,
        6.0,
        Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );
    response.on_hover_text(format!(
        "{}: {} notes, {:.0} beats, {} to {}. The line is where Melody Voice is; the green \
         line is \"Play melody\".",
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

/// Mode, melody and part; the piano roll with its transport; and the sliders used most.
fn basic(ui: &mut Ui, fx: &mut Snapshot, v: &View, r: &mut Response) {
    let p = palette(ui);
    let mode = Mode::from_index(fx.choice(P::MelodyMode));
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
                .button(format!("{}  Import MIDI\u{2026}", icons::IMPORT))
                .on_hover_text("Use your own .mid file (or drop one on the window)")
                .clicked()
            {
                r.actions.push(Action::Import);
            }
        }
    });
    if mode == Mode::ScaleSnap {
        widgets::hint(
            ui,
            "No melody: your voice is pulled to the nearest note of this key and scale.",
        );
    } else {
        part_picker(ui, v, &mut r.actions);
        if let Some(loaded) = v.melody {
            piano_roll(ui, p, &loaded.melody, v.note, v.beat, v.preview);
        }
        ui.horizontal_wrapped(|ui| {
            let (text, play) = if v.preview.is_some() {
                (format!("{}  Stop melody", icons::STOP), false)
            } else {
                (format!("{}  Play melody", icons::PLAY), true)
            };
            if ui
                .button(text)
                .on_hover_text(
                    "Hear the tune (at the tempo slider's speed) in your headphones only; it \
                     never goes into your mic",
                )
                .clicked()
            {
                r.actions.push(Action::Preview(play));
            }
            if ui
                .button(format!("{}  Restart", icons::RESTORE))
                .on_hover_text("Back to the first note")
                .clicked()
            {
                r.actions.push(Action::Restart);
            }
            if ui
                .button(format!("{}  Next note", icons::NEXT))
                .on_hover_text("Step to the next note (in manual mode this is how it moves)")
                .clicked()
            {
                r.actions.push(Action::Next);
            }
            if let Some(m) = v.melody {
                ui.label(
                    RichText::new(position_text(v.note, v.finished, m.melody.notes.len()))
                        .small()
                        .color(p.text_muted),
                );
            }
            if !v.monitor_on {
                ui.label(
                    RichText::new("Turn on the monitor (Audio setup) to hear Play melody.")
                        .small()
                        .color(p.warning),
                );
            }
        });
    }
    ui.add_space(space::XS);
    let columns = if ui.available_width() > 640.0 { 2 } else { 1 };
    ui.columns(columns, |cols| {
        let ui = &mut cols[0];
        for id in [P::MelodyTranspose, P::MelodyRetune, P::MelodyGlide] {
            r.changed |= control(ui, fx, id);
        }
        let ui = &mut cols[columns - 1];
        if mode == Mode::Tempo {
            r.changed |= control(ui, fx, P::MelodyBpm);
            widgets::hint(
                ui,
                "The song starts when you start singing and plays on at this tempo; notes you \
                 are silent for pass by.",
            );
        }
        if mode != Mode::ScaleSnap {
            for id in [P::MelodyLoop, P::MelodyFold] {
                r.changed |= control(ui, fx, id);
            }
        }
    });
}

/// The settings you rarely touch.
fn advanced(ui: &mut Ui, fx: &mut Snapshot, r: &mut Response) {
    let mode = Mode::from_index(fx.choice(P::MelodyMode));
    let columns = if ui.available_width() > 640.0 { 2 } else { 1 };
    ui.columns(columns, |cols| {
        let ui = &mut cols[0];
        for id in [P::MelodyWindow, P::MelodyMinHz, P::MelodyMaxHz] {
            r.changed |= control(ui, fx, id);
        }
        // Only the per-syllable mode listens for syllables.
        ui.add_enabled_ui(mode == Mode::PerSyllable, |ui| {
            for id in [P::MelodySensitivity, P::MelodyMinNote] {
                r.changed |= control(ui, fx, id);
            }
        })
        .response
        .on_disabled_hover_text("Used in Per syllable mode");
        let ui = &mut cols[columns - 1];
        r.changed |= control(ui, fx, P::MelodyFormant);
        ui.add_enabled_ui(fx.on(P::MelodyFormant), |ui| {
            r.changed |= control(ui, fx, P::MelodyFormantShift);
        });
        r.changed |= control(ui, fx, P::MelodyMix);
        ui.add_enabled_ui(mode != Mode::ScaleSnap, |ui| {
            r.changed |= control(ui, fx, P::MelodyResetAfter);
        });
    });
}

fn card_open(ui: &Ui, id: Id, on: bool) -> bool {
    ui.data(|d| d.get_temp::<bool>(id.with("by_hand")))
        .unwrap_or(on)
}

pub fn panel(ui: &mut Ui, fx: &mut Snapshot, v: &View) -> Response {
    let p = palette(ui);
    let mut r = Response::default();
    let id = Id::new("melody_card");
    let on = fx.on(P::MelodyOn);
    let open = card_open(ui, id, on);
    let mut state = CollapsingState::load_with_default_open(ui.ctx(), id, open);
    state.set_open(open);
    let set_by_hand = |ui: &Ui, open: Option<bool>| {
        ui.data_mut(|d| match open {
            Some(open) => {
                d.insert_temp(id.with("by_hand"), open);
            }
            None => d.remove::<bool>(id.with("by_hand")),
        });
    };
    let border = if on {
        mix(p.border, p.accent, 0.4)
    } else {
        p.border
    };
    let shown = card_frame(p)
        .stroke(Stroke::new(1.0, border))
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let row = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), 28.0));
            let header = ui
                .interact(row, id.with("header"), Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if header.clicked() {
                set_by_hand(ui, Some(!open));
            }
            ui.horizontal(|ui| {
                ui.set_height(28.0);
                ui.spacing_mut().item_spacing.x = space::S;
                let (icon_color, name_color) = if on {
                    (p.accent, p.text)
                } else {
                    (p.text_faint, p.text_muted)
                };
                ui.label(RichText::new(icons::MELODY).size(18.0).color(icon_color));
                ui.label(
                    RichText::new("Melody Voice")
                        .size(font::SECTION)
                        .strong()
                        .color(name_color),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let arrow = if open { icons::COLLAPSE } else { icons::EXPAND };
                    let tip = if open { "Close" } else { "Show the controls" };
                    if widgets::icon_button(ui, arrow, tip).clicked() {
                        set_by_hand(ui, Some(!open));
                    }
                    let mut switch = on;
                    let toggle = widgets::toggle(ui, &mut switch)
                        .on_hover_text("Talk, and it comes out sung to a melody");
                    #[cfg(test)]
                    widgets::remember(ui.ctx(), "melody", "toggle", toggle.rect);
                    if toggle.changed() {
                        fx.set(P::MelodyOn, if switch { 1.0 } else { 0.0 });
                        r.changed = true;
                        set_by_hand(ui, None); // on opens it, off closes it
                    }
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(summary(fx, v.source))
                                    .small()
                                    .color(p.text_muted),
                            )
                            .truncate(),
                        );
                    });
                });
            });
            state.show_body_unindented(ui, |ui| {
                ui.add_space(space::XS);
                if !on {
                    // Still adjustable, but visibly not in use.
                    ui.multiply_opacity(0.6);
                }
                if on {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(status_text(v.detected, v.target))
                                .strong()
                                .size(18.0)
                                .color(p.text),
                        );
                        ui.label(
                            RichText::new(format!(
                                "Adds about {:.0} ms of delay, more than any other effect.",
                                v.latency_ms
                            ))
                            .small()
                            .color(p.warning),
                        );
                    });
                }
                basic(ui, fx, v, &mut r);
                ui.add_space(space::XS);
                egui::CollapsingHeader::new(RichText::new("Advanced").color(p.text_muted))
                    .id_salt("melody_advanced")
                    .show(ui, |ui| advanced(ui, fx, &mut r))
                    .header_response
                    .on_hover_text(
                        "Analysis window, pitch range, syllable detection, formants, dry/wet and \
                     the restart after silence",
                    );
            });
        });
    #[cfg(test)]
    widgets::remember(ui.ctx(), "melody", "card", shown.response.rect);
    let _ = shown;
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fonts_have_every_special_character_the_panel_uses() {
        let ctx = egui::Context::default();
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .textures_delta
            .clear();
        let font = egui::FontId::proportional(14.0);
        // (The default fonts have no arrows and no "●": "→" would show as a box, hence "»".)
        // This also covers the stars and the middle dot used across the UI.
        let candidates = [
            '\u{bb}', '\u{2605}', '\u{2606}', '\u{2014}', '\u{2026}', '\u{fc}', '\u{e8}', '\u{b7}',
            '\u{2013}',
        ];
        let missing: Vec<char> = candidates
            .into_iter()
            .filter(|&c| !ctx.fonts_mut(|f| f.has_glyph(&font, c)))
            .collect();
        assert!(missing.is_empty(), "no glyph for {missing:?}");
    }

    #[test]
    fn texts() {
        assert_eq!(status_text(57.2, 60.0), "You: A3  \u{bb}  Target: C4");
        assert_eq!(status_text(f32::NAN, f32::NAN), "You: -  \u{bb}  Target: -");
        assert_eq!(position_text(Some(2), false, 42), "Note 3 of 42");
        assert!(position_text(None, false, 42).starts_with("Ready"));
        assert!(position_text(Some(41), true, 42).contains("Restart"));
    }

    #[test]
    fn the_collapsed_line_says_what_is_set_up() {
        let mut fx = Snapshot::defaults();
        let birthday = MelodySource::BuiltIn("Happy Birthday".into());
        assert_eq!(
            summary(&fx, &birthday),
            "off  \u{b7}  Happy Birthday  \u{b7}  Per syllable"
        );
        fx.set(P::MelodyOn, 1.0);
        fx.set(P::MelodyMode, 3.0); // scale snap: no melody, a key and a scale
        fx.set(P::MelodyKey, 2.0);
        let text = summary(&fx, &birthday);
        assert!(text.starts_with("on  \u{b7}  D "), "{text}");
        assert!(!text.contains("Happy Birthday"), "{text}");
        assert!(is_black_key(61) && !is_black_key(60) && is_black_key(-2));
    }
}
