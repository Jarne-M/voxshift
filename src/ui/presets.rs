//! Preset picker: built-ins plus your own, save as / delete, and a "modified" marker.

use crate::config::Config;
use crate::config::presets::{self, Preset};
use crate::dsp::params::Snapshot;
use egui::{RichText, Ui};

pub enum Action {
    Apply(Preset),
    Save(String),
    Delete(String),
}

pub fn bar(ui: &mut Ui, cfg: &Config, fx: &Snapshot, new_name: &mut String) -> Option<Action> {
    let builtin = presets::builtin();
    let all = || builtin.iter().chain(cfg.user_presets.iter());
    let current = cfg
        .preset
        .as_ref()
        .and_then(|n| all().find(|p| &p.name == n));
    let shown = match current {
        Some(p) if presets::matches(p, fx, &cfg.melody.source) => p.name.clone(),
        Some(p) => format!("{} (modified)", p.name),
        None => "(custom)".to_string(),
    };
    let mut action = None;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Preset").strong());
        egui::ComboBox::from_id_salt("preset")
            .width(200.0)
            .selected_text(shown)
            .show_ui(ui, |ui| {
                ui.label(RichText::new("Built-in").small().weak());
                for p in &builtin {
                    if ui
                        .selectable_label(cfg.preset.as_ref() == Some(&p.name), &p.name)
                        .clicked()
                    {
                        action = Some(Action::Apply(p.clone()));
                    }
                }
                if !cfg.user_presets.is_empty() {
                    ui.separator();
                    ui.label(RichText::new("Yours").small().weak());
                    for p in &cfg.user_presets {
                        if ui
                            .selectable_label(cfg.preset.as_ref() == Some(&p.name), &p.name)
                            .clicked()
                        {
                            action = Some(Action::Apply(p.clone()));
                        }
                    }
                }
            });
        ui.separator();
        ui.add(
            egui::TextEdit::singleline(new_name)
                .hint_text("new preset name")
                .desired_width(150.0),
        );
        let name = new_name.trim();
        let valid = !name.is_empty() && !presets::is_builtin(name);
        let overwrite = cfg.user_presets.iter().any(|p| p.name == name);
        let label = if overwrite { "Update" } else { "Save" };
        let hint = if presets::is_builtin(name) {
            "Built-in presets cannot be overwritten"
        } else {
            "Save the current sliders (and, with Melody Voice on, its melody)"
        };
        if ui
            .add_enabled(valid, egui::Button::new(label))
            .on_hover_text(hint)
            .clicked()
        {
            action = Some(Action::Save(name.to_string()));
        }
        if let Some(p) = current.filter(|p| !presets::is_builtin(&p.name))
            && ui
                .button("Delete")
                .on_hover_text(format!("Delete \"{}\"", p.name))
                .clicked()
        {
            action = Some(Action::Delete(p.name.clone()));
        }
    });
    action
}
