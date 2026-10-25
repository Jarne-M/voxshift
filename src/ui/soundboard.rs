//! Soundboard: tabs (All, Favorites, Recent, Unsorted and your folders), search, sorting and
//! the clip grid. Drawing only reads the board; changes come back as actions.

use super::theme::{ACCENT, BAD, CARD};
use crate::audio::shared::Shared;
use crate::soundboard::hotkeys::Hotkey;
use crate::soundboard::library::{Board, LoadState, SoundEntry};
use crate::soundboard::mixer::SoundId;
use crate::soundboard::organize::{self, BoardSettings, BoardTab, SortMode};
use crate::soundboard::playmode::PlayMode;
use crate::voice_keys::KeyTarget;
use egui::{Color32, Id, Key, RichText, Ui, vec2};

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
    Tab(BoardTab),
    Sort(SortMode),
    ToggleFavorite(SoundId),
    Color(SoundId, Option<u8>),
    /// Into a folder (`None` = Unsorted).
    MoveTo(SoundId, Option<u32>),
    /// Drag-and-drop in "My order": the first clip goes where the second is.
    Reorder(SoundId, SoundId),
    /// Pick the file of a clip whose file is missing.
    Locate(SoundId),
    NewFolder,
    RenameFolder(u32, String),
    DeleteFolder(u32),
    MoveFolder(u32, isize),
    ClearRecent,
    PlayMode(SoundId, PlayMode),
    /// Mouse button pressed on a hold / loop clip (it plays until the button goes up).
    HoldDown(SoundId),
    /// No mouse button is down (any clip held by the mouse stops).
    MouseUp,
}

const CELL: f32 = 190.0;

/// Clip colours: (name, colour). Saved as the index.
pub const PALETTE: [(&str, Color32); 10] = [
    ("Red", Color32::from_rgb(200, 60, 60)),
    ("Orange", Color32::from_rgb(220, 120, 40)),
    ("Yellow", Color32::from_rgb(230, 200, 60)),
    ("Lime", Color32::from_rgb(150, 200, 60)),
    ("Green", Color32::from_rgb(50, 160, 90)),
    ("Teal", Color32::from_rgb(40, 160, 160)),
    ("Blue", Color32::from_rgb(60, 120, 220)),
    ("Purple", Color32::from_rgb(130, 80, 200)),
    ("Pink", Color32::from_rgb(220, 90, 160)),
    ("Grey", Color32::from_rgb(140, 140, 150)),
];

/// Black or white, whichever reads better on `bg` (in any theme).
pub fn text_on(bg: Color32) -> Color32 {
    let l = 0.2126 * f32::from(bg.r()) + 0.7152 * f32::from(bg.g()) + 0.0722 * f32::from(bg.b());
    if l > 150.0 {
        Color32::BLACK
    } else {
        Color32::WHITE
    }
}

/// What is dragged between clips and tabs.
#[derive(Clone, Copy)]
struct ClipDrag(SoundId);

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
    pub settings: &'a BoardSettings,
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

fn top_row(ui: &mut Ui, v: &View, actions: &mut Vec<Action>) {
    ui.horizontal_wrapped(|ui| {
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
            actions,
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
            actions,
        );
    });
}

/// One tab button; a clip dropped on it is moved there (or starred, on Favorites).
fn tab(
    ui: &mut Ui,
    v: &View,
    t: BoardTab,
    label: String,
    actions: &mut Vec<Action>,
) -> egui::Response {
    let selected = v.settings.tab == t;
    let r = ui.selectable_label(selected, label);
    if r.clicked() && !selected {
        actions.push(Action::Tab(t));
    }
    let target = match t {
        BoardTab::Folder(f) => Some(Some(f)),
        BoardTab::Unsorted => Some(None),
        _ => None,
    };
    let droppable = target.is_some() || t == BoardTab::Favorites;
    if droppable && r.dnd_hover_payload::<ClipDrag>().is_some() {
        ui.painter().rect_stroke(
            r.rect,
            4.0,
            egui::Stroke::new(2.0, ACCENT),
            egui::StrokeKind::Outside,
        );
    }
    if droppable && let Some(drag) = r.dnd_release_payload::<ClipDrag>() {
        match target {
            Some(folder) => actions.push(Action::MoveTo(drag.0, folder)),
            None => {
                let starred = v
                    .board
                    .sounds
                    .iter()
                    .any(|s| s.id == drag.0 && s.entry.favorite);
                if !starred {
                    actions.push(Action::ToggleFavorite(drag.0));
                }
            }
        }
    }
    r
}

fn tab_bar(ui: &mut Ui, v: &View, entries: &[SoundEntry], actions: &mut Vec<Action>) {
    let count = |f: &dyn Fn(&SoundEntry) -> bool| entries.iter().filter(|e| f(e)).count();
    ui.horizontal_wrapped(|ui| {
        tab(
            ui,
            v,
            BoardTab::All,
            format!("All ({})", entries.len()),
            actions,
        );
        tab(
            ui,
            v,
            BoardTab::Favorites,
            format!("\u{2605} Favorites ({})", count(&|e| e.favorite)),
            actions,
        );
        tab(ui, v, BoardTab::Recent, "Recent".into(), actions);
        tab(
            ui,
            v,
            BoardTab::Unsorted,
            format!("Unsorted ({})", count(&|e| e.folder.is_none())),
            actions,
        );
        for (i, f) in v.settings.folders.iter().enumerate() {
            let n = count(&|e| e.folder == Some(f.id));
            let r = tab(
                ui,
                v,
                BoardTab::Folder(f.id),
                format!("{} ({n})", f.name),
                actions,
            );
            r.on_hover_text(
                "Right-click to rename, move or delete. Drop clips here to move them in.",
            )
            .context_menu(|ui| {
                folder_menu(ui, f.id, &f.name, i, v.settings.folders.len(), actions)
            });
        }
        if ui
            .small_button("+ Folder")
            .on_hover_text("New folder")
            .clicked()
        {
            actions.push(Action::NewFolder);
        }
    });
}

fn folder_menu(
    ui: &mut Ui,
    id: u32,
    name: &str,
    index: usize,
    count: usize,
    actions: &mut Vec<Action>,
) {
    let key = Id::new(("rename_folder", id));
    let mut text = ui
        .data_mut(|d| d.get_temp::<String>(key))
        .unwrap_or_else(|| name.to_string());
    ui.label(RichText::new("Name").small().weak());
    let edit = ui.text_edit_singleline(&mut text);
    let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
    ui.data_mut(|d| d.insert_temp(key, text.clone()));
    if (ui.button("Rename").clicked() || enter) && text.trim() != name {
        actions.push(Action::RenameFolder(id, text));
        ui.data_mut(|d| d.remove::<String>(key));
        ui.close();
    }
    ui.separator();
    if index > 0 && ui.button("Move left").clicked() {
        actions.push(Action::MoveFolder(id, -1));
        ui.close();
    }
    if index + 1 < count && ui.button("Move right").clicked() {
        actions.push(Action::MoveFolder(id, 1));
        ui.close();
    }
    ui.separator();
    if ui
        .button(RichText::new("Delete folder").color(BAD))
        .on_hover_text("Its clips go to Unsorted; no clip or file is deleted")
        .clicked()
    {
        actions.push(Action::DeleteFolder(id));
        ui.close();
    }
}

/// The search box (Ctrl+F focuses it, Esc clears it) and the sort order.
fn search_row(ui: &mut Ui, v: &View, search: &mut String, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        let id = Id::new("soundboard_search");
        let r = ui.add(
            egui::TextEdit::singleline(search)
                .id(id)
                .hint_text("Search all clips (Ctrl+F)")
                .desired_width(240.0),
        );
        if ui.input(|i| i.modifiers.command && i.key_pressed(Key::F)) {
            r.request_focus();
        }
        if (r.has_focus() || r.lost_focus()) && ui.input(|i| i.key_pressed(Key::Escape)) {
            search.clear();
        }
        if !search.is_empty() && ui.small_button("x").on_hover_text("Clear search").clicked() {
            search.clear();
        }
        ui.separator();
        ui.label("Sort");
        let mut sort = v.settings.sort;
        egui::ComboBox::from_id_salt("soundboard_sort")
            .selected_text(sort.label())
            .show_ui(ui, |ui| {
                for m in SortMode::ALL {
                    ui.selectable_value(&mut sort, m, m.label());
                }
            });
        if sort != v.settings.sort {
            actions.push(Action::Sort(sort));
        }
        if v.settings.tab == BoardTab::Recent
            && !v.settings.recent.is_empty()
            && ui
                .small_button("Clear")
                .on_hover_text("Empty the Recent list")
                .clicked()
        {
            actions.push(Action::ClearRecent);
        }
    });
}

pub fn panel(ui: &mut Ui, v: &View, search: &mut String) -> Vec<Action> {
    let mut actions = Vec::new();
    // A clip held with the mouse stops as soon as no button is down, wherever it was let go
    // (or if the window lost the release).
    if !ui.input(|i| i.pointer.any_down()) {
        actions.push(Action::MouseUp);
    }
    top_row(ui, v, &mut actions);
    ui.label(
        RichText::new(if v.hotkeys_available {
            "Drop wav/mp3/ogg/flac files here. Hotkeys work while other apps (games) are focused, and the key still reaches them."
        } else {
            "Drop wav/mp3/ogg/flac files here. (Global hotkeys are not available on this system.)"
        })
        .small(),
    );
    ui.add_space(4.0);

    if v.board.sounds.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("No sounds yet").heading());
            ui.label("Click \"Add sounds...\" or drag audio files onto the window.");
        });
        return actions;
    }

    let entries = v.board.entries();
    tab_bar(ui, v, &entries, &mut actions);
    search_row(ui, v, search, &mut actions);
    ui.add_space(6.0);

    let shown = organize::visible(&entries, v.settings, search);
    if shown.is_empty() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(match (search.trim().is_empty(), v.settings.tab) {
                (false, _) => "No clip matches the search.",
                (true, BoardTab::Favorites) => "No favourites yet: click a clip's star.",
                (true, BoardTab::Recent) => "Nothing played yet.",
                (true, _) => {
                    "Nothing here yet: drag clips onto this tab, or use Move to... (right-click)."
                }
            });
        });
        return actions;
    }
    // Dragging onto another clip reorders, but only where the order on screen is "My order".
    let reorderable = v.settings.sort == SortMode::Manual
        && search.trim().is_empty()
        && v.settings.tab != BoardTab::Recent;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
        for i in shown {
            let s = &v.board.sounds[i];
            // Each card stacks its controls vertically inside the wrapping row.
            let layout = egui::Layout::top_down(egui::Align::Min);
            let card = ui.allocate_ui_with_layout(vec2(CELL, 150.0), layout, |ui| {
                ui.dnd_drag_source(Id::new(("clip", s.id)), ClipDrag(s.id), |ui| {
                    egui::Frame::new()
                        .fill(CARD)
                        .corner_radius(8)
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| {
                            ui.set_width(CELL - 16.0);
                            cell(ui, v, s.id, &s.entry, &s.state, &mut actions);
                        });
                })
                .response
            });
            let r = card.inner;
            if reorderable {
                if let Some(drag) = r.dnd_hover_payload::<ClipDrag>()
                    && drag.0 != s.id
                {
                    ui.painter().rect_stroke(
                        r.rect,
                        8.0,
                        egui::Stroke::new(2.0, ACCENT),
                        egui::StrokeKind::Outside,
                    );
                }
                if let Some(drag) = r.dnd_release_payload::<ClipDrag>()
                    && drag.0 != s.id
                {
                    actions.push(Action::Reorder(drag.0, s.id));
                }
            }
        }
    });
    actions
}

fn clip_menu(
    ui: &mut Ui,
    v: &View,
    id: SoundId,
    e: &SoundEntry,
    missing: bool,
    actions: &mut Vec<Action>,
) {
    if ui.button("Stop").clicked() {
        actions.push(Action::Stop(id));
        ui.close();
    }
    let star = if e.favorite {
        "Remove from Favorites"
    } else {
        "Add to Favorites"
    };
    if ui.button(star).clicked() {
        actions.push(Action::ToggleFavorite(id));
        ui.close();
    }
    ui.menu_button("Move to...", |ui| {
        if ui
            .add_enabled(e.folder.is_some(), egui::Button::new("Unsorted"))
            .clicked()
        {
            actions.push(Action::MoveTo(id, None));
            ui.close();
        }
        for f in &v.settings.folders {
            if ui
                .add_enabled(e.folder != Some(f.id), egui::Button::new(&f.name))
                .clicked()
            {
                actions.push(Action::MoveTo(id, Some(f.id)));
                ui.close();
            }
        }
        if v.settings.folders.is_empty() {
            ui.label(
                RichText::new("No folders yet: \"+ Folder\" makes one")
                    .small()
                    .weak(),
            );
        }
    });
    ui.menu_button("Play mode", |ui| {
        for m in PlayMode::ALL {
            if ui.selectable_label(e.play_mode == m, m.label()).clicked() {
                actions.push(Action::PlayMode(id, m));
                ui.close();
            }
        }
        ui.label(
            RichText::new("Hold and loop work with the hotkey and with the mouse button.")
                .small()
                .weak(),
        );
    });
    ui.menu_button("Colour", |ui| {
        if ui.selectable_label(e.color.is_none(), "None").clicked() {
            actions.push(Action::Color(id, None));
            ui.close();
        }
        for (i, (name, c)) in PALETTE.iter().enumerate() {
            let text = RichText::new(format!("\u{2605} {name}")).color(*c);
            if ui
                .selectable_label(e.color == Some(i as u8), text)
                .clicked()
            {
                actions.push(Action::Color(id, Some(i as u8)));
                ui.close();
            }
        }
    });
    if missing && ui.button("Locate file...").clicked() {
        actions.push(Action::Locate(id));
        ui.close();
    }
    ui.separator();
    if ui.button(RichText::new("Delete").color(BAD)).clicked() {
        actions.push(Action::Remove(id));
        ui.close();
    }
}

fn cell(
    ui: &mut Ui,
    v: &View,
    id: SoundId,
    e: &SoundEntry,
    state: &LoadState,
    actions: &mut Vec<Action>,
) {
    let playing = v.shared.is_playing(id);
    let name = e.name.as_str();
    let missing = matches!(state, LoadState::Failed(_));
    let (text, enabled, tip) = match state {
        LoadState::Ready(d) => (
            name.to_string(),
            true,
            format!(
                "{:.1} s, played {} times. Click to play, again to restart. Right-click for \
                 more (stop, favourite, folder, colour, delete); drag onto a tab to move it.",
                d.len_frames() as f32 / d.sample_rate as f32,
                e.plays
            ),
        ),
        LoadState::Loading => (
            format!("{name}\n(loading...)"),
            false,
            "Decoding...".to_string(),
        ),
        LoadState::Failed(err) => (
            format!("{name}\n(file missing)"),
            false,
            format!("{err}\n{}\nRight-click, Locate file...", e.path.display()),
        ),
    };
    let tint = e
        .color
        .and_then(|c| PALETTE.get(usize::from(c)))
        .map(|p| p.1);
    let fill = match (playing, tint) {
        (true, _) => ACCENT,
        (false, Some(c)) => c,
        (false, None) => Color32::from_gray(52),
    };
    let button = egui::Button::new(RichText::new(text).strong().color(text_on(fill)))
        .fill(fill)
        .min_size(vec2(ui.available_width(), 52.0))
        .wrap();
    let r = ui.add_enabled(enabled, button);
    let r = if enabled {
        r.on_hover_text(tip)
    } else {
        r.on_disabled_hover_text(tip)
    };
    if e.play_mode.is_held() {
        // Plays while the mouse button is held on it (like the hotkey).
        if enabled && r.hovered() && ui.input(|i| i.pointer.primary_pressed()) {
            actions.push(Action::HoldDown(id));
        }
    } else if r.clicked() {
        actions.push(Action::Play(id));
    }
    // Right-click opens the menu, also on a greyed-out (missing) clip, whose disabled button
    // does not react to the mouse itself.
    if enabled {
        r.context_menu(|ui| clip_menu(ui, v, id, e, missing, actions));
    } else {
        ui.interact(r.rect, Id::new(("clip_menu", id)), egui::Sense::click())
            .context_menu(|ui| clip_menu(ui, v, id, e, missing, actions));
    }

    let mut vol = e.volume;
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
        let (star, star_color) = if e.favorite {
            ("\u{2605}", Color32::from_rgb(240, 200, 60))
        } else {
            ("\u{2606}", ui.visuals().weak_text_color())
        };
        if ui
            .add(
                egui::Button::new(RichText::new(star).color(star_color))
                    .small()
                    .frame(false),
            )
            .on_hover_text(if e.favorite {
                "Favourite (click to remove)"
            } else {
                "Add to Favorites"
            })
            .clicked()
        {
            actions.push(Action::ToggleFavorite(id));
        }
        let badge = e.play_mode.badge();
        if !badge.is_empty() {
            ui.label(RichText::new(badge).small().strong().color(ACCENT))
                .on_hover_text(format!(
                    "Play mode: {}. Right-click to change.",
                    e.play_mode.label()
                ));
        }
        if v.capturing_sound() == Some(id) {
            if ui
                .button(RichText::new("Press a key... (Esc)").color(ACCENT))
                .on_hover_text("Press the key combination to use. Click to cancel.")
                .clicked()
            {
                actions.push(Action::CancelHotkey);
            }
        } else {
            let (label, tip) = match &e.hotkey {
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
            if e.hotkey.is_some()
                && ui
                    .small_button("x")
                    .on_hover_text("Remove hotkey")
                    .clicked()
            {
                actions.push(Action::ClearHotkey(id));
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if missing {
                if ui
                    .small_button("Locate...")
                    .on_hover_text("The file was moved or deleted: point to it again")
                    .clicked()
                {
                    actions.push(Action::Locate(id));
                }
            } else if ui
                .small_button("Delete")
                .on_hover_text("Remove from the soundboard")
                .clicked()
            {
                actions.push(Action::Remove(id));
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_text_stays_readable_on_every_colour() {
        for (name, c) in PALETTE {
            let t = text_on(c);
            let l = |c: Color32| {
                0.2126 * f32::from(c.r()) + 0.7152 * f32::from(c.g()) + 0.0722 * f32::from(c.b())
            };
            let contrast = (l(t) - l(c)).abs();
            assert!(contrast > 90.0, "{name}: contrast {contrast}");
        }
        assert_eq!(text_on(Color32::from_gray(52)), Color32::WHITE);
        assert_eq!(
            text_on(PALETTE[2].1),
            Color32::BLACK,
            "yellow gets dark text"
        );
    }
}
