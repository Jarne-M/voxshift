//! Soundboard grid. Drawing only reads the board; changes come back as actions.

use super::theme::{ACCENT, BAD, CARD};
use crate::audio::shared::Shared;
use crate::soundboard::hotkeys::Hotkey;
use crate::soundboard::library::{Board, LoadState};
use crate::soundboard::mixer::SoundId;
use crate::voice_keys::KeyTarget;
use egui::{Color32, RichText, Ui, vec2};

pub enum Action {
    AddFiles,
    Play(SoundId),
    Stop(SoundId),
    StopAll,
    Remove(SoundId),
    Volume(SoundId, f32),
    BeginHotkey(SoundId),
    CancelHotkey,
    ClearHotkey(SoundId),
    MonitorSounds(bool),
    /// Assign / clear the Stop all or "Also play in my headphones" hotkey (the same ones as
    /// under Voice changer hotkeys).
    BeginActionKey(KeyTarget),
    ClearActionKey(KeyTarget),
}

const CELL: f32 = 190.0;

pub struct View<'a> {
    pub board: &'a Board,
    pub shared: &'a Shared,
    /// The hotkey waiting for a key, whatever it is.
    pub capturing: Option<KeyTarget>,
    pub hotkeys_available: bool,
    pub monitor_sounds: bool,
    pub monitor_on: bool,
    pub stop_key: Option<&'a Hotkey>,
    pub monitor_sounds_key: Option<&'a Hotkey>,
}

impl View<'_> {
    fn capturing_sound(&self) -> Option<SoundId> {
        match self.capturing {
            Some(KeyTarget::Sound(id)) => Some(id),
            _ => None,
        }
    }
}

/// The small hotkey button next to Stop all / Also play in my headphones.
fn action_key(
    ui: &mut Ui,
    v: &View,
    target: KeyTarget,
    key: Option<&Hotkey>,
    what: &str,
    actions: &mut Vec<Action>,
) {
    if v.capturing == Some(target) {
        if ui
            .small_button(RichText::new("Press a key... (Esc)").color(ACCENT))
            .on_hover_text("Press the key combination to use. Click to cancel.")
            .clicked()
        {
            actions.push(Action::CancelHotkey);
        }
        return;
    }
    let (label, tip) = match key {
        Some(k) => (
            k.label.clone(),
            format!("Hotkey for {what}: {}. Click to change.", k.label),
        ),
        None => (
            "Not set".to_string(),
            format!("No hotkey for {what}. Click, then press a key."),
        ),
    };
    if ui
        .add_enabled(v.hotkeys_available, egui::Button::new(label).small())
        .on_hover_text(tip)
        .clicked()
    {
        actions.push(Action::BeginActionKey(target));
    }
    if key.is_some()
        && ui
            .small_button("x")
            .on_hover_text("Remove hotkey")
            .clicked()
    {
        actions.push(Action::ClearActionKey(target));
    }
}

pub fn panel(ui: &mut Ui, v: &View) -> Vec<Action> {
    let mut actions = Vec::new();
    ui.horizontal(|ui| {
        if ui
            .button(RichText::new("+ Add sounds...").strong())
            .clicked()
        {
            actions.push(Action::AddFiles);
        }
        if ui
            .add(egui::Button::new(RichText::new("Stop all").color(Color32::WHITE)).fill(BAD))
            .clicked()
        {
            actions.push(Action::StopAll);
        }
        action_key(
            ui,
            v,
            KeyTarget::StopSounds,
            v.stop_key,
            "Stop all",
            &mut actions,
        );
        ui.separator();
        let mut mon = v.monitor_sounds;
        let tip = if v.monitor_on {
            "Sounds also play on the monitor output"
        } else {
            "Turn the monitor on (left panel) to hear sounds yourself"
        };
        if ui
            .checkbox(&mut mon, "Also play in my headphones")
            .on_hover_text(tip)
            .changed()
        {
            actions.push(Action::MonitorSounds(mon));
        }
        action_key(
            ui,
            v,
            KeyTarget::MonitorSoundsToggle,
            v.monitor_sounds_key,
            "\"Also play in my headphones\"",
            &mut actions,
        );
    });
    ui.label(
        RichText::new(if v.hotkeys_available {
            "Drop wav/mp3/ogg/flac files here. Hotkeys work while other apps (games) are focused, and the key still reaches them."
        } else {
            "Drop wav/mp3/ogg/flac files here. (Global hotkeys are not available on this system.)"
        })
        .small(),
    );
    ui.add_space(6.0);

    if v.board.sounds.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("No sounds yet").heading());
            ui.label("Click \"Add sounds...\" or drag audio files onto the window.");
        });
        return actions;
    }

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
        for s in &v.board.sounds {
            // Each card stacks its controls vertically inside the wrapping row.
            let layout = egui::Layout::top_down(egui::Align::Min);
            ui.allocate_ui_with_layout(vec2(CELL, 150.0), layout, |ui| {
                egui::Frame::new()
                    .fill(CARD)
                    .corner_radius(8)
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_width(CELL - 16.0);
                        cell(
                            ui,
                            v,
                            s.id,
                            &s.entry.name,
                            &s.state,
                            s.entry.volume,
                            s.entry.hotkey.as_ref(),
                            &mut actions,
                        );
                    });
            });
        }
    });
    actions
}

#[allow(clippy::too_many_arguments)]
fn cell(
    ui: &mut Ui,
    v: &View,
    id: SoundId,
    name: &str,
    state: &LoadState,
    volume: f32,
    hotkey: Option<&crate::soundboard::hotkeys::Hotkey>,
    actions: &mut Vec<Action>,
) {
    let playing = v.shared.is_playing(id);
    let (text, enabled, tip) = match state {
        LoadState::Ready(d) => (
            name.to_string(),
            true,
            format!(
                "{:.1} s - click to play, again to restart, right-click to stop",
                d.len_frames() as f32 / d.sample_rate as f32
            ),
        ),
        LoadState::Loading => (
            format!("{name}\n(loading...)"),
            false,
            "Decoding...".to_string(),
        ),
        LoadState::Failed(e) => (format!("{name}\n(failed)"), false, e.clone()),
    };
    let fill = if playing {
        ACCENT
    } else {
        Color32::from_gray(52)
    };
    let button = egui::Button::new(RichText::new(text).strong().color(Color32::WHITE))
        .fill(fill)
        .min_size(vec2(ui.available_width(), 52.0))
        .wrap();
    let r = ui.add_enabled(enabled, button).on_hover_text(tip.clone());
    let r = if enabled {
        r
    } else {
        r.on_disabled_hover_text(tip)
    };
    if r.clicked() {
        actions.push(Action::Play(id));
    }
    if r.secondary_clicked() {
        actions.push(Action::Stop(id));
    }

    let mut vol = volume;
    if ui
        .add(
            egui::Slider::new(&mut vol, 0.0..=1.5)
                .show_value(false)
                .text("vol"),
        )
        .changed()
    {
        actions.push(Action::Volume(id, vol));
    }

    ui.horizontal(|ui| {
        if v.capturing_sound() == Some(id) {
            if ui
                .button(RichText::new("Press a key... (Esc)").color(ACCENT))
                .on_hover_text("Press the key combination to use. Click to cancel.")
                .clicked()
            {
                actions.push(Action::CancelHotkey);
            }
        } else {
            let (label, tip) = match hotkey {
                Some(h) => (
                    h.label.clone(),
                    format!("Hotkey: {}. Click to change.", h.label),
                ),
                None => (
                    "Not set".to_string(),
                    "No hotkey. Click, then press a key to assign a global hotkey".to_string(),
                ),
            };
            if ui
                .add_enabled(v.hotkeys_available, egui::Button::new(label).small())
                .on_hover_text(tip)
                .clicked()
            {
                actions.push(Action::BeginHotkey(id));
            }
            if hotkey.is_some()
                && ui
                    .small_button("x")
                    .on_hover_text("Remove hotkey")
                    .clicked()
            {
                actions.push(Action::ClearHotkey(id));
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button("Delete")
                .on_hover_text("Remove from the soundboard")
                .clicked()
            {
                actions.push(Action::Remove(id));
            }
        });
    });
}
