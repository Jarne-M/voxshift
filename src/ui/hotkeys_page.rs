//! The Hotkeys page: every hotkey in one place, the soundboard's clips included, grouped
//! (Safety, Voice changer, Presets, Melody Voice, Monitor & soundboard, Clips) and with a
//! filter box. Keys are shown as keycaps; assigning one goes through the same capture and
//! Keep / Change check as everywhere else. Every action can have a second key that does the
//! same (e.g. "8" next to "Ctrl + 8"), set here.

use super::icons;
use super::theme::{font, palette, space};
use super::widgets::{self, KeySlot, card, section_title};
use crate::config::Config;
use crate::keymap::Slot;
use crate::soundboard::hotkeys::Hotkey;
use crate::soundboard::mixer::SoundId;
use crate::soundboard::organize;
use crate::voice_keys::{KeyTarget, PresetHotkey, VoiceKeyMode};
use egui::{Align, Id, Key, Layout, RichText, Ui, vec2};

pub enum Action {
    Capture(KeyTarget, Slot),
    CancelCapture,
    /// Both keys.
    Clear(KeyTarget),
    /// Only the second key.
    ClearSecond(KeyTarget),
    AddPresetKey,
    RemovePresetKey(usize),
}

/// A clip as the page lists it.
pub struct Clip {
    pub id: SoundId,
    pub name: String,
    pub hotkey: Option<Hotkey>,
}

pub struct View {
    /// The key waiting to be pressed: whose, and which of its two.
    pub capturing: Option<(KeyTarget, Slot)>,
    pub hotkeys_available: bool,
    pub preset_names: Vec<String>,
    pub clips: Vec<Clip>,
}

/// One hotkey row.
pub struct Row<'a> {
    pub target: KeyTarget,
    pub label: String,
    pub tip: &'static str,
    pub key: Option<&'a Hotkey>,
}

/// The groups of the page, in order, with their rows (presets and clips included).
pub fn groups<'a>(cfg: &'a Config, v: &'a View) -> Vec<(&'static str, &'static str, Vec<Row<'a>>)> {
    let vk = &cfg.voice_keys;
    let row = |target, label: &str, tip, key: &'a Option<Hotkey>| Row {
        target,
        label: label.to_string(),
        tip,
        key: key.as_ref(),
    };
    vec![
        (
            icons::SAFETY,
            "Safety",
            vec![row(
                KeyTarget::Panic,
                "Panic (safe mode)",
                "Stops all sounds and sends only your clean mic: voice changer, effects and \
                 Melody Voice off, nothing in your settings changed. Press again to restore. \
                 Beats every other hotkey.",
                &vk.panic,
            )],
        ),
        (
            icons::VOICE,
            "Voice changer",
            vec![
                row(
                    KeyTarget::Toggle,
                    "Effects on / off",
                    "Switches the voice changer; how is set next to it",
                    &vk.toggle,
                ),
                row(KeyTarget::Mute, "Mute / unmute mic", "", &vk.mute),
            ],
        ),
        (
            icons::PRESETS,
            "Presets",
            vk.presets
                .iter()
                .enumerate()
                .map(|(i, p)| Row {
                    target: KeyTarget::Preset(i),
                    label: p.preset.clone(),
                    tip: "Loads this preset and switches the voice changer on",
                    key: p.hotkey.as_ref(),
                })
                .collect(),
        ),
        (
            icons::MELODY,
            "Melody Voice",
            vec![
                row(KeyTarget::MelodyToggle, "On / off", "", &vk.melody_toggle),
                row(
                    KeyTarget::MelodyRestart,
                    "Restart the melody",
                    "Back to the first note",
                    &vk.melody_restart,
                ),
                row(
                    KeyTarget::MelodyNext,
                    "Next note",
                    "For the Manual step mode",
                    &vk.melody_next,
                ),
            ],
        ),
        (
            icons::HEADPHONES,
            "Monitor & soundboard",
            vec![
                row(
                    KeyTarget::MonitorToggle,
                    "Hear myself",
                    "Turns \u{201c}my voice\u{201d} on the monitor on or off (your headphones \
                     only, never the virtual mic), with a beep: high = on, low = off",
                    &vk.monitor_toggle,
                ),
                row(
                    KeyTarget::MonitorSoundsToggle,
                    "Hear clips myself",
                    "Turns the soundboard in your headphones on or off (others always hear \
                     the clips), with the same beeps",
                    &vk.monitor_sounds_toggle,
                ),
                row(
                    KeyTarget::StopSounds,
                    "Stop all clips",
                    "Stops every playing clip, like the Stop all button",
                    &vk.stop_sounds,
                ),
            ],
        ),
        (
            icons::SOUNDBOARD,
            "Clips",
            v.clips
                .iter()
                .map(|c| Row {
                    target: KeyTarget::Sound(c.id),
                    label: c.name.clone(),
                    tip: "",
                    key: c.hotkey.as_ref(),
                })
                .collect(),
        ),
    ]
}

/// Whether a row is shown for this filter: its name, its group or its key.
pub fn row_matches(group: &str, row: &Row, filter: &str) -> bool {
    let key = row.key.map(Hotkey::pretty_all).unwrap_or_default();
    organize::matches(&row.label, filter)
        || organize::matches(group, filter)
        || (!key.is_empty() && organize::matches(&key, filter))
}

/// The keycap button of one of a row's two keys (`key` is that one key alone).
fn key_cell(
    ui: &mut Ui,
    target: KeyTarget,
    which: Slot,
    key: Option<&Hotkey>,
    v: &View,
    actions: &mut Vec<Action>,
) {
    if v.capturing == Some((target, which)) {
        if widgets::hotkey_button(ui, KeySlot::Capturing)
            .on_hover_text("Press the key or combination (Ctrl/Alt/Shift + key). Click to cancel.")
            .clicked()
        {
            actions.push(Action::CancelCapture);
        }
        return;
    }
    let parts = key.map(Hotkey::parts);
    let (slot, hover) = match (&parts, which) {
        (Some(p), Slot::Main) => (
            KeySlot::Set(p),
            "Click, then press another key or combination",
        ),
        (Some(p), Slot::Second) => (
            KeySlot::Set(p),
            "The second key: does the same. Click, then press another key or combination",
        ),
        (None, Slot::Main) => (
            KeySlot::Empty,
            "No hotkey. Click, then press the key or combination to use",
        ),
        (None, Slot::Second) => (
            KeySlot::Prompt("+ Second key"),
            "Another key that does the same, e.g. 8 next to Ctrl + 8 (so it still works \
             while you hold Ctrl in a game). Click, then press it",
        ),
    };
    let r = ui
        .add_enabled_ui(v.hotkeys_available, |ui| {
            widgets::hotkey_button(ui, slot).on_hover_text(hover)
        })
        .inner;
    #[cfg(test)]
    {
        let name = match which {
            Slot::Main => format!("{target:?}"),
            Slot::Second => format!("{target:?} second"),
        };
        widgets::remember(ui.ctx(), "hotkey", name, r.rect);
    }
    if r.clicked() {
        actions.push(Action::Capture(target, which));
    }
}

const LABEL_W: f32 = 230.0;
const ROW_H: f32 = 32.0;
/// The main key's column (wide enough for "Ctrl + Shift + F8"; longer ones push on).
const MAIN_KEY_W: f32 = 150.0;

/// One row: the action's name (or, for a preset key, which preset), its key, and Clear.
fn row_ui(
    ui: &mut Ui,
    cfg_presets: &mut [PresetHotkey],
    r: &Row,
    v: &View,
    actions: &mut Vec<Action>,
) {
    let p = palette(ui);
    ui.horizontal(|ui| {
        ui.set_min_height(ROW_H);
        let label_w = LABEL_W.min(ui.available_width() * 0.45);
        match r.target {
            KeyTarget::Preset(i) if i < cfg_presets.len() => {
                let preset = &mut cfg_presets[i].preset;
                // In a box as wide as the other rows' labels, so every key starts in one column.
                ui.allocate_ui_with_layout(
                    vec2(label_w, ROW_H),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.set_min_size(vec2(label_w, ROW_H));
                        egui::ComboBox::from_id_salt(("preset_key", i))
                            .selected_text(preset.as_str())
                            .width(label_w - 16.0)
                            .show_ui(ui, |ui| {
                                for name in &v.preset_names {
                                    ui.selectable_value(preset, name.clone(), name);
                                }
                            })
                            .response
                            .on_hover_text(r.tip);
                    },
                );
            }
            _ => {
                let label = widgets::column_label(
                    ui,
                    label_w,
                    ROW_H,
                    RichText::new(&r.label).color(p.text),
                );
                if !r.tip.is_empty() {
                    label.on_hover_text(r.tip);
                }
            }
        }
        let main = r.key.map(Hotkey::single);
        // In a column of its own, so the second keys line up too.
        ui.allocate_ui_with_layout(
            vec2(MAIN_KEY_W, ROW_H),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_min_size(vec2(MAIN_KEY_W, ROW_H));
                key_cell(ui, r.target, Slot::Main, main.as_ref(), v, actions);
            },
        );
        // The second key: offered once there is a main key (or while it waits for one).
        let second = r.key.and_then(|k| k.second.as_deref());
        let capturing_second = v.capturing == Some((r.target, Slot::Second));
        if r.key.is_some() || capturing_second {
            ui.label(RichText::new("or").color(p.text_muted));
            key_cell(ui, r.target, Slot::Second, second, v, actions);
            if second.is_some()
                && !capturing_second
                && widgets::icon_button(ui, icons::CLOSE, "Remove the second key").clicked()
            {
                actions.push(Action::ClearSecond(r.target));
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let KeyTarget::Preset(i) = r.target
                && widgets::icon_button(ui, icons::DELETE, "Remove this preset key").clicked()
            {
                actions.push(Action::RemovePresetKey(i));
            }
            let tip = if second.is_some() {
                "Clear both keys"
            } else {
                "Clear this hotkey"
            };
            if r.key.is_some() && widgets::icon_button(ui, icons::CLOSE, tip).clicked() {
                actions.push(Action::Clear(r.target));
            }
        });
    });
}

/// Extra controls that belong to a group (only without a filter, so they never hide rows).
fn group_extras(ui: &mut Ui, name: &str, cfg: &mut Config, actions: &mut Vec<Action>) {
    let p = palette(ui);
    match name {
        "Safety" => {
            ui.horizontal(|ui| {
                widgets::toggle(ui, &mut cfg.feedback.panic_mutes_monitor);
                ui.label("Panic also mutes my headphones").on_hover_text(
                    "\u{201c}Hear myself\u{201d} and the soundboard in your headphones go \
                     quiet until you restore",
                );
            });
        }
        "Voice changer" => {
            ui.horizontal(|ui| {
                let label_w = LABEL_W.min(ui.available_width() * 0.45);
                widgets::column_label(
                    ui,
                    label_w,
                    ROW_H,
                    RichText::new("The effects key").color(p.text_muted),
                );
                widgets::segmented(
                    ui,
                    &mut cfg.voice_keys.mode,
                    &[
                        (VoiceKeyMode::Toggle, "Switches"),
                        (VoiceKeyMode::HoldForEffects, "Hold for effects"),
                        (VoiceKeyMode::HoldForClean, "Hold for clean"),
                    ],
                );
            });
        }
        "Presets" => {
            let add = ui
                .button(
                    RichText::new(format!("{}  Add a preset key", icons::ADD)).color(p.text_muted),
                )
                .on_hover_text("A hotkey that loads one preset");
            if add.clicked() {
                actions.push(Action::AddPresetKey);
            }
        }
        _ => {}
    }
}

pub fn page(ui: &mut Ui, cfg: &mut Config, v: &View, filter: &mut String) -> Vec<Action> {
    let p = palette(ui);
    let mut actions = Vec::new();
    if !v.hotkeys_available {
        widgets::notice(
            ui,
            icons::WARNING,
            "Global hotkeys are not available on this system.",
            widgets::Tone::Warning,
        );
    }
    let (set, total) = {
        let gs = groups(cfg, v);
        let rows = gs.iter().flat_map(|g| &g.2);
        (
            rows.clone().filter(|r| r.key.is_some()).count(),
            rows.count(),
        )
    };
    ui.horizontal(|ui| {
        let id = Id::new("hotkeys_filter");
        let r = ui.add(
            egui::TextEdit::singleline(filter)
                .id(id)
                .hint_text(format!(
                    "{}  Filter by action, clip or key (Ctrl+F)",
                    icons::SEARCH
                ))
                .desired_width(320.0),
        );
        if ui.input(|i| i.modifiers.command && i.key_pressed(Key::F)) {
            r.request_focus();
        }
        if (r.has_focus() || r.lost_focus()) && ui.input(|i| i.key_pressed(Key::Escape)) {
            filter.clear();
        }
        if !filter.is_empty()
            && widgets::icon_button(ui, icons::CLOSE, "Clear the filter").clicked()
        {
            filter.clear();
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(format!("{set} of {total} set")).color(p.text_muted));
        });
    });
    ui.add_space(space::S);

    let filtering = !filter.trim().is_empty();
    // (The rows borrow the config; the extras below change it. Collect what to draw first.)
    let shown: Vec<(&'static str, &'static str, Vec<usize>, usize)> = groups(cfg, v)
        .iter()
        .map(|(icon, name, rows)| {
            let keep = rows
                .iter()
                .enumerate()
                .filter(|(_, r)| !filtering || row_matches(name, r, filter))
                .map(|(i, _)| i)
                .collect();
            (*icon, *name, keep, rows.len())
        })
        .collect();
    let mut any = false;
    for (icon, name, keep, count) in shown {
        if filtering && keep.is_empty() {
            continue;
        }
        any = true;
        card(ui, |ui| {
            ui.horizontal(|ui| {
                section_title(ui, icon, name);
                if name == "Clips" {
                    widgets::hint(ui, "Also set from a clip's menu on the Soundboard page");
                }
            });
            if count == 0 {
                let empty = match name {
                    "Presets" => "No preset keys yet.",
                    _ => "No clips yet: add some on the Soundboard page.",
                };
                widgets::hint(ui, empty);
            }
            // Rebuilt here: `groups` borrows the config only for this group's rows.
            let mut presets = cfg.voice_keys.presets.clone();
            {
                let gs = groups(cfg, v);
                let rows = &gs.iter().find(|g| g.1 == name).expect("group").2;
                for &i in &keep {
                    row_ui(ui, &mut presets, &rows[i], v, &mut actions);
                }
            }
            for (i, p) in presets.into_iter().enumerate() {
                if let Some(slot) = cfg.voice_keys.presets.get_mut(i) {
                    slot.preset = p.preset;
                }
            }
            if !filtering {
                ui.add_space(space::XS);
                group_extras(ui, name, cfg, &mut actions);
            }
        });
        ui.add_space(space::M);
    }
    if !any {
        ui.add_space(space::XL);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("No hotkey matches the filter.")
                    .size(font::BODY)
                    .color(p.text_muted),
            );
        });
    }
    actions
}

/// A new preset-key row, defaulting to the first preset that has none yet.
pub fn new_preset_key(existing: &[PresetHotkey], names: &[String]) -> PresetHotkey {
    let preset = names
        .iter()
        .find(|n| !existing.iter().any(|p| &p.preset == *n))
        .or(names.first())
        .cloned()
        .unwrap_or_default();
    PresetHotkey {
        preset,
        hotkey: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_hotkey_has_a_row_and_the_filter_finds_names_groups_and_keys() {
        let mut cfg = Config::default();
        let f8 = Hotkey::new(0x77, true, false, false, false, "");
        cfg.voice_keys.panic = Some(Hotkey::new(
            0x67,
            false,
            false,
            false,
            false,
            "7 (PAVE NUM.)",
        ));
        cfg.voice_keys.toggle = Some(f8.clone());
        cfg.voice_keys.presets.push(PresetHotkey {
            preset: "Robot".into(),
            hotkey: None,
        });
        let v = View {
            capturing: None,
            hotkeys_available: true,
            preset_names: vec!["Robot".into()],
            clips: vec![
                Clip {
                    id: 3,
                    name: "Écoute".into(),
                    hotkey: None,
                },
                Clip {
                    id: 4,
                    name: "Airhorn".into(),
                    hotkey: Some(Hotkey::new(0x61, false, false, false, false, "")),
                },
            ],
        };
        let gs = groups(&cfg, &v);
        let names: Vec<&str> = gs.iter().map(|g| g.1).collect();
        assert_eq!(
            names,
            [
                "Safety",
                "Voice changer",
                "Presets",
                "Melody Voice",
                "Monitor & soundboard",
                "Clips"
            ]
        );
        let all: Vec<KeyTarget> = gs
            .iter()
            .flat_map(|g| g.2.iter().map(|r| r.target))
            .collect();
        // Every voice key the keymap knows about, plus the preset and both clips.
        for (t, _, _) in cfg.voice_keys.action_keys() {
            assert!(all.contains(&t), "{t:?} has no row");
        }
        for t in [
            KeyTarget::Toggle,
            KeyTarget::Mute,
            KeyTarget::Preset(0),
            KeyTarget::Sound(3),
            KeyTarget::Sound(4),
        ] {
            assert!(all.contains(&t), "{t:?} has no row");
        }
        let find = |filter: &str| -> Vec<KeyTarget> {
            gs.iter()
                .flat_map(|(_, g, rows)| {
                    rows.iter()
                        .filter(move |r| row_matches(g, r, filter))
                        .map(|r| r.target)
                })
                .collect()
        };
        assert_eq!(find("ecou"), [KeyTarget::Sound(3)], "accents ignored");
        assert_eq!(find("num 7"), [KeyTarget::Panic], "by its pretty key name");
        assert_eq!(find("ctrl + f8"), [KeyTarget::Toggle]);
        assert_eq!(
            find("clips").len(),
            4,
            "the group's two clips, Stop all clips and Hear clips myself"
        );
        assert!(find("zzz").is_empty());
        // A second key is found too.
        cfg.voice_keys.mute = Hotkey::pair(
            Some(Hotkey::new(0x4D, true, false, false, false, "")),
            Some(Hotkey::new(0x7A, false, false, false, false, "")),
        );
        let gs = groups(&cfg, &v);
        let found: Vec<KeyTarget> = gs
            .iter()
            .flat_map(|(_, g, rows)| {
                rows.iter()
                    .filter(move |r| row_matches(g, r, "f11"))
                    .map(|r| r.target)
            })
            .collect();
        assert_eq!(found, [KeyTarget::Mute]);
    }
}
