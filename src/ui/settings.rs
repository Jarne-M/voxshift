//! The Settings page: appearance (theme, UI scale), window & startup, what happens when the
//! voice changer switches, the settings file (where it is, reset) and About.

use super::icons;
use super::theme::{self, ThemeChoice, palette, space};
use super::widgets::{self, Tone, card, section_title};
use crate::config::{CloseBehavior, Config, StartVoice};
use egui::{Color32, Id, RichText, Stroke, Ui};
use std::path::Path;

pub const REPO_URL: &str = "https://github.com/Jarne-M/voxshift";
pub const RELEASES_URL: &str = "https://github.com/Jarne-M/voxshift/releases";

/// The UI scale choices.
pub const SCALES: [(f32, &str); 5] = [
    (0.9, "90 %"),
    (1.0, "100 %"),
    (1.1, "110 %"),
    (1.25, "125 %"),
    (1.5, "150 %"),
];

pub struct View<'a> {
    /// Current state of the Run registry entry.
    pub autostart: bool,
    /// The Run entry starts a different copy of VoxShift.
    pub autostart_elsewhere: bool,
    pub tray_available: bool,
    /// The monitor is on (the beeps play there).
    pub monitor_on: bool,
    /// Where the settings are saved (none in tests).
    pub config_path: Option<&'a Path>,
}

pub enum Action {
    Autostart(bool),
    Scale(f32),
    OpenConfigFolder,
    /// Confirmed in the dialog.
    Reset,
    /// Open the setup wizard.
    Wizard,
}

fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        let text = RichText::new(label).color(palette(ui).text_muted);
        widgets::column_label(ui, 170.0, 28.0, text);
        add(ui);
    });
}

/// A switch with its label after it.
fn switch(ui: &mut Ui, on: &mut bool, label: &str) -> egui::Response {
    ui.horizontal(|ui| {
        let r = widgets::toggle(ui, on);
        ui.label(label);
        r
    })
    .inner
}

fn appearance(ui: &mut Ui, cfg: &mut Config, actions: &mut Vec<Action>) {
    card(ui, |ui| {
        section_title(ui, icons::APPEARANCE, "Appearance");
        row(ui, "Theme", |ui| {
            if widgets::segmented(
                ui,
                &mut cfg.theme,
                &ThemeChoice::ALL.map(|t| (t, t.label())),
            ) {
                theme::set_choice(ui.ctx(), cfg.theme);
            }
        });
        row(ui, "UI scale", |ui| {
            // The nearest choice is lit (Ctrl + / Ctrl - can set values in between).
            let mut scale = SCALES
                .iter()
                .map(|s| s.0)
                .min_by(|a, b| {
                    (a - cfg.ui_scale)
                        .abs()
                        .total_cmp(&(b - cfg.ui_scale).abs())
                })
                .unwrap_or(1.0);
            if widgets::segmented(ui, &mut scale, &SCALES) {
                actions.push(Action::Scale(scale));
            }
        });
        accent_row(ui, cfg);
    });
}

/// One colour of the quick picks: a round swatch, ringed when it is the one in use.
fn swatch(ui: &mut Ui, color: Color32, selected: bool, name: &str) -> egui::Response {
    let p = palette(ui);
    let size = 22.0;
    let (rect, r) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    let c = rect.center();
    let hovered = r.hovered();
    ui.painter().circle_filled(c, size / 2.0 - 3.0, color);
    if selected || hovered {
        let ring = if selected { p.text } else { p.border_strong };
        ui.painter()
            .circle_stroke(c, size / 2.0 - 0.5, Stroke::new(1.5, ring));
    }
    #[cfg(test)]
    widgets::remember(ui.ctx(), "accent", name, rect);
    r.on_hover_text(format!("{name} ({})", theme::to_hex(color)))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// "Accent colour": quick picks, a colour picker, a hex field and Reset. A change is stored in
/// the config; the app restyles everything from it on the next frame.
fn accent_row(ui: &mut Ui, cfg: &mut Config) {
    let current = theme::accent_of(&cfg.accent);
    let mut pick = None;
    row(ui, "Accent colour", |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (name, color) in theme::ACCENTS {
            if swatch(ui, color, current == color, name).clicked() {
                pick = Some(color);
            }
        }
        ui.add_space(space::S);
        let mut rgb = [current.r(), current.g(), current.b()];
        if ui
            .color_edit_button_srgb(&mut rgb)
            .on_hover_text("Any colour of your own")
            .changed()
        {
            pick = Some(Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
        }
        // While you type, the field keeps your text; otherwise it shows the colour in use.
        let id = Id::new("accent_hex");
        let mut text = ui
            .data(|d| d.get_temp::<String>(id))
            .unwrap_or_else(|| theme::to_hex(current));
        let r = ui
            .add(
                egui::TextEdit::singleline(&mut text)
                    .desired_width(72.0)
                    .char_limit(7)
                    .hint_text("#RRGGBB"),
            )
            .on_hover_text("Type or paste a colour code, like #14B8A6");
        #[cfg(test)]
        widgets::remember(ui.ctx(), "accent", "hex", r.rect);
        if r.changed()
            && let Some(c) = theme::parse_hex(&text)
        {
            pick = Some(c);
        }
        if r.has_focus() {
            ui.data_mut(|d| d.insert_temp(id, text));
        } else {
            ui.data_mut(|d| d.remove::<String>(id));
        }
        let reset = ui
            .add_enabled(current != theme::DEFAULT_ACCENT, egui::Button::new("Reset"))
            .on_hover_text("Back to teal, the default");
        #[cfg(test)]
        widgets::remember(ui.ctx(), "accent", "reset", reset.rect);
        if reset.clicked() {
            pick = Some(theme::DEFAULT_ACCENT);
        }
    });
    if let Some(c) = pick {
        cfg.accent = theme::to_hex(c);
        ui.ctx().request_repaint();
    }
    if let Some(what) = theme::resembles_status(theme::accent_of(&cfg.accent)) {
        widgets::notice(
            ui,
            icons::WARNING,
            &format!(
                "This colour looks like the {what} colour VoxShift uses for status, so \
                 \u{201c}active\u{201d} and a warning may be hard to tell apart. You can still \
                 use it."
            ),
            Tone::Warning,
        );
    }
}

fn window(ui: &mut Ui, cfg: &mut Config, v: &View, actions: &mut Vec<Action>) {
    let p = palette(ui);
    card(ui, |ui| {
        section_title(ui, icons::WINDOW, "Window & startup");
        row(ui, "Closing the window", |ui| {
            let label = |b: CloseBehavior| match b {
                CloseBehavior::Ask => "Ask every time",
                CloseBehavior::MinimizeToTray => "Minimize to the tray",
                CloseBehavior::Quit => "Quit",
            };
            egui::ComboBox::from_id_salt("close_behavior")
                .selected_text(label(cfg.close_behavior))
                .width(200.0)
                .show_ui(ui, |ui| {
                    for b in [
                        CloseBehavior::Ask,
                        CloseBehavior::MinimizeToTray,
                        CloseBehavior::Quit,
                    ] {
                        ui.selectable_value(&mut cfg.close_behavior, b, label(b));
                    }
                });
        });
        if cfg.close_behavior == CloseBehavior::Quit {
            widgets::notice(
                ui,
                icons::WARNING,
                "Closing the window then stops your virtual mic.",
                Tone::Warning,
            );
        }
        ui.add_space(space::XS);
        let mut on = v.autostart;
        if switch(ui, &mut on, "Start with Windows")
            .on_hover_text("Adds VoxShift to your user's startup programs (no admin needed)")
            .changed()
        {
            actions.push(Action::Autostart(on));
        }
        if v.autostart && v.autostart_elsewhere {
            widgets::notice(
                ui,
                icons::WARNING,
                "Windows starts a different VoxShift.exe: switch this off and on again to use \
                 this one.",
                Tone::Warning,
            );
        }
        ui.add_enabled_ui(v.tray_available, |ui| {
            switch(ui, &mut cfg.start_minimized, "Start minimized to the tray");
        });
        row(ui, "Voice changer at start", |ui| {
            let label = |s: StartVoice| match s {
                StartVoice::RememberLast => "As I left it",
                StartVoice::On => "On",
                StartVoice::Off => "Off (clean voice)",
            };
            widgets::segmented(
                ui,
                &mut cfg.start_voice,
                &[StartVoice::RememberLast, StartVoice::On, StartVoice::Off].map(|s| (s, label(s))),
            );
        });
        ui.label(
            RichText::new("The window's size and place are remembered.")
                .small()
                .color(p.text_muted),
        );
    });
}

fn feedback(ui: &mut Ui, cfg: &mut Config, v: &View) {
    card(ui, |ui| {
        section_title(ui, icons::FEEDBACK, "When the voice changer switches");
        let f = &mut cfg.feedback;
        switch(
            ui,
            &mut f.cues,
            "Beep in my headphones (on: rising, off: falling)",
        )
        .on_hover_text(
            "The \u{201c}Hear myself\u{201d} hotkey beeps too: one high beep for on, one low \
             for off. Beeps never go into your mic.",
        );
        if f.cues {
            widgets::Slider::new(&mut f.cue_volume, 0.05..=1.0)
                .label("Beep volume")
                // (In line with the other rows' controls.)
                .label_width(162.0)
                .percent(true)
                .default_value(0.5)
                .tip("How loud the switch beeps are in your headphones.")
                .show(ui);
            if !v.monitor_on {
                widgets::notice(
                    ui,
                    icons::WARNING,
                    "Beeps play on the monitor: turn the monitor on under Audio setup (untick \
                     \u{201c}my voice\u{201d} there to hear only the beeps).",
                    Tone::Warning,
                );
            }
        }
        switch(
            ui,
            &mut f.overlay,
            "Show \u{201c}Voice changer: ON / OFF\u{201d} on screen for a second",
        );
    });
}

fn settings_file(ui: &mut Ui, v: &View, actions: &mut Vec<Action>) {
    let p = palette(ui);
    let confirm = Id::new("reset_settings_confirm");
    card(ui, |ui| {
        section_title(ui, icons::FILE, "Settings file");
        if let Some(path) = v.config_path {
            ui.horizontal(|ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(path.display().to_string())
                            .monospace()
                            .color(p.text_muted),
                    )
                    .truncate(),
                );
            });
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    v.config_path.is_some(),
                    egui::Button::new(format!("{}  Open folder", icons::LOCATE)),
                )
                .on_hover_text("Opens the folder with config.json in Explorer")
                .clicked()
            {
                actions.push(Action::OpenConfigFolder);
            }
            if ui
                .button(
                    RichText::new(format!("{}  Reset settings\u{2026}", icons::RESTORE))
                        .color(p.danger),
                )
                .clicked()
            {
                ui.data_mut(|d| d.insert_temp(confirm, true));
            }
        });
    });
    if ui.data(|d| d.get_temp::<bool>(confirm)).unwrap_or(false) {
        let mut answer = None;
        let modal = widgets::modal(ui.ctx(), "reset_settings").show(ui.ctx(), |ui| {
            ui.set_width(420.0);
            ui.heading("Reset settings?");
            ui.label(
                "Devices, hotkeys, effects, the monitor, the window and these settings go \
                 back to how a fresh install starts.",
            );
            widgets::hint(
                ui,
                "Kept: your clips (with their hotkeys), folders, your presets and imported \
                 melodies.",
            );
            ui.add_space(space::S);
            ui.horizontal(|ui| {
                if widgets::primary_button(ui, "Keep my settings").clicked() {
                    answer = Some(false);
                }
                if widgets::danger_button(ui, "Reset").clicked() {
                    answer = Some(true);
                }
            });
        });
        if answer.is_none() && modal.should_close() {
            answer = Some(false);
        }
        if let Some(reset) = answer {
            ui.data_mut(|d| d.remove::<bool>(confirm));
            if reset {
                actions.push(Action::Reset);
            }
        }
    }
}

fn about(ui: &mut Ui, actions: &mut Vec<Action>) {
    let p = palette(ui);
    card(ui, |ui| {
        ui.horizontal(|ui| {
            section_title(ui, icons::ABOUT, "About");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let r = ui
                    .button(format!("{}  Setup wizard\u{2026}", icons::LOGO))
                    .on_hover_text(
                        "The three first-start steps again: virtual mic, microphone, test",
                    );
                #[cfg(test)]
                widgets::remember(ui.ctx(), "settings", "wizard", r.rect);
                if r.clicked() {
                    actions.push(Action::Wizard);
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(concat!("VoxShift ", env!("CARGO_PKG_VERSION")))
                    .strong()
                    .color(p.text),
            );
            widgets::hint(ui, "Voice changer and soundboard for Windows");
        });
        ui.horizontal(|ui| {
            ui.hyperlink_to(
                format!("{}  Releases (new versions)", icons::LINK),
                RELEASES_URL,
            );
            ui.add_space(space::M);
            ui.hyperlink_to(format!("{}  Source code", icons::LINK), REPO_URL);
        });
    });
}

pub fn page(ui: &mut Ui, cfg: &mut Config, v: &View) -> Vec<Action> {
    let mut actions = Vec::new();
    appearance(ui, cfg, &mut actions);
    ui.add_space(space::M);
    window(ui, cfg, v, &mut actions);
    ui.add_space(space::M);
    feedback(ui, cfg, v);
    ui.add_space(space::M);
    settings_file(ui, v, &mut actions);
    ui.add_space(space::M);
    about(ui, &mut actions);
    actions
}

/// The settings a reset brings back to their defaults; what you made yourself is kept.
pub fn reset(cfg: &Config) -> Config {
    Config {
        sounds: cfg.sounds.clone(),
        soundboard: cfg.soundboard.clone(),
        user_presets: cfg.user_presets.clone(),
        melody: crate::config::MelodySettings {
            files: cfg.melody.files.clone(),
            ..Default::default()
        },
        page: crate::config::Page::Settings,
        ..Config::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reset_keeps_what_you_made() {
        let mut cfg = Config::default();
        cfg.sounds.push(crate::soundboard::library::SoundEntry {
            name: "Airhorn".into(),
            ..Default::default()
        });
        cfg.soundboard.create_folder("Memes");
        cfg.melody.files.push("C:/m/song.mid".into());
        cfg.devices.output = "Speakers".into();
        cfg.monitor_volume = 0.2;
        cfg.ui_scale = 1.5;
        cfg.voice_keys.toggle = Some(crate::soundboard::hotkeys::Hotkey::new(
            0x77, false, false, false, false, "F8",
        ));
        let r = reset(&cfg);
        assert_eq!(r.sounds, cfg.sounds);
        assert_eq!(r.soundboard, cfg.soundboard);
        assert_eq!(r.melody.files, cfg.melody.files);
        assert!(r.devices.output.is_empty() && r.voice_keys.toggle.is_none());
        assert_eq!((r.monitor_volume, r.ui_scale), (0.8, 1.0));
    }
}
