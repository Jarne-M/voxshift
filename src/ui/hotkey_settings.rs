//! "Voice changer hotkeys" settings: toggle / hold key, mute key, preset keys, feedback and
//! the start state.

use super::theme::{ACCENT, WARN, card};
use crate::config::{Config, StartVoice};
use crate::soundboard::hotkeys::Hotkey;
use crate::voice_keys::{KeyTarget, PresetHotkey, VoiceKeyMode};
use egui::{RichText, Ui};

pub enum Action {
    Capture(KeyTarget),
    CancelCapture,
    Clear(KeyTarget),
    AddPresetKey,
    RemovePresetKey(usize),
}

pub struct View {
    pub capturing: Option<KeyTarget>,
    pub hotkeys_available: bool,
    pub monitor_on: bool,
    pub preset_names: Vec<String>,
}

fn key_button(
    ui: &mut Ui,
    target: KeyTarget,
    key: Option<&Hotkey>,
    v: &View,
    actions: &mut Vec<Action>,
) {
    if v.capturing == Some(target) {
        if ui
            .button(RichText::new("Press a key... (Esc)").color(ACCENT))
            .on_hover_text("Press the key or combination (Ctrl/Alt/Shift + key). Click to cancel.")
            .clicked()
        {
            actions.push(Action::CancelCapture);
        }
        return;
    }
    let text = key.map_or("Set hotkey".to_string(), |k| k.label.clone());
    if ui
        .add_enabled(v.hotkeys_available, egui::Button::new(text))
        .on_hover_text("Click, then press the key or combination to use")
        .clicked()
    {
        actions.push(Action::Capture(target));
    }
    if key.is_some() && ui.small_button("Clear").clicked() {
        actions.push(Action::Clear(target));
    }
}

pub fn panel(ui: &mut Ui, cfg: &mut Config, v: &View) -> Vec<Action> {
    let mut actions = Vec::new();
    card(ui, |ui| {
        ui.heading("Voice changer hotkeys");
        ui.label(
            RichText::new("Work in the background and in games; the game still gets the key.")
                .small(),
        );
        ui.add_space(4.0);

        ui.label(RichText::new("Effects on/off").strong());
        ui.horizontal(|ui| {
            key_button(
                ui,
                KeyTarget::Toggle,
                cfg.voice_keys.toggle.as_ref(),
                v,
                &mut actions,
            )
        });
        let mode_label = |m: VoiceKeyMode| match m {
            VoiceKeyMode::Toggle => "Toggle (press = switch)",
            VoiceKeyMode::HoldForEffects => "Hold for effects (push-to-talk)",
            VoiceKeyMode::HoldForClean => "Hold for clean (push-to-clean)",
        };
        egui::ComboBox::from_id_salt("voice_key_mode")
            .selected_text(mode_label(cfg.voice_keys.mode))
            .width(240.0)
            .show_ui(ui, |ui| {
                for m in [
                    VoiceKeyMode::Toggle,
                    VoiceKeyMode::HoldForEffects,
                    VoiceKeyMode::HoldForClean,
                ] {
                    ui.selectable_value(&mut cfg.voice_keys.mode, m, mode_label(m));
                }
            });
        ui.add_space(4.0);

        ui.label(RichText::new("Mute / unmute mic").strong());
        ui.horizontal(|ui| {
            key_button(
                ui,
                KeyTarget::Mute,
                cfg.voice_keys.mute.as_ref(),
                v,
                &mut actions,
            )
        });
        ui.add_space(4.0);

        ui.label(RichText::new("Jump to a preset").strong());
        let mut remove = None;
        for (i, p) in cfg.voice_keys.presets.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt(("preset_key", i))
                    .selected_text(&p.preset)
                    .width(110.0)
                    .show_ui(ui, |ui| {
                        for name in &v.preset_names {
                            ui.selectable_value(&mut p.preset, name.clone(), name);
                        }
                    });
                key_button(ui, KeyTarget::Preset(i), p.hotkey.as_ref(), v, &mut actions);
                if ui
                    .small_button("x")
                    .on_hover_text("Remove this preset key")
                    .clicked()
                {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            actions.push(Action::RemovePresetKey(i));
        }
        if ui.small_button("+ Add preset hotkey").clicked() {
            actions.push(Action::AddPresetKey);
        }
        ui.add_space(4.0);

        ui.label(RichText::new("Melody Voice").strong());
        let vk = &cfg.voice_keys;
        for (target, key, label) in [
            (KeyTarget::MelodyToggle, &vk.melody_toggle, "On / off"),
            (
                KeyTarget::MelodyRestart,
                &vk.melody_restart,
                "Restart melody",
            ),
            (KeyTarget::MelodyNext, &vk.melody_next, "Next note"),
        ] {
            ui.horizontal(|ui| {
                ui.add_sized([100.0, 20.0], egui::Label::new(label));
                key_button(ui, target, key.as_ref(), v, &mut actions);
            });
        }

        ui.separator();
        ui.label(RichText::new("When it switches").strong());
        let f = &mut cfg.feedback;
        ui.checkbox(
            &mut f.cues,
            "Beep in my headphones (ON: rising, OFF: falling)",
        );
        ui.add_enabled_ui(f.cues, |ui| {
            ui.add(egui::Slider::new(&mut f.cue_volume, 0.05..=1.0).text("beep volume"));
        });
        if f.cues && !v.monitor_on {
            ui.label(
                RichText::new(
                    "Beeps play on the monitor output: turn the monitor on above (untick \
                     \"my voice\" there to hear only the beeps). They never go into your mic.",
                )
                .small()
                .color(WARN),
            );
        }
        ui.checkbox(
            &mut f.overlay,
            "Show \"Voice changer: ON/OFF\" on screen for a second",
        );

        ui.separator();
        let start_label = |s: StartVoice| match s {
            StartVoice::RememberLast => "Remember last",
            StartVoice::On => "Effects ON",
            StartVoice::Off => "Effects OFF (clean)",
        };
        ui.horizontal(|ui| {
            ui.label("At start:");
            egui::ComboBox::from_id_salt("start_voice")
                .selected_text(start_label(cfg.start_voice))
                .show_ui(ui, |ui| {
                    for s in [StartVoice::RememberLast, StartVoice::On, StartVoice::Off] {
                        ui.selectable_value(&mut cfg.start_voice, s, start_label(s));
                    }
                });
        });
    });
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
