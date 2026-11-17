//! The preset bar: every preset is a chip, the built-in ones first and your own after them
//! (favourites first). The loaded preset is filled in the accent colour, with a dot once a
//! slider moved; only then do Save and Revert appear. Sharing and importing live in the
//! "more" menu, and your own presets are edited from their right-click menu.

use super::icons;
use super::theme::{font, palette, space};
use super::widgets::{self, card};
use crate::config::Config;
use crate::config::presets::{self, Preset};
use crate::dsp::params::Snapshot;
use egui::{
    Align, Color32, CursorIcon, FontId, Key, Layout, Response, RichText, Sense, Stroke, StrokeKind,
    Ui, pos2, vec2,
};

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Apply(Preset),
    /// Save the current settings as your preset with this name (new, or replacing it).
    Save(String),
    Delete(String),
    Rename {
        from: String,
        to: String,
    },
    Favorite(String, bool),
    /// Back to the loaded preset as it was saved.
    Revert,
    /// Write your presets and MIDI files to a .voxpack to send to someone.
    Export,
    /// Add the presets and MIDI files from a .voxpack.
    Import,
}

/// The dialog that asks for a preset name.
struct NameDialog {
    name: String,
    /// The preset being renamed (`None`: the current settings become a new preset).
    renaming: Option<String>,
    focus_pending: bool,
}

/// What the bar keeps between frames.
#[derive(Default)]
pub struct BarState {
    dialog: Option<NameDialog>,
}

impl BarState {
    /// Opens the dialog that saves the current settings under a new name.
    pub fn ask_new_name(&mut self) {
        self.dialog = Some(NameDialog {
            name: String::new(),
            renaming: None,
            focus_pending: true,
        });
    }

    fn ask_rename(&mut self, name: &str) {
        self.dialog = Some(NameDialog {
            name: name.to_string(),
            renaming: Some(name.to_string()),
            focus_pending: true,
        });
    }

    #[cfg(test)]
    pub fn dialog_open(&self) -> bool {
        self.dialog.is_some()
    }

    /// Types `name` into the open dialog (what the text field does).
    #[cfg(test)]
    pub fn type_name(&mut self, name: &str) {
        self.dialog.as_mut().expect("dialog open").name = name.to_string();
    }
}

/// The loaded preset and whether the sliders moved since.
pub fn loaded<'a>(cfg: &'a Config, fx: &Snapshot) -> Option<(&'a Preset, bool)> {
    let name = cfg.preset.as_ref()?;
    let builtin: &'static [Preset] = presets::builtin_presets();
    let preset = builtin
        .iter()
        .chain(&cfg.user_presets)
        .find(|p| &p.name == name)?;
    let modified = !presets::matches(preset, fx, &cfg.melody.source);
    Some((preset, modified))
}

/// One preset as a chip (the soundboard's tabs use the same chip).
pub(super) fn chip(
    ui: &mut Ui,
    name: &str,
    selected: bool,
    modified: bool,
    favorite: bool,
) -> Response {
    let p = palette(ui);
    let text = if favorite {
        format!("\u{2605} {name}")
    } else {
        name.to_string()
    };
    let color = if selected { p.on_accent } else { p.text };
    let galley = ui
        .painter()
        .layout_no_wrap(text, FontId::proportional(font::BODY), color);
    let dot = if modified { 12.0 } else { 0.0 };
    let (rect, r) =
        ui.allocate_exact_size(vec2(galley.size().x + 24.0 + dot, 30.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let hovered = r.hovered();
        let (fill, stroke) = match (selected, hovered) {
            (true, false) => (p.accent, p.accent),
            (true, true) => (p.accent_hover, p.accent_hover),
            (false, true) => (p.surface_raised, p.border_strong),
            (false, false) => (Color32::TRANSPARENT, p.border),
        };
        let painter = ui.painter();
        painter.rect(
            rect,
            rect.height() / 2.0,
            fill,
            Stroke::new(1.0, stroke),
            StrokeKind::Inside,
        );
        let at = pos2(rect.left() + 12.0, rect.center().y - galley.size().y / 2.0);
        painter.galley(at, galley, color);
        if modified {
            painter.circle_filled(pos2(rect.right() - 13.0, rect.center().y), 3.0, color);
        }
        if r.has_focus() {
            painter.rect_stroke(
                rect.expand(2.0),
                rect.height() / 2.0 + 2.0,
                Stroke::new(1.5, p.accent),
                StrokeKind::Outside,
            );
        }
    }
    r.on_hover_cursor(CursorIcon::PointingHand)
}

/// The right-click menu of one of your own presets.
fn own_menu(ui: &mut Ui, preset: &Preset, state: &mut BarState, actions: &mut Vec<Action>) {
    let name = &preset.name;
    if ui
        .button(format!("{}  Rename\u{2026}", icons::EDIT))
        .clicked()
    {
        state.ask_rename(name);
        ui.close();
    }
    let star = if preset.favorite {
        "\u{2606}  Remove from favourites"
    } else {
        "\u{2605}  Add to favourites"
    };
    if ui
        .button(star)
        .on_hover_text("Favourites come first in this bar and are offered in the tray menu")
        .clicked()
    {
        actions.push(Action::Favorite(name.clone(), !preset.favorite));
        ui.close();
    }
    if ui
        .button(format!(
            "{}  Replace with the current settings",
            icons::SAVE
        ))
        .clicked()
    {
        actions.push(Action::Save(name.clone()));
        ui.close();
    }
    ui.separator();
    if ui
        .button(RichText::new(format!("{}  Delete", icons::DELETE)).color(palette(ui).danger))
        .clicked()
    {
        actions.push(Action::Delete(name.clone()));
        ui.close();
    }
}

fn more_menu(ui: &mut Ui, cfg: &Config, state: &mut BarState, actions: &mut Vec<Action>) {
    if ui
        .button(format!("{}  Save as a new preset\u{2026}", icons::ADD))
        .clicked()
    {
        state.ask_new_name();
        ui.close();
    }
    ui.separator();
    // Built-in presets and melodies are in every copy of VoxShift: only your own count.
    let shareable = !cfg.user_presets.is_empty() || !cfg.melody.files.is_empty();
    let share = ui
        .add_enabled(
            shareable,
            egui::Button::new(format!("{}  Share\u{2026}", icons::SHARE)),
        )
        .on_hover_text(
            "Save your presets and your Melody Voice MIDI files in one .voxpack file to send \
             to a friend",
        )
        .on_disabled_hover_text(
            "Nothing to share yet. Built-in presets and melodies are already in everyone's \
             VoxShift: save a preset of your own or import a MIDI file first.",
        );
    if share.clicked() {
        actions.push(Action::Export);
        ui.close();
    }
    if ui
        .button(format!("{}  Import\u{2026}", icons::IMPORT))
        .on_hover_text("Add presets and melodies from a .voxpack file (or drop it on the window)")
        .clicked()
    {
        actions.push(Action::Import);
        ui.close();
    }
}

/// Asks for a name; returns the action once it is confirmed.
fn name_dialog(ctx: &egui::Context, cfg: &Config, state: &mut BarState) -> Option<Action> {
    let dialog = state.dialog.as_mut()?;
    let p = super::theme::palette_of(ctx);
    let renaming = dialog.renaming.clone();
    let name = dialog.name.trim().to_string();
    let problem = presets::name_problem(&name, &cfg.user_presets, renaming.as_deref());
    let replaces = renaming.is_none() && cfg.user_presets.iter().any(|p| p.name == name);
    let mut confirm = false;
    let mut cancel = false;
    let response = super::widgets::modal(ctx, "preset_name").show(ctx, |ui| {
        ui.set_width(360.0);
        match &renaming {
            Some(old) => ui.heading(format!("Rename \"{old}\"")),
            None => ui.heading("Save as a new preset"),
        };
        if renaming.is_none() {
            widgets::hint(
                ui,
                "Saves the current sliders and, with Melody Voice on, its melody.",
            );
        }
        ui.add_space(space::S);
        let edit = ui.add(
            egui::TextEdit::singleline(&mut dialog.name)
                .hint_text("Preset name")
                .desired_width(f32::INFINITY),
        );
        if std::mem::take(&mut dialog.focus_pending) {
            edit.request_focus();
        }
        let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        if enter && problem.is_some() {
            edit.request_focus(); // Enter on a name that can't be used: keep typing
        }
        let note = match (problem, replaces) {
            (Some(why), _) if !name.is_empty() => RichText::new(why).small().color(p.warning),
            (None, true) => RichText::new("You have a preset with this name: it is replaced.")
                .small()
                .color(p.text_muted),
            _ => RichText::new(" ").small(),
        };
        ui.label(note);
        ui.add_space(space::S);
        ui.horizontal(|ui| {
            let label = match (&renaming, replaces) {
                (Some(_), _) => "Rename",
                (None, true) => "Replace",
                (None, false) => "Save",
            };
            let ok = ui
                .add_enabled_ui(problem.is_none(), |ui| widgets::primary_button(ui, label))
                .inner;
            if (ok.clicked() || enter) && problem.is_none() {
                confirm = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
    });
    if confirm {
        state.dialog = None;
        return Some(match renaming {
            Some(from) if from == name => return None,
            Some(from) => Action::Rename { from, to: name },
            None => Action::Save(name),
        });
    }
    if cancel || response.should_close() {
        state.dialog = None; // Esc or a click outside
    }
    None
}

pub fn bar(ui: &mut Ui, cfg: &Config, fx: &Snapshot, state: &mut BarState) -> Vec<Action> {
    let p = palette(ui);
    let mut actions = Vec::new();
    let current = loaded(cfg, fx);
    let modified = current.is_some_and(|(_, m)| m);
    let is_current = |preset: &Preset| current.is_some_and(|(c, _)| c.name == preset.name);
    #[cfg(test)]
    for sometimes in ["save", "revert"] {
        widgets::forget(ui.ctx(), "preset", sometimes);
    }
    card(ui, |ui| {
        ui.horizontal(|ui| {
            widgets::section_title(ui, icons::PRESETS, "Presets");
            if current.is_none() {
                widgets::hint(ui, "Custom settings: no preset is loaded");
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                widgets::menu_icon_button(
                    ui,
                    icons::MORE,
                    "Save as a new preset, share, import",
                    |ui| more_menu(ui, cfg, state, &mut actions),
                );
                // Save and Revert only exist while there is something to save or revert.
                if let Some((preset, true)) = current {
                    let own = !presets::is_builtin(&preset.name);
                    let (label, tip) = if own {
                        ("Save", format!("Save the changes to \"{}\"", preset.name))
                    } else {
                        (
                            "Save as\u{2026}",
                            "Built-in presets can't be changed: save your version under a \
                             name of your own"
                                .to_string(),
                        )
                    };
                    let save = widgets::primary_button(ui, label).on_hover_text(tip);
                    #[cfg(test)]
                    widgets::remember(ui.ctx(), "preset", "save", save.rect);
                    if save.clicked() {
                        if own {
                            actions.push(Action::Save(preset.name.clone()));
                        } else {
                            state.ask_new_name();
                        }
                    }
                    let revert = ui
                        .button(format!("{}  Revert", icons::RESTORE))
                        .on_hover_text(format!("Back to \"{}\" as it was saved", preset.name));
                    #[cfg(test)]
                    widgets::remember(ui.ctx(), "preset", "revert", revert.rect);
                    if revert.clicked() {
                        actions.push(Action::Revert);
                    }
                }
            });
        });
        ui.add_space(space::XS);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(space::S, space::S);
            for preset in presets::builtin_presets() {
                let selected = is_current(preset);
                let r = chip(ui, &preset.name, selected, selected && modified, false);
                #[cfg(test)]
                widgets::remember(ui.ctx(), "preset_chip", &preset.name, r.rect);
                if r.clicked() {
                    actions.push(Action::Apply(preset.clone()));
                }
            }
            // Your own presets, set apart from the built-in ones. The divider and "Yours" go to
            // the next line with the chip after them, rather than being left at the end of one.
            let yours = RichText::new("Yours").small().color(p.text_muted);
            let label_w = ui
                .painter()
                .layout_no_wrap("Yours".into(), FontId::proportional(font::SMALL), p.text)
                .size()
                .x;
            // (At the start of a line the divider has nothing to divide: then it's left out.)
            if ui.available_size_before_wrap().x < 9.0 + label_w + 80.0 + 2.0 * space::S {
                ui.end_row();
            } else {
                let (rect, _) = ui.allocate_exact_size(vec2(9.0, 30.0), Sense::hover());
                ui.painter().vline(
                    rect.center().x,
                    (rect.top() + 5.0)..=(rect.bottom() - 5.0),
                    Stroke::new(1.0, p.border_strong),
                );
            }
            ui.label(yours);
            for preset in presets::favorites_first(&cfg.user_presets) {
                let selected = is_current(preset);
                let r = chip(
                    ui,
                    &preset.name,
                    selected,
                    selected && modified,
                    preset.favorite,
                );
                #[cfg(test)]
                widgets::remember(ui.ctx(), "preset_chip", &preset.name, r.rect);
                let r = r.on_hover_text("Right-click to rename, favourite, replace or delete");
                if r.clicked() {
                    actions.push(Action::Apply(preset.clone()));
                }
                r.context_menu(|ui| own_menu(ui, preset, state, &mut actions));
            }
            let add = ui
                .add(
                    egui::Button::new(
                        RichText::new(format!("{}  New", icons::ADD)).color(p.text_muted),
                    )
                    .corner_radius(15)
                    .min_size(vec2(0.0, 30.0)),
                )
                .on_hover_text("Save the current settings as a new preset of your own");
            #[cfg(test)]
            widgets::remember(ui.ctx(), "preset", "new", add.rect);
            if add.clicked() {
                state.ask_new_name();
            }
        });
    });
    actions.extend(name_dialog(ui.ctx(), cfg, state));
    actions
}
