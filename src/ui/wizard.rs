//! The first-run setup wizard: three steps from a fresh install to a working virtual mic.
//!
//! 1. Virtual mic: is VB-CABLE there (download link and "Check again" if not)?
//! 2. Microphone: pick it and see its meter move.
//! 3. Test: the headphones, a mic check, and what to pick in Discord or a game.
//!
//! It opens by itself on the very first start and again from Settings. Everything it sets is
//! an ordinary setting, also found on the Audio setup page.

use super::audio_setup::{self, Changed, VB_CABLE_URL};
use super::icons;
use super::mic_check::{self, MicCheck};
use super::theme::{font, mix, palette, space};
use super::widgets::{self, Tone};
use crate::config::Config;
use egui::{Align, Color32, Layout, RichText, Ui, vec2};

pub const STEPS: [&str; 3] = ["Virtual mic", "Microphone", "Test"];

/// The wizard while it is open.
#[derive(Debug, Default)]
pub struct Wizard {
    pub step: usize,
}

/// What the wizard asks of the app this frame.
#[derive(Default)]
pub struct Outcome {
    /// Devices, mic or monitor settings changed (as on the Audio setup page).
    pub changed: Changed,
    pub mic_check: bool,
    /// Finished or skipped: close it and don't open it by itself again.
    pub close: bool,
}

/// The step bar on top: numbered dots with their names, done ones ticked.
fn step_bar(ui: &mut Ui, step: usize) {
    let p = palette(ui);
    ui.horizontal(|ui| {
        for (i, name) in STEPS.iter().enumerate() {
            if i > 0 {
                ui.label(RichText::new("\u{2014}").color(p.text_faint));
            }
            let (fill, text): (Color32, Color32) = if i == step {
                (p.accent, p.on_accent)
            } else if i < step {
                (mix(p.surface, p.success, 0.35), p.text)
            } else {
                (p.surface_raised, p.text_muted)
            };
            let mark = if i < step {
                icons::CHECK_CIRCLE.to_string()
            } else {
                (i + 1).to_string()
            };
            let (rect, _) = ui.allocate_exact_size(vec2(22.0, 22.0), egui::Sense::hover());
            ui.painter().circle_filled(rect.center(), 11.0, fill);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                mark,
                egui::FontId::proportional(font::SMALL),
                text,
            );
            let color = if i == step { p.text } else { p.text_muted };
            ui.label(RichText::new(*name).color(color));
        }
    });
}

fn virtual_mic_step(ui: &mut Ui, v: &audio_setup::View, out: &mut Outcome) {
    let p = palette(ui);
    ui.label(
        "VoxShift sends your changed voice into a virtual audio cable. Discord, OBS and games \
         then use that cable as their microphone.",
    );
    ui.add_space(space::S);
    match &v.status.cable {
        Some(cable) => {
            widgets::notice(
                ui,
                icons::CHECK_CIRCLE,
                &format!("VB-CABLE is installed: VoxShift plays into \u{201c}{cable}\u{201d}."),
                Tone::Success,
            );
            audio_setup::state_line(ui, &v.status.output);
        }
        None => {
            widgets::notice(
                ui,
                icons::WARNING,
                "VB-CABLE was not found. Install the free driver, restart Windows if it asks, \
                 then check again.",
                Tone::Warning,
            );
            ui.horizontal(|ui| {
                ui.hyperlink_to(format!("{}  Download VB-CABLE", icons::LINK), VB_CABLE_URL);
                ui.add_space(space::M);
                let again = ui.button(format!("{}  Check again", icons::RESTORE));
                #[cfg(test)]
                widgets::remember(ui.ctx(), "wizard", "rescan", again.rect);
                out.changed.rescan |= again.clicked();
            });
            ui.label(
                RichText::new(
                    "You can go on without it and pick another output later under Audio setup.",
                )
                .small()
                .color(p.text_muted),
            );
        }
    }
}

fn mic_step(ui: &mut Ui, cfg: &mut Config, v: &audio_setup::View, out: &mut Outcome) {
    ui.label("Pick the microphone you talk into, then say something: the meter should move.");
    ui.add_space(space::S);
    audio_setup::mic_row(ui, cfg, v, 0, &mut out.changed);
    ui.add_space(space::XS);
    widgets::hint(
        ui,
        "A second mic to fall back on (when this one is unplugged) can be set under Audio setup.",
    );
}

fn test_step(
    ui: &mut Ui,
    cfg: &mut Config,
    v: &audio_setup::View,
    check: &MicCheck,
    now: f64,
    out: &mut Outcome,
) {
    let p = palette(ui);
    ui.label(RichText::new("Hear yourself").strong().color(p.text));
    let r = ui.checkbox(
        &mut cfg.devices.monitor_enabled,
        "Play the result in my headphones",
    );
    #[cfg(test)]
    widgets::remember(ui.ctx(), "wizard", "monitor", r.rect);
    if r.changed() {
        out.changed.devices = true;
        if cfg.devices.monitor_enabled && !cfg.monitor_voice && !cfg.monitor_sounds {
            cfg.monitor_voice = true;
            out.changed.monitor = true;
        }
    }
    if cfg.devices.monitor_enabled {
        out.changed.devices |= audio_setup::device_combo(
            ui,
            "wizard_monitor",
            &mut cfg.devices.monitor,
            &v.status.outputs,
            "(none)",
        );
        audio_setup::state_line(ui, &v.status.monitor);
    }
    ui.add_space(space::M);
    ui.label(RichText::new("Check your mic").strong().color(p.text));
    ui.horizontal(|ui| {
        out.mic_check |= mic_check::button(ui, check, now);
        widgets::hint(ui, "Talk for 5 seconds, then listen to what others hear.");
    });
    ui.add_space(space::M);
    ui.label(
        RichText::new("In Discord, OBS or your game")
            .strong()
            .color(p.text),
    );
    ui.label(
        "Choose \u{201c}CABLE Output (VB-Audio Virtual Cable)\u{201d} as the microphone \
         (input device). That's it.",
    );
}

/// Draws the wizard (a modal window). `now`: the app's clock, for the mic check.
pub fn show(
    ctx: &egui::Context,
    wizard: &mut Wizard,
    cfg: &mut Config,
    v: &audio_setup::View,
    check: &MicCheck,
    now: f64,
) -> Outcome {
    let mut out = Outcome::default();
    let last = STEPS.len() - 1;
    let modal = egui::Modal::new(egui::Id::new("setup_wizard")).show(ctx, |ui| {
        let p = palette(ui);
        ui.set_width(520.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(icons::LOGO).size(22.0).color(p.accent));
            ui.label(
                RichText::new("Set up VoxShift")
                    .size(font::TITLE)
                    .strong()
                    .color(p.text),
            );
        });
        ui.add_space(space::S);
        step_bar(ui, wizard.step);
        ui.separator();
        ui.add_space(space::XS);
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), 0.0),
            Layout::top_down(Align::Min),
            |ui| {
                ui.set_min_height(210.0);
                match wizard.step {
                    0 => virtual_mic_step(ui, v, &mut out),
                    1 => mic_step(ui, cfg, v, &mut out),
                    _ => test_step(ui, cfg, v, check, now, &mut out),
                }
            },
        );
        ui.separator();
        ui.horizontal(|ui| {
            let skip = ui
                .add(
                    egui::Button::new(RichText::new("Skip setup").color(p.text_muted)).frame(false),
                )
                .on_hover_text("Open it again any time under Settings \u{203a} About");
            #[cfg(test)]
            widgets::remember(ui.ctx(), "wizard", "skip", skip.rect);
            out.close |= skip.clicked();
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let label = if wizard.step == last {
                    "Finish"
                } else {
                    "Next"
                };
                let next = widgets::primary_button(ui, label);
                #[cfg(test)]
                widgets::remember(ui.ctx(), "wizard", "next", next.rect);
                if next.clicked() {
                    if wizard.step == last {
                        out.close = true;
                    } else {
                        wizard.step += 1;
                    }
                }
                if wizard.step > 0 {
                    let back = ui.button("Back");
                    #[cfg(test)]
                    widgets::remember(ui.ctx(), "wizard", "back", back.rect);
                    if back.clicked() {
                        wizard.step -= 1;
                    }
                }
            });
        });
    });
    // Esc or a click outside: the same as "Skip".
    out.close |= modal.should_close();
    out
}
