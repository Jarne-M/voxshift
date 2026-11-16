//! "Window & startup" settings: what the close button does, start with Windows, start hidden.

use super::icons;
use super::theme::{self, ThemeChoice, WARN};
use super::widgets::{card, section_title};
use crate::config::{CloseBehavior, Config};
use egui::{RichText, Ui};

pub struct View {
    /// Current state of the Run registry entry.
    pub autostart: bool,
    /// The Run entry starts a different copy of VoxShift.
    pub autostart_elsewhere: bool,
    pub tray_available: bool,
}

/// Returns the wanted "Start with Windows" state when the user toggled it.
pub fn panel(ui: &mut Ui, cfg: &mut Config, v: &View) -> Option<bool> {
    let mut autostart_toggle = None;
    card(ui, |ui| {
        section_title(ui, icons::WINDOW, "Window & startup");
        ui.horizontal(|ui| {
            ui.label("Theme");
            let before = cfg.theme;
            egui::ComboBox::from_id_salt("theme")
                .selected_text(cfg.theme.label())
                .show_ui(ui, |ui| {
                    for t in ThemeChoice::ALL {
                        ui.selectable_value(&mut cfg.theme, t, t.label());
                    }
                });
            if cfg.theme != before {
                theme::set_choice(ui.ctx(), cfg.theme);
            }
        });
        ui.label("When I close the window:");
        let label = |b: CloseBehavior| match b {
            CloseBehavior::Ask => "Ask every time",
            CloseBehavior::MinimizeToTray => "Always minimize to tray",
            CloseBehavior::Quit => "Always quit",
        };
        egui::ComboBox::from_id_salt("close_behavior")
            .selected_text(label(cfg.close_behavior))
            .show_ui(ui, |ui| {
                for b in [
                    CloseBehavior::Ask,
                    CloseBehavior::MinimizeToTray,
                    CloseBehavior::Quit,
                ] {
                    ui.selectable_value(&mut cfg.close_behavior, b, label(b));
                }
            });
        if cfg.close_behavior == CloseBehavior::Quit {
            ui.label(
                RichText::new("Closing the window then stops your virtual mic.")
                    .small()
                    .color(WARN),
            );
        }
        ui.add_space(4.0);
        let mut on = v.autostart;
        if ui
            .checkbox(&mut on, "Start with Windows")
            .on_hover_text("Adds VoxShift to your user's startup programs (no admin needed)")
            .changed()
        {
            autostart_toggle = Some(on);
        }
        if v.autostart && v.autostart_elsewhere {
            ui.label(
                RichText::new("Windows starts a different VoxShift.exe; untick and tick again to use this one.")
                    .small()
                    .color(WARN),
            );
        }
        ui.add_enabled(
            v.tray_available,
            egui::Checkbox::new(&mut cfg.start_minimized, "Start minimized to tray"),
        );
    });
    autostart_toggle
}
