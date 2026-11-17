//! The Soundboard page: a toolbar (add, search, sort, view), the tabs (All, Favorites, Recent,
//! Unsorted and your folders) and the clips, as pads in a grid or as a compact list. Next to
//! the page there is a strip with what is playing now and, while a clip is being edited, a
//! panel with everything about that clip.
//!
//! A pad only shows what matters at a glance: its colour, its name and, when there is
//! something to say, a star, its play mode and its hotkey. Everything else is in the
//! right-click menu and in the edit panel (the pencil that appears on a pad under the
//! pointer). Drawing only reads the board; changes come back as actions.

use super::theme::{Palette, RADIUS_M, RADIUS_S, font, mix, palette, space};
use super::widgets::{self, KeySlot, Tone};
use super::{icons, presets};
use crate::audio::shared::Shared;
use crate::soundboard::hotkeys::Hotkey;
use crate::soundboard::library::{Board, LoadState, Sound, SoundEntry};
use crate::soundboard::mixer::SoundId;
use crate::soundboard::names;
use crate::soundboard::organize::{
    self, BoardSettings, BoardTab, BoardView, PAD_SIZE_DEFAULT, PAD_SIZE_RANGE, SortMode,
};
use crate::soundboard::playmode::PlayMode;
use crate::soundboard::shape::{MAX_FADE_S, Trim};
use crate::voice_keys::KeyTarget;
use egui::{
    Align, Align2, Color32, CornerRadius, CursorIcon, FontId, Frame, Id, Key, Layout, Margin, Rect,
    Response, RichText, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};
use std::sync::atomic::Ordering::Relaxed;

/// The search box's widget id (Ctrl+F on another page focuses it).
pub fn search_id() -> Id {
    Id::new("soundboard_search")
}

pub enum Action {
    AddFiles,
    Play(SoundId),
    Stop(SoundId),
    StopAll,
    /// Takes the clip off the board (the app offers an Undo).
    Remove(SoundId),
    Volume(SoundId, f32),
    /// The master volume of every clip (0..1).
    MasterVolume(f32),
    /// "Even out loudness" on or off.
    Normalize(bool),
    /// A clip's trim and fades.
    Trim(SoundId, crate::soundboard::shape::Trim),
    /// Play (or, when it is already, stop) a clip in the headphones only.
    Preview(SoundId),
    StopPreview,
    /// A new display name; the file keeps its own.
    Rename(SoundId, String),
    BeginHotkey(SoundId),
    CancelHotkey,
    ClearHotkey(SoundId),
    MonitorSounds(bool),
    /// Assign / clear the Stop all or "Also play in my headphones" hotkey (the same ones as
    /// on the Hotkeys page).
    BeginActionKey(KeyTarget),
    ClearActionKey(KeyTarget),
    Tab(BoardTab),
    Sort(SortMode),
    /// Pads in a grid, or a compact list.
    View(BoardView),
    /// The width of a pad in the grid.
    PadSize(f32),
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
    /// Removes the folder; its clips go to Unsorted (the app offers an Undo).
    DeleteFolder(u32),
    MoveFolder(u32, isize),
    ClearRecent,
    PlayMode(SoundId, PlayMode),
    /// Mouse button pressed on a hold / loop clip (it plays until the button goes up).
    HoldDown(SoundId),
    /// No mouse button is down (any clip held by the mouse stops).
    MouseUp,
}

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

/// Space between pads.
const GAP: f32 = 10.0;
/// Height of a row in the list view, and the space between rows.
const ROW_H: f32 = 38.0;
const ROW_GAP: f32 = 4.0;
/// The pencil and the grip on a pad.
const TOOL: f32 = 24.0;
/// Width of the edit panel and height of the "Now playing" strip.
pub const EDIT_W: f32 = 300.0;
pub const NOW_H: f32 = 52.0;
/// Volume a new clip gets (see `SoundEntry::default`).
const VOLUME_DEFAULT: f32 = 0.8;

/// What is dragged between clips and tabs.
#[derive(Clone, Copy)]
struct ClipDrag(SoundId);

/// The clip open in the edit panel.
struct Editing {
    id: SoundId,
    /// The name as it is being typed (the clip follows while it is a usable name).
    name: String,
    /// The name field still has to take the focus (opened by "Rename").
    focus_name: bool,
}

/// What the page keeps between frames (not saved).
#[derive(Default)]
pub struct State {
    editing: Option<Editing>,
}

impl State {
    /// Opens the edit panel on a clip. `rename`: put the cursor in the name field.
    pub fn open(&mut self, id: SoundId, name: &str, rename: bool) {
        self.editing = Some(Editing {
            id,
            name: name.to_string(),
            focus_name: rename,
        });
    }

    pub fn close(&mut self) {
        self.editing = None;
    }

    /// The clip in the edit panel, if it is open.
    pub fn editing(&self) -> Option<SoundId> {
        self.editing.as_ref().map(|e| e.id)
    }
}

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

    fn sound(&self, id: SoundId) -> Option<&Sound> {
        self.board.sounds.iter().find(|s| s.id == id)
    }
}

// ------------------------------------------------------------------------------------------
// Small helpers
// ------------------------------------------------------------------------------------------

/// A clip's length in seconds, once it is loaded.
fn length_s(state: &LoadState) -> Option<f32> {
    match state {
        LoadState::Ready(d) => Some(d.len_frames() as f32 / d.sample_rate.max(1) as f32),
        _ => None,
    }
}

/// Seconds as "0:07" (rounded up, so a clip that is still playing never shows 0:00).
pub fn clock(seconds: f32) -> String {
    let s = seconds.max(0.0).ceil() as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

/// The time shown on a clip: what is left while it plays, its length otherwise.
fn time_text(length: Option<f32>, progress: Option<f32>) -> Option<String> {
    let length = length?;
    Some(match progress {
        Some(p) => clock(length * (1.0 - p.clamp(0.0, 1.0))),
        None => clock(length),
    })
}

fn tint(e: &SoundEntry) -> Option<Color32> {
    e.color
        .and_then(|c| PALETTE.get(usize::from(c)))
        .map(|p| p.1)
}

/// How many pads fit next to each other in `available`, and how wide each then is (they
/// stretch a little so a row always ends at the edge).
pub fn grid_columns(available: f32, pad: f32) -> (usize, f32) {
    let cols = (((available + GAP) / (pad + GAP)).floor() as usize).max(1);
    let width = (available - GAP * (cols as f32 - 1.0)) / cols as f32;
    (cols, width.max(1.0))
}

fn pad_height(size: f32) -> f32 {
    (size * 0.5).clamp(76.0, 112.0)
}

/// An icon button that stays lit while what it switches is on.
fn toggle_icon(ui: &mut Ui, icon: &str, on: bool, tip: &str) -> Response {
    let p = palette(ui);
    let color = if on { p.text } else { p.text_muted };
    ui.add(
        egui::Button::new(RichText::new(icon).size(16.0).color(color))
            .selected(on)
            .frame_when_inactive(on)
            .min_size(vec2(28.0, 28.0))
            .corner_radius(CornerRadius::same(RADIUS_S)),
    )
    .on_hover_text(tip)
}

/// A small pill of text painted at `left`, vertically centred on `cy`; returns its width.
fn mini_pill(ui: &Ui, left: f32, cy: f32, text: &str, fg: Color32, max_width: f32) -> f32 {
    let p = palette(ui);
    let pad = vec2(6.0, 2.0);
    let galley = widgets::elided(
        ui,
        text,
        FontId::proportional(font::SMALL - 1.0),
        fg,
        (max_width - 2.0 * pad.x).max(1.0),
        1,
    );
    let size = galley.size() + 2.0 * pad;
    let rect = Rect::from_min_size(pos2(left, cy - size.y / 2.0), size);
    ui.painter()
        .rect_filled(rect, size.y / 2.0, mix(p.surface, fg, 0.16));
    ui.painter().galley(rect.min + pad, galley, fg);
    size.x
}

/// How wide [`mini_pill`] would be for this text.
fn mini_pill_width(ui: &Ui, text: &str) -> f32 {
    let galley = ui.painter().layout_no_wrap(
        text.to_string(),
        FontId::proportional(font::SMALL - 1.0),
        Color32::WHITE,
    );
    galley.size().x + 12.0
}

// ------------------------------------------------------------------------------------------
// Toolbar
// ------------------------------------------------------------------------------------------

/// Add sounds, the search box (Ctrl+F focuses it, Esc clears it) and the sort order.
fn toolbar_left(
    ui: &mut Ui,
    v: &View,
    search: &mut String,
    narrow: bool,
    actions: &mut Vec<Action>,
) {
    let p = palette(ui);
    let add = widgets::primary_button(ui, &format!("{}  Add sounds", icons::ADD))
        .on_hover_text("Pick wav, mp3, ogg or flac files (or drop them on the window)");
    if add.clicked() {
        actions.push(Action::AddFiles);
    }
    let id = search_id();
    let r = ui.add(
        egui::TextEdit::singleline(search)
            .id(id)
            .hint_text(format!("{}  Search all clips (Ctrl+F)", icons::SEARCH))
            .desired_width(if narrow { 150.0 } else { 220.0 }),
    );
    if ui.input(|i| i.modifiers.command && i.key_pressed(Key::F)) {
        r.request_focus();
    }
    if (r.has_focus() || r.lost_focus()) && ui.input(|i| i.key_pressed(Key::Escape)) {
        search.clear();
    }
    if !search.is_empty() && widgets::icon_button(ui, icons::CLOSE, "Clear the search").clicked() {
        search.clear();
    }
    let mut sort = v.settings.sort;
    egui::ComboBox::from_id_salt("soundboard_sort")
        .selected_text(RichText::new(sort.label()).color(p.text))
        .width(132.0)
        .show_ui(ui, |ui| {
            for m in SortMode::ALL {
                ui.selectable_value(&mut sort, m, m.label());
            }
        })
        .response
        .on_hover_text(
            "The order of the clips. Dragging a clip onto another makes it \"My order\".",
        );
    if sort != v.settings.sort {
        actions.push(Action::Sort(sort));
    }
}

/// One hotkey in the "more" menu: set, change or remove it.
fn action_key_menu(
    ui: &mut Ui,
    v: &View,
    target: KeyTarget,
    key: Option<&Hotkey>,
    what: &str,
    actions: &mut Vec<Action>,
) {
    let label = match key {
        Some(k) => format!(
            "{}  Hotkey for {what}: {}\u{2026}",
            icons::KEYBOARD,
            k.pretty()
        ),
        None => format!("{}  Set a hotkey for {what}\u{2026}", icons::KEYBOARD),
    };
    if ui
        .add_enabled(v.hotkeys_available, egui::Button::new(label))
        .on_hover_text("Click, then press the key or combination to use")
        .on_disabled_hover_text("Global hotkeys are not available on this system")
        .clicked()
    {
        actions.push(Action::BeginActionKey(target));
        ui.close();
    }
    if key.is_some()
        && ui
            .button(format!("{}  Remove the hotkey for {what}", icons::CLOSE))
            .clicked()
    {
        actions.push(Action::ClearActionKey(target));
        ui.close();
    }
}

/// The view controls, added from the right edge inwards.
fn toolbar_right(ui: &mut Ui, v: &View, actions: &mut Vec<Action>) {
    let p = palette(ui);
    widgets::menu_icon_button(ui, icons::MORE, "Hotkeys, new folder, Recent", |ui| {
        action_key_menu(
            ui,
            v,
            KeyTarget::StopSounds,
            v.stop_key,
            "Stop all",
            actions,
        );
        ui.separator();
        action_key_menu(
            ui,
            v,
            KeyTarget::MonitorSoundsToggle,
            v.monitor_sounds_key,
            "\"Hear clips myself\"",
            actions,
        );
        ui.separator();
        let mut even = v.settings.normalize;
        let r = ui.checkbox(&mut even, "Even out loudness").on_hover_text(
            "Play every clip at about the same loudness, so a quiet clip and a loud one \
                 need no fiddling with their volumes. Your files are not changed.",
        );
        #[cfg(test)]
        tests::remember(ui, ("menu", "normalize".into()), r.rect);
        if r.changed() {
            actions.push(Action::Normalize(even));
        }
        ui.separator();
        if ui
            .button(format!("{}  New folder", icons::FOLDER_ADD))
            .clicked()
        {
            actions.push(Action::NewFolder);
            ui.close();
        }
        if ui
            .add_enabled(
                !v.settings.recent.is_empty(),
                egui::Button::new(format!("{}  Empty \"Recent\"", icons::DELETE)),
            )
            .clicked()
        {
            actions.push(Action::ClearRecent);
            ui.close();
        }
    });
    let list = v.settings.view == BoardView::List;
    if toggle_icon(ui, icons::VIEW_LIST, list, "Compact list").clicked() && !list {
        actions.push(Action::View(BoardView::List));
    }
    if toggle_icon(ui, icons::VIEW_GRID, !list, "Pads in a grid").clicked() && list {
        actions.push(Action::View(BoardView::Grid));
    }
    if !list {
        let mut size = v.settings.pad_size;
        let r = widgets::Slider::new(&mut size, PAD_SIZE_RANGE)
            .label("Pad size")
            .default_value(PAD_SIZE_DEFAULT)
            .step(2.0)
            .tip("How big the pads are")
            .compact(96.0)
            .show(ui);
        #[cfg(test)]
        tests::remember(ui, ("toolbar", "pad_size".into()), r.rect);
        if r.changed() {
            actions.push(Action::PadSize(size));
        }
        ui.label(
            RichText::new(icons::PAD_SIZE)
                .size(15.0)
                .color(p.text_faint),
        );
    }
    let tip = match (v.monitor_sounds, v.monitor_on) {
        (true, true) => "You hear the clips in your headphones too. Click to stop that.",
        (true, false) => {
            "Clips would play in your headphones too, but the monitor is off: turn it on \
             under Audio setup."
        }
        (false, _) => "Also play clips in my headphones (others always hear them)",
    };
    let hear = toggle_icon(ui, icons::HEADPHONES, v.monitor_sounds, tip);
    #[cfg(test)]
    tests::remember(ui, ("toolbar", "hear".into()), hear.rect);
    if hear.clicked() {
        actions.push(Action::MonitorSounds(!v.monitor_sounds));
    }
}

fn toolbar(ui: &mut Ui, v: &View, search: &mut String, actions: &mut Vec<Action>) {
    // (Everything on one row needs about 800 points.)
    let narrow = ui.available_width() < 820.0;
    if narrow {
        ui.horizontal(|ui| toolbar_left(ui, v, search, true, actions));
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                toolbar_right(ui, v, actions)
            });
        });
    } else {
        ui.horizontal(|ui| {
            toolbar_left(ui, v, search, false, actions);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                toolbar_right(ui, v, actions)
            });
        });
    }
}

/// While a soundboard hotkey waits for its key: says so, right under the toolbar.
fn capture_banner(ui: &mut Ui, v: &View, actions: &mut Vec<Action>) {
    let what = match v.capturing {
        Some(KeyTarget::StopSounds) => "Stop all".to_string(),
        Some(KeyTarget::MonitorSoundsToggle) => "\"Hear clips myself\"".to_string(),
        Some(KeyTarget::Sound(id)) => match v.sound(id) {
            Some(s) => format!("the clip \"{}\"", s.entry.name),
            None => return,
        },
        _ => return,
    };
    let p = palette(ui);
    Frame::new()
        .fill(mix(p.surface, p.accent, 0.10))
        .stroke(Stroke::new(1.0, mix(p.border, p.accent, 0.45)))
        .corner_radius(CornerRadius::same(RADIUS_S))
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let r = widgets::hotkey_button(ui, KeySlot::Capturing)
                    .on_hover_text("Press the key or combination to use. Click to cancel.");
                if r.clicked() {
                    actions.push(Action::CancelHotkey);
                }
                ui.label(RichText::new(format!("is the new hotkey for {what}")).color(p.text));
            });
        });
}

// ------------------------------------------------------------------------------------------
// Tabs
// ------------------------------------------------------------------------------------------

/// One tab; a clip dropped on it is moved there (or starred, on Favorites).
fn tab(ui: &mut Ui, v: &View, t: BoardTab, label: String, actions: &mut Vec<Action>) -> Response {
    let selected = v.settings.tab == t;
    let r = presets::chip(ui, &label, selected, false, false);
    #[cfg(test)]
    tests::remember(ui, ("tab", format!("{t:?}")), r.rect);
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
            r.rect.height() / 2.0,
            Stroke::new(2.0, palette(ui).accent),
            StrokeKind::Outside,
        );
    }
    if droppable && let Some(drag) = r.dnd_release_payload::<ClipDrag>() {
        match target {
            Some(folder) => actions.push(Action::MoveTo(drag.0, folder)),
            None => {
                let starred = v.sound(drag.0).is_some_and(|s| s.entry.favorite);
                if !starred {
                    actions.push(Action::ToggleFavorite(drag.0));
                }
            }
        }
    }
    r
}

fn tab_bar(ui: &mut Ui, v: &View, entries: &[SoundEntry], actions: &mut Vec<Action>) {
    let p = palette(ui);
    let count = |f: &dyn Fn(&SoundEntry) -> bool| entries.iter().filter(|e| f(e)).count();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(space::S, space::S);
        tab(
            ui,
            v,
            BoardTab::All,
            format!("All  {}", entries.len()),
            actions,
        );
        tab(
            ui,
            v,
            BoardTab::Favorites,
            format!("\u{2605} Favorites  {}", count(&|e| e.favorite)),
            actions,
        );
        tab(ui, v, BoardTab::Recent, "Recent".into(), actions);
        tab(
            ui,
            v,
            BoardTab::Unsorted,
            format!("Unsorted  {}", count(&|e| e.folder.is_none())),
            actions,
        );
        if !v.settings.folders.is_empty() {
            // Your folders, set apart from the built-in tabs.
            let (rect, _) = ui.allocate_exact_size(vec2(9.0, 30.0), Sense::hover());
            ui.painter().vline(
                rect.center().x,
                (rect.top() + 5.0)..=(rect.bottom() - 5.0),
                Stroke::new(1.0, p.border_strong),
            );
        }
        for (i, f) in v.settings.folders.iter().enumerate() {
            let n = count(&|e| e.folder == Some(f.id));
            let r = tab(
                ui,
                v,
                BoardTab::Folder(f.id),
                format!("{}  {}  {n}", icons::FOLDER, f.name),
                actions,
            );
            r.on_hover_text(
                "Right-click to rename, move or delete. Drop clips here to move them in.",
            )
            .context_menu(|ui| {
                folder_menu(ui, f.id, &f.name, i, v.settings.folders.len(), actions)
            });
        }
        let add = ui
            .add(
                egui::Button::new(
                    RichText::new(format!("{}  Folder", icons::ADD)).color(p.text_muted),
                )
                .corner_radius(15)
                .min_size(vec2(0.0, 30.0)),
            )
            .on_hover_text("New folder");
        if add.clicked() {
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
    let p = palette(ui);
    let key = Id::new(("rename_folder", id));
    let mut text = ui
        .data_mut(|d| d.get_temp::<String>(key))
        .unwrap_or_else(|| name.to_string());
    ui.label(RichText::new("Name").small().color(p.text_muted));
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
        .button(RichText::new(format!("{}  Delete folder", icons::DELETE)).color(p.danger))
        .on_hover_text("Its clips go to Unsorted; no clip or file is deleted. Can be undone.")
        .clicked()
    {
        actions.push(Action::DeleteFolder(id));
        ui.close();
    }
}

// ------------------------------------------------------------------------------------------
// The page
// ------------------------------------------------------------------------------------------

/// The page when there is no clip yet: one big place to drop files on, or click.
fn drop_zone(ui: &mut Ui, actions: &mut Vec<Action>) {
    let p = palette(ui);
    let size = vec2(ui.available_width(), 240.0);
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    #[cfg(test)]
    tests::remember(ui, ("drop_zone", "0".into()), rect);
    if ui.is_rect_visible(rect) {
        let hovered = r.hovered();
        let painter = ui.painter();
        let fill = if hovered {
            mix(p.surface, p.accent, 0.08)
        } else {
            p.surface
        };
        painter.rect_filled(rect, CornerRadius::same(RADIUS_M), fill);
        let edge = if hovered { p.accent } else { p.border_strong };
        let inner = rect.shrink(6.0);
        let corners = [
            inner.left_top(),
            inner.right_top(),
            inner.right_bottom(),
            inner.left_bottom(),
            inner.left_top(),
        ];
        painter.extend(egui::Shape::dashed_line(
            &corners,
            Stroke::new(1.5, edge),
            8.0,
            6.0,
        ));
        let c = rect.center();
        painter.text(
            c - vec2(0.0, 46.0),
            Align2::CENTER_CENTER,
            icons::DROP_FILES,
            FontId::proportional(44.0),
            if hovered { p.accent } else { p.text_faint },
        );
        painter.text(
            c + vec2(0.0, 8.0),
            Align2::CENTER_CENTER,
            "Drop wav, mp3, ogg or flac files here",
            FontId::proportional(font::SECTION),
            p.text,
        );
        painter.text(
            c + vec2(0.0, 34.0),
            Align2::CENTER_CENTER,
            "or click to pick them",
            FontId::proportional(font::BODY),
            p.text_muted,
        );
    }
    if r.on_hover_cursor(CursorIcon::PointingHand).clicked() {
        actions.push(Action::AddFiles);
    }
}

/// A line in the middle of an empty tab.
fn empty_note(ui: &mut Ui, text: &str) {
    let p = palette(ui);
    ui.add_space(space::XXL);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(text).color(p.text_muted));
    });
}

pub fn panel(ui: &mut Ui, v: &View, state: &mut State, search: &mut String) -> Vec<Action> {
    let mut actions = Vec::new();
    // A clip held with the mouse stops as soon as no button is down, wherever it was let go
    // (or if the window lost the release).
    if !ui.input(|i| i.pointer.any_down()) {
        actions.push(Action::MouseUp);
    }
    if v.shared.safe_mode.load(Relaxed) {
        widgets::notice(
            ui,
            icons::SAFE,
            "Safe mode: clips don't play until you press Restore (top right).",
            Tone::Danger,
        );
    }
    if !v.hotkeys_available {
        widgets::notice(
            ui,
            icons::KEYBOARD,
            "Global hotkeys are not available on this system: clips only play from here.",
            Tone::Warning,
        );
    }
    if v.board.sounds.is_empty() {
        drop_zone(ui, &mut actions);
        return actions;
    }

    toolbar(ui, v, search, &mut actions);
    capture_banner(ui, v, &mut actions);
    let entries = v.board.entries();
    tab_bar(ui, v, &entries, &mut actions);
    ui.add_space(space::XS);

    let shown = organize::visible(&entries, v.settings, search);
    if shown.is_empty() {
        empty_note(
            ui,
            match (search.trim().is_empty(), v.settings.tab) {
                (false, _) => "No clip matches the search.",
                (true, BoardTab::Favorites) => {
                    "No favorites yet: right-click a clip and add it to Favorites."
                }
                (true, BoardTab::Recent) => "Nothing played yet.",
                (true, _) => {
                    "Nothing here yet: drag clips onto this tab, or use Move to (right-click)."
                }
            },
        );
        return actions;
    }
    // Dropping onto another clip reorders (under another sort, what's shown becomes "My order"
    // first). Not in search results or Recent, whose order isn't yours to change.
    let reorderable = search.trim().is_empty() && v.settings.tab != BoardTab::Recent;
    clips(ui, v, state, &shown, reorderable, &mut actions);
    actions
}

/// Lays the clips out (grid or list) and draws the ones in view.
fn clips(
    ui: &mut Ui,
    v: &View,
    state: &mut State,
    shown: &[usize],
    reorderable: bool,
    actions: &mut Vec<Action>,
) {
    let list = v.settings.view == BoardView::List;
    let available = ui.available_width();
    let (cols, w, h, gap) = if list {
        // Rows stay short enough to read across: more columns on a wide window.
        let cols = ((available / 520.0).floor() as usize).clamp(1, 3);
        let w = (available - GAP * (cols as f32 - 1.0)) / cols as f32;
        (cols, w, ROW_H, vec2(GAP, ROW_GAP))
    } else {
        let (cols, w) = grid_columns(available, v.settings.pad_size);
        (cols, w, pad_height(v.settings.pad_size), vec2(GAP, GAP))
    };
    let rows = shown.len().div_ceil(cols);
    let total = rows as f32 * h + rows.saturating_sub(1) as f32 * gap.y;
    let (area, _) = ui.allocate_exact_size(vec2(available, total), Sense::hover());
    for (n, &i) in shown.iter().enumerate() {
        let (row, col) = (n / cols, n % cols);
        let min = area.min + vec2(col as f32 * (w + gap.x), row as f32 * (h + gap.y));
        let rect = Rect::from_min_size(min, vec2(w, h));
        // Only what is in view is drawn and listens to the mouse: a big board stays light.
        if ui.is_rect_visible(rect) {
            pad(
                ui,
                v,
                state,
                &v.board.sounds[i],
                rect,
                list,
                reorderable,
                actions,
            );
        }
    }
}

// ------------------------------------------------------------------------------------------
// One clip
// ------------------------------------------------------------------------------------------

/// What a clip looks like right now.
struct Look<'a> {
    rect: Rect,
    e: &'a SoundEntry,
    state: &'a LoadState,
    /// 0..1 while it plays.
    progress: Option<f32>,
    hovered: bool,
    editing: bool,
}

impl Look<'_> {
    fn fill(&self, p: &Palette) -> Color32 {
        let base = match tint(self.e) {
            Some(c) => mix(p.surface, c, 0.10),
            None => p.surface,
        };
        if self.progress.is_some() {
            mix(base, p.accent, 0.20)
        } else if self.hovered {
            mix(base, p.text, 0.06)
        } else {
            base
        }
    }

    fn border(&self, p: &Palette) -> Stroke {
        if self.progress.is_some() {
            Stroke::new(1.5, p.accent)
        } else if self.editing {
            Stroke::new(1.5, mix(p.border_strong, p.accent, 0.6))
        } else if self.hovered {
            Stroke::new(1.0, p.border_strong)
        } else {
            Stroke::new(1.0, p.border)
        }
    }

    /// The background: a surface with the clip's colour as a strip along the left edge.
    fn paint_body(&self, ui: &Ui, radius: u8, strip: f32) {
        let p = palette(ui);
        let painter = ui.painter();
        let fill = self.fill(p);
        match tint(self.e) {
            Some(c) => {
                // The colour first, then the surface over all but the strip: the strip gets
                // the pad's own rounded corners that way.
                painter.rect_filled(self.rect, CornerRadius::same(radius), c);
                let body = Rect::from_min_max(
                    pos2(self.rect.left() + strip, self.rect.top()),
                    self.rect.max,
                );
                let corners = CornerRadius {
                    nw: 0,
                    sw: 0,
                    ne: radius,
                    se: radius,
                };
                painter.rect_filled(body, corners, fill);
            }
            None => {
                painter.rect_filled(self.rect, CornerRadius::same(radius), fill);
            }
        }
        painter.rect_stroke(
            self.rect,
            CornerRadius::same(radius),
            self.border(p),
            StrokeKind::Inside,
        );
    }

    fn paint_progress(&self, ui: &Ui, inset: f32, from_bottom: f32, thickness: f32) {
        let Some(progress) = self.progress else {
            return;
        };
        let p = palette(ui);
        let bottom = self.rect.bottom() - from_bottom;
        let track = Rect::from_min_max(
            pos2(self.rect.left() + inset, bottom - thickness),
            pos2(self.rect.right() - inset, bottom),
        );
        let r = thickness / 2.0;
        ui.painter()
            .rect_filled(track, r, mix(p.surface, p.text, 0.14));
        let done = Rect::from_min_max(
            track.min,
            pos2(
                track.left() + track.width() * progress.clamp(0.0, 1.0),
                track.bottom(),
            ),
        );
        ui.painter().rect_filled(done, r, p.accent);
    }

    /// A pad of the grid: the name on up to two lines, the badges under it.
    fn paint_pad(&self, ui: &Ui) {
        let p = palette(ui);
        self.paint_body(ui, RADIUS_M, 5.0);
        let rect = self.rect;
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        let left = rect.left() + 14.0;
        let right = rect.right() - 10.0;
        let ready = matches!(self.state, LoadState::Ready(_));
        let name_color = if ready { p.text } else { p.text_muted };
        let name = widgets::elided(
            ui,
            &self.e.name,
            FontId::proportional(font::BODY),
            name_color,
            right - left,
            2,
        );
        painter.galley(pos2(left, rect.top() + 9.0), name, name_color);

        let cy = rect.bottom() - 19.0;
        let small = FontId::proportional(font::SMALL);
        match self.state {
            LoadState::Loading => {
                painter.text(
                    pos2(left, cy),
                    Align2::LEFT_CENTER,
                    "Loading\u{2026}",
                    small,
                    p.text_faint,
                );
            }
            LoadState::Failed(_) => {
                painter.text(
                    pos2(left, cy),
                    Align2::LEFT_CENTER,
                    format!("{}  File missing", icons::WARNING),
                    small,
                    p.warning,
                );
            }
            LoadState::Ready(_) => {
                let mut limit = right;
                if self.progress.is_some()
                    && let Some(time) = time_text(length_s(self.state), self.progress)
                {
                    let r = painter.text(
                        pos2(right, cy),
                        Align2::RIGHT_CENTER,
                        time,
                        small.clone(),
                        p.text,
                    );
                    limit = r.left() - 6.0;
                }
                let mut x = left;
                if self.e.favorite {
                    let r = painter.text(
                        pos2(x, cy),
                        Align2::LEFT_CENTER,
                        "\u{2605}",
                        FontId::proportional(font::BODY),
                        p.warning,
                    );
                    x = r.right() + 5.0;
                }
                let badge = self.e.play_mode.badge();
                if !badge.is_empty() && limit - x > 40.0 {
                    x += mini_pill(ui, x, cy, badge, p.accent, limit - x) + 5.0;
                }
                if let Some(key) = &self.e.hotkey
                    && limit - x > 34.0
                {
                    mini_pill(ui, x, cy, &key.pretty(), p.text_muted, limit - x);
                }
            }
        }
        self.paint_progress(ui, 10.0, 5.0, 3.0);
    }

    /// A row of the list: everything on one line.
    fn paint_row(&self, ui: &Ui) {
        let p = palette(ui);
        self.paint_body(ui, RADIUS_S, 4.0);
        let rect = self.rect;
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        let cy = rect.center().y;
        let playing = self.progress.is_some();
        let (icon, icon_color) = match self.state {
            LoadState::Ready(_) if playing => (icons::STOP, p.accent),
            LoadState::Ready(_) if self.hovered => (icons::PLAY, p.text),
            LoadState::Ready(_) => (icons::PLAY, p.text_faint),
            LoadState::Loading => ("\u{2026}", p.text_faint),
            LoadState::Failed(_) => (icons::WARNING, p.warning),
        };
        painter.text(
            pos2(rect.left() + 22.0, cy),
            Align2::CENTER_CENTER,
            icon,
            FontId::proportional(15.0),
            icon_color,
        );
        let left = rect.left() + 40.0;
        // From the right: room for the pencil and the grip, the time, then the badges.
        let mut right = rect.right() - (2.0 * TOOL + 14.0);
        let small = FontId::proportional(font::SMALL);
        let status = match self.state {
            LoadState::Loading => Some(("Loading\u{2026}".to_string(), p.text_faint)),
            LoadState::Failed(_) => Some(("File missing".to_string(), p.warning)),
            LoadState::Ready(_) => time_text(length_s(self.state), self.progress)
                .map(|t| (t, if playing { p.text } else { p.text_faint })),
        };
        if let Some((text, color)) = status {
            let r = painter.text(pos2(right, cy), Align2::RIGHT_CENTER, text, small, color);
            right = right.min(r.left()) - 8.0;
        }
        // (Narrow rows keep the name: badges only show while there is room for them.)
        let room = |right: f32, w: f32| right - w - left > 120.0;
        if let Some(key) = &self.e.hotkey {
            let text = key.pretty();
            let w = mini_pill_width(ui, &text);
            if room(right, w) {
                mini_pill(ui, right - w, cy, &text, p.text_muted, w);
                right -= w + 6.0;
            }
        }
        let badge = self.e.play_mode.badge();
        if !badge.is_empty() {
            let w = mini_pill_width(ui, badge);
            if room(right, w) {
                mini_pill(ui, right - w, cy, badge, p.accent, w);
                right -= w + 6.0;
            }
        }
        if self.e.favorite {
            let r = painter.text(
                pos2(right, cy),
                Align2::RIGHT_CENTER,
                "\u{2605}",
                FontId::proportional(font::BODY),
                p.warning,
            );
            right = r.left() - 6.0;
        }
        let ready = matches!(self.state, LoadState::Ready(_));
        let name_color = if ready { p.text } else { p.text_muted };
        let name = widgets::elided(
            ui,
            &self.e.name,
            FontId::proportional(font::BODY),
            name_color,
            (right - left).max(20.0),
            1,
        );
        let at = pos2(left, cy - name.size().y / 2.0);
        painter.galley(at, name, name_color);
        self.paint_progress(ui, 8.0, 2.0, 2.0);
    }
}

/// The pencil and the grip that appear on a clip under the pointer.
fn paint_tools(ui: &Ui, grip: &Response, edit: &Response, editing: bool) {
    let p = palette(ui);
    let painter = ui.painter();
    let back = grip.rect.union(edit.rect).expand(2.0);
    painter.rect(
        back,
        CornerRadius::same(RADIUS_S),
        p.surface_raised,
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
    let dots = if grip.hovered() || grip.dragged() {
        p.text
    } else {
        p.text_muted
    };
    let c = grip.rect.center();
    for row in [-5.0, 0.0, 5.0] {
        for col in [-3.0, 3.0] {
            painter.circle_filled(c + vec2(col, row), 1.6, dots);
        }
    }
    let pencil = if editing {
        p.accent
    } else if edit.hovered() {
        p.text
    } else {
        p.text_muted
    };
    painter.text(
        edit.rect.center(),
        Align2::CENTER_CENTER,
        icons::EDIT,
        FontId::proportional(15.0),
        pencil,
    );
}

/// What a clip says when the pointer rests on it.
fn pad_tip(ui: &mut Ui, s: &Sound) {
    let p = palette(ui);
    let e = &s.entry;
    ui.set_max_width(340.0);
    ui.label(RichText::new(&e.name).strong());
    ui.label(RichText::new(e.file_name()).small().color(p.text_muted));
    match &s.state {
        LoadState::Ready(_) => {
            let length = length_s(&s.state).unwrap_or(0.0);
            ui.label(
                RichText::new(format!(
                    "{length:.1} s  \u{b7}  played {} time{}",
                    e.plays,
                    if e.plays == 1 { "" } else { "s" }
                ))
                .small()
                .color(p.text_muted),
            );
            ui.label(match e.play_mode {
                PlayMode::Normal => "Click to play, again to restart.",
                PlayMode::Hold => "Plays while you hold the mouse button (or the key).",
                PlayMode::Toggle => "Click to play, again to stop.",
                PlayMode::LoopWhileHeld => "Loops while you hold the mouse button (or the key).",
            });
            ui.label(
                RichText::new("Right-click for more  \u{b7}  the pencil edits it")
                    .small()
                    .color(p.text_muted),
            );
        }
        LoadState::Loading => {
            ui.label("Loading\u{2026}");
        }
        LoadState::Failed(err) => {
            ui.label(RichText::new(err).color(p.warning));
            ui.label(
                RichText::new(e.path.display().to_string())
                    .small()
                    .color(p.text_muted),
            );
            ui.label("Click to point to the file again.");
        }
    }
}

/// One clip, as a pad (`list` = false) or a list row, in `rect`.
#[allow(clippy::too_many_arguments)]
fn pad(
    ui: &mut Ui,
    v: &View,
    state: &mut State,
    s: &Sound,
    rect: Rect,
    list: bool,
    reorderable: bool,
    actions: &mut Vec<Action>,
) {
    let (id, e) = (s.id, &s.entry);
    let pad_id = Id::new(("clip_pad", id));
    let ready = matches!(s.state, LoadState::Ready(_));
    let missing = matches!(s.state, LoadState::Failed(_));
    let progress = v.shared.progress_of(id);
    let editing = state.editing() == Some(id);

    // The clip itself only takes clicks, so a slightly moving click still plays it. The
    // pencil and the grip come after it and lie on top: they get their own clicks and drags.
    let r = ui.interact(rect, pad_id, Sense::click());
    let tools_top = if list {
        rect.center().y - TOOL / 2.0
    } else {
        rect.top() + 6.0
    };
    let edit_rect = Rect::from_min_size(
        pos2(rect.right() - 6.0 - TOOL, tools_top),
        Vec2::splat(TOOL),
    );
    let grip_rect = edit_rect.translate(vec2(-TOOL, 0.0));
    let grip = ui.interact(grip_rect, pad_id.with("grip"), Sense::click_and_drag());
    let edit = ui.interact(edit_rect, pad_id.with("edit"), Sense::click());
    #[cfg(test)]
    {
        tests::remember(ui, ("clip", id.to_string()), rect);
        tests::remember(ui, ("card", id.to_string()), rect);
        tests::remember(ui, ("grip", id.to_string()), grip_rect);
        tests::remember(ui, ("pencil", id.to_string()), edit_rect);
    }
    let enabled = ui.is_enabled();
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, e.name.as_str()));

    let on_tools = ui
        .input(|i| i.pointer.interact_pos())
        .is_some_and(|at| grip_rect.union(edit_rect).contains(at));
    if edit.clicked() {
        if editing {
            state.close();
        } else {
            state.open(id, &e.name, false);
        }
    }
    // Shift+click: hear it in the headphones only, whatever its play mode.
    let shift = ui.input(|i| i.modifiers.shift);
    if ready && shift {
        if r.clicked() {
            actions.push(Action::Preview(id));
        }
    } else if ready {
        if e.play_mode.is_held() {
            // Plays while the mouse button is held on it (like the hotkey).
            if r.hovered() && !on_tools && ui.input(|i| i.pointer.primary_pressed()) {
                actions.push(Action::HoldDown(id));
            }
        } else if r.clicked() {
            actions.push(Action::Play(id));
        }
    } else if missing && r.clicked() {
        actions.push(Action::Locate(id));
    }
    r.context_menu(|ui| clip_menu(ui, v, state, s, actions));

    // The grip drags the clip onto a tab or onto another clip; its name follows the pointer.
    grip.dnd_set_drag_payload(ClipDrag(id));
    if grip.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        if let Some(at) = ui.ctx().pointer_interact_pos() {
            egui::Area::new(Id::new(("clip_drag", id)))
                .order(egui::Order::Tooltip)
                .fixed_pos(at + vec2(14.0, 10.0))
                .interactable(false)
                .show(ui.ctx(), |ui| {
                    Frame::popup(ui.style()).show(ui, |ui| {
                        ui.label(RichText::new(&e.name).strong());
                    });
                });
        }
    } else if grip.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
    }
    let mut drop_here = false;
    if reorderable {
        if let Some(drag) = r.dnd_hover_payload::<ClipDrag>()
            && drag.0 != id
        {
            drop_here = true;
        }
        if let Some(drag) = r.dnd_release_payload::<ClipDrag>()
            && drag.0 != id
        {
            actions.push(Action::Reorder(drag.0, id));
        }
    }

    if ui.is_rect_visible(rect) {
        let p = palette(ui);
        let dragging = egui::DragAndDrop::has_any_payload(ui.ctx());
        let under_pointer = ui.rect_contains_pointer(rect);
        let look = Look {
            rect,
            e,
            state: &s.state,
            progress,
            hovered: under_pointer && !dragging,
            editing,
        };
        if list {
            look.paint_row(ui);
        } else {
            look.paint_pad(ui);
        }
        if (under_pointer && !dragging) || editing || grip.dragged() {
            paint_tools(ui, &grip, &edit, editing);
        }
        let radius = if list { RADIUS_S } else { RADIUS_M };
        if drop_here {
            ui.painter().rect_stroke(
                rect,
                CornerRadius::same(radius),
                Stroke::new(2.0, p.accent),
                StrokeKind::Outside,
            );
        } else if r.has_focus() {
            ui.painter().rect_stroke(
                rect.expand(2.0),
                CornerRadius::same(radius + 2),
                Stroke::new(1.5, p.accent),
                StrokeKind::Outside,
            );
        }
    }

    grip.on_hover_text(
        "Drag onto another clip to put it there, or onto a folder tab to move it into that \
         folder",
    );
    edit.on_hover_text(if editing {
        "Close the edit panel"
    } else {
        "Edit: name, volume, hotkey, play mode, colour, folder"
    });
    let r = if ready {
        r.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        r
    };
    r.on_hover_ui(|ui| pad_tip(ui, s));
}

/// A clip's right-click menu.
fn clip_menu(ui: &mut Ui, v: &View, state: &mut State, s: &Sound, actions: &mut Vec<Action>) {
    let p = palette(ui);
    let (id, e) = (s.id, &s.entry);
    ui.set_min_width(230.0);
    if v.shared.is_playing(id) && ui.button(format!("{}  Stop", icons::STOP)).clicked() {
        actions.push(Action::Stop(id));
        ui.close();
    }
    let previewing = v.board.previewing() == Some(id);
    let listen = if previewing {
        format!("{}  Stop the preview", icons::HEADPHONES)
    } else {
        format!("{}  Preview in my headphones", icons::HEADPHONES)
    };
    if ui
        .add_enabled(
            matches!(s.state, LoadState::Ready(_)),
            egui::Button::new(listen),
        )
        .on_hover_text("Only you hear it; it does not go to the virtual mic (Shift+click)")
        .clicked()
    {
        actions.push(Action::Preview(id));
        ui.close();
    }
    if ui
        .button(format!("{}  Edit\u{2026}", icons::EDIT))
        .on_hover_text("Everything about this clip in a panel on the right")
        .clicked()
    {
        state.open(id, &e.name, false);
        ui.close();
    }
    if ui.button("      Rename\u{2026}").clicked() {
        state.open(id, &e.name, true);
        ui.close();
    }
    let star = if e.favorite {
        "\u{2606}   Remove from Favorites"
    } else {
        "\u{2605}   Add to Favorites"
    };
    if ui.button(star).clicked() {
        actions.push(Action::ToggleFavorite(id));
        ui.close();
    }
    ui.separator();
    ui.menu_button(format!("{}  Move to", icons::FOLDER), |ui| {
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
            widgets::hint(ui, "No folders yet: \"+ Folder\" makes one");
        }
    });
    ui.menu_button(format!("{}  Play mode", icons::PLAY_MODE), |ui| {
        for m in PlayMode::ALL {
            if ui.selectable_label(e.play_mode == m, m.label()).clicked() {
                actions.push(Action::PlayMode(id, m));
                ui.close();
            }
        }
        widgets::hint(
            ui,
            "Hold and loop work with the hotkey and with the mouse button.",
        );
    });
    ui.menu_button(format!("{}  Colour", icons::COLOR), |ui| {
        if ui.selectable_label(e.color.is_none(), "None").clicked() {
            actions.push(Action::Color(id, None));
            ui.close();
        }
        for (i, (name, c)) in PALETTE.iter().enumerate() {
            let text = RichText::new(format!("\u{25cf}  {name}")).color(*c);
            if ui
                .selectable_label(e.color == Some(i as u8), text)
                .clicked()
            {
                actions.push(Action::Color(id, Some(i as u8)));
                ui.close();
            }
        }
    });
    let key_label = match &e.hotkey {
        Some(k) => format!("{}  Hotkey: {}\u{2026}", icons::KEYBOARD, k.pretty()),
        None => format!("{}  Set a hotkey\u{2026}", icons::KEYBOARD),
    };
    if ui
        .add_enabled(v.hotkeys_available, egui::Button::new(key_label))
        .on_hover_text("Click, then press the key or combination to use")
        .clicked()
    {
        actions.push(Action::BeginHotkey(id));
        ui.close();
    }
    if e.hotkey.is_some() && ui.button("      Remove the hotkey").clicked() {
        actions.push(Action::ClearHotkey(id));
        ui.close();
    }
    ui.separator();
    let mut volume = e.volume;
    if volume_slider(ui, &mut volume).changed() {
        actions.push(Action::Volume(id, volume));
    }
    if matches!(s.state, LoadState::Failed(_)) {
        ui.separator();
        if ui
            .button(format!("{}  Locate file\u{2026}", icons::LOCATE))
            .clicked()
        {
            actions.push(Action::Locate(id));
            ui.close();
        }
    }
    ui.separator();
    if ui
        .button(RichText::new(format!("{}  Delete", icons::DELETE)).color(p.danger))
        .on_hover_text("Takes the clip off the soundboard (the file stays). Can be undone.")
        .clicked()
    {
        actions.push(Action::Remove(id));
        ui.close();
    }
}

fn volume_slider(ui: &mut Ui, volume: &mut f32) -> Response {
    widgets::Slider::new(volume, 0.0..=1.5)
        .label("Volume")
        .percent(true)
        .default_value(VOLUME_DEFAULT)
        .label_width(64.0)
        .tip("How loud this clip plays, for others and in your headphones")
        .show(ui)
}

// ------------------------------------------------------------------------------------------
// The "Now playing" strip and the edit panel
// ------------------------------------------------------------------------------------------

/// One playing clip in the strip; a click stops it.
/// `preview`: it plays in the headphones only (it shows a headphones icon until hovered).
fn now_chip(ui: &mut Ui, s: &Sound, progress: f32, preview: bool) -> Response {
    let p = palette(ui);
    let time = time_text(length_s(&s.state), Some(progress)).unwrap_or_default();
    let font = FontId::proportional(font::SMALL + 0.5);
    let name = widgets::elided(ui, &s.entry.name, font.clone(), p.text, 150.0, 1);
    let time_galley = ui.painter().layout_no_wrap(time, font, p.text_muted);
    let width = 30.0 + name.size().x + 10.0 + time_galley.size().x + 12.0;
    let (rect, r) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let radius = rect.height() / 2.0;
        let hovered = r.hovered();
        let fill = if hovered {
            mix(p.surface_raised, p.danger, 0.18)
        } else {
            p.surface_raised
        };
        painter.rect_filled(rect, radius, fill);
        // The part that has played is tinted: the chip is its own progress bar.
        let done = Rect::from_min_max(
            rect.min,
            pos2(
                rect.left() + rect.width() * progress.clamp(0.0, 1.0),
                rect.bottom(),
            ),
        );
        painter
            .with_clip_rect(done.intersect(ui.clip_rect()))
            .rect_filled(rect, radius, mix(fill, p.accent, 0.30));
        let edge = if hovered { p.danger } else { p.border_strong };
        painter.rect_stroke(rect, radius, Stroke::new(1.0, edge), StrokeKind::Inside);
        let cy = rect.center().y;
        let icon = if preview && !hovered {
            icons::HEADPHONES
        } else {
            icons::STOP
        };
        painter.text(
            pos2(rect.left() + 16.0, cy),
            Align2::CENTER_CENTER,
            icon,
            FontId::proportional(13.0),
            if hovered { p.danger } else { p.accent },
        );
        let at = pos2(rect.left() + 30.0, cy - name.size().y / 2.0);
        let name_w = name.size().x;
        painter.galley(at, name, p.text);
        let at = pos2(
            rect.left() + 30.0 + name_w + 10.0,
            cy - time_galley.size().y / 2.0,
        );
        painter.galley(at, time_galley, p.text_muted);
    }
    let tip = if preview {
        format!(
            "Stop the preview of \"{}\" (only you hear it)",
            s.entry.name
        )
    } else {
        format!("Stop \"{}\"", s.entry.name)
    };
    r.on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(tip)
}

/// The strip under the page: what is playing, each with its own stop, and Stop all.
fn now_playing(ui: &mut Ui, v: &View, actions: &mut Vec<Action>) {
    let p = palette(ui);
    let playing: Vec<(&Sound, f32)> = v
        .shared
        .playing()
        .into_iter()
        .filter_map(|id| Some((v.sound(id)?, v.shared.progress_of(id).unwrap_or(0.0))))
        .collect();
    let preview = v.board.previewing().and_then(|id| v.sound(id));
    egui::Panel::bottom("now_playing")
        .exact_size(NOW_H)
        .frame(
            Frame::new()
                .fill(p.surface)
                .inner_margin(Margin::symmetric(16, 0)),
        )
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                let lit = if playing.is_empty() {
                    p.text_faint
                } else {
                    p.accent
                };
                ui.label(RichText::new(icons::VOLUME).size(17.0).color(lit));
                ui.label(RichText::new("Now playing").strong().color(p.text_muted));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let tip = match v.stop_key {
                        Some(k) => format!("Stop every clip ({})", k.pretty()),
                        None => "Stop every clip".to_string(),
                    };
                    let stop = ui
                        .add_enabled_ui(!playing.is_empty(), |ui| {
                            widgets::danger_button(ui, &format!("{}  Stop all", icons::STOP))
                        })
                        .inner
                        .on_hover_text(tip)
                        .on_disabled_hover_text("Nothing is playing");
                    #[cfg(test)]
                    tests::remember(ui, ("now", "stop_all".into()), stop.rect);
                    if stop.clicked() {
                        actions.push(Action::StopAll);
                    }
                    ui.add_space(space::S);
                    let mut master = v.settings.master_volume;
                    let r = widgets::Slider::new(&mut master, 0.0..=1.0)
                        .label("Master")
                        .percent(true)
                        .default_value(1.0)
                        .tip(
                            "Volume of every clip, for others and in your headphones (each \
                             clip's own volume comes on top)",
                        )
                        .compact(if ui.available_width() > 640.0 {
                            120.0
                        } else {
                            80.0
                        })
                        .show(ui);
                    #[cfg(test)]
                    tests::remember(ui, ("now", "master".into()), r.rect);
                    if r.changed() {
                        actions.push(Action::MasterVolume(master));
                    }
                    // (Right to left: this lands left of the track.)
                    ui.label(
                        RichText::new(format!("Master {:.0} %", master * 100.0))
                            .size(font::SMALL)
                            .color(p.text_muted),
                    );
                    ui.add_space(space::S);
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        if playing.is_empty() && preview.is_none() {
                            ui.label(RichText::new("Nothing is playing").color(p.text_faint));
                            return;
                        }
                        egui::ScrollArea::horizontal()
                            .id_salt("now_playing_clips")
                            .show(ui, |ui| {
                                if let Some(s) = preview {
                                    let progress = v.shared.listen_progress.get();
                                    let r = now_chip(ui, s, progress, true);
                                    #[cfg(test)]
                                    tests::remember(ui, ("now", "preview".into()), r.rect);
                                    if r.clicked() {
                                        actions.push(Action::StopPreview);
                                    }
                                }
                                for (s, progress) in &playing {
                                    let r = now_chip(ui, s, *progress, false);
                                    #[cfg(test)]
                                    tests::remember(ui, ("now", s.id.to_string()), r.rect);
                                    if r.clicked() {
                                        actions.push(Action::Stop(s.id));
                                    }
                                }
                            });
                    });
                });
            });
        });
}

/// A label in front of a group of controls in the edit panel.
fn field(ui: &mut Ui, text: &str) {
    let p = palette(ui);
    ui.add_space(space::S);
    ui.label(RichText::new(text).small().strong().color(p.text_muted));
}

/// One colour to pick; `None` is "no colour".
fn swatch(ui: &mut Ui, color: Option<Color32>, name: &str, selected: bool) -> Response {
    let p = palette(ui);
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let c = rect.center();
        match color {
            Some(color) => {
                painter.circle_filled(c, 8.0, color);
            }
            None => {
                painter.circle_stroke(c, 7.5, Stroke::new(1.5, p.border_strong));
                painter.line_segment(
                    [c + vec2(-5.0, 5.0), c + vec2(5.0, -5.0)],
                    Stroke::new(1.5, p.border_strong),
                );
            }
        }
        if selected {
            painter.circle_stroke(c, 10.5, Stroke::new(1.5, p.text));
        } else if r.hovered() {
            painter.circle_stroke(c, 10.5, Stroke::new(1.0, p.border_strong));
        }
    }
    r.on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(name)
}

/// Everything about one clip.
/// "Trim & fade" in the edit panel: where the clip starts and ends, and its fades. Folded
/// away until used. `length`: the whole file, in seconds.
fn trim_section(ui: &mut Ui, id: SoundId, trim: &Trim, length: f32, actions: &mut Vec<Action>) {
    let p = palette(ui);
    ui.add_space(space::XS);
    let header = if trim.is_none() {
        "Trim & fade".to_string()
    } else {
        let kept = length - trim.start - trim.end;
        format!("Trim & fade  \u{b7}  plays {kept:.1} of {length:.1} s")
    };
    let r = egui::CollapsingHeader::new(RichText::new(header).small().strong().color(p.text_muted))
        .id_salt(("trim", id))
        .default_open(!trim.is_none())
        .show(ui, |ui| {
            let mut t = *trim;
            let mut changed = false;
            let mut row = |ui: &mut Ui, value: &mut f32, max: f32, label: &str, tip: &str| {
                let r = widgets::Slider::new(value, 0.0..=max.max(0.01))
                    .label(label)
                    .unit(" s")
                    .decimals(2)
                    .default_value(0.0)
                    .label_width(64.0)
                    .tip(tip)
                    .show(ui);
                #[cfg(test)]
                tests::remember(ui, ("trim", label.to_string()), r.rect);
                changed |= r.changed();
            };
            row(
                ui,
                &mut t.start,
                length,
                "Start",
                "Skip this much at the start",
            );
            row(ui, &mut t.end, length, "End", "Cut this much off the end");
            row(
                ui,
                &mut t.fade_in,
                MAX_FADE_S,
                "Fade in",
                "Rise from silence over this time",
            );
            row(
                ui,
                &mut t.fade_out,
                MAX_FADE_S,
                "Fade out",
                "Fall to silence over this time",
            );
            if changed {
                actions.push(Action::Trim(id, t));
            }
            if !trim.is_none() && ui.small_button("Use the whole clip").clicked() {
                actions.push(Action::Trim(id, Trim::default()));
            }
        });
    r.header_response.on_hover_text(
        "Cut silence or noise off the ends and soften them. The file is not changed.",
    );
}

fn edit_panel(ui: &mut Ui, v: &View, state: &mut State, s: &Sound, actions: &mut Vec<Action>) {
    let p = palette(ui);
    let (id, e) = (s.id, &s.entry);
    let mut close = false;
    ui.horizontal(|ui| {
        widgets::section_title(ui, icons::EDIT, "Edit clip");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let x = widgets::icon_button(ui, icons::CLOSE, "Close");
            #[cfg(test)]
            tests::remember(ui, ("edit", "close".into()), x.rect);
            close = x.clicked();
        });
    });

    field(ui, "Name");
    if let Some(editing) = state.editing.as_mut() {
        let r = ui.add(
            egui::TextEdit::singleline(&mut editing.name)
                .hint_text("Clip name")
                .desired_width(f32::INFINITY),
        );
        #[cfg(test)]
        tests::remember(ui, ("edit", "name".into()), r.rect);
        if std::mem::take(&mut editing.focus_name) {
            r.request_focus();
        }
        // The clip follows while you type; an empty field leaves the name as it was.
        if r.changed()
            && let Some(name) = names::typed_name(&editing.name)
            && name != e.name
        {
            actions.push(Action::Rename(id, name));
        }
        if r.lost_focus() && names::typed_name(&editing.name).is_none() {
            editing.name = e.name.clone();
        }
    }
    ui.add(
        egui::Label::new(
            RichText::new(format!("File: {}", e.file_name()))
                .small()
                .color(p.text_muted),
        )
        .truncate(),
    )
    .on_hover_text(format!(
        "{}\nRenaming a clip never renames its file.",
        e.path.display()
    ));

    ui.add_space(space::S);
    ui.horizontal(|ui| {
        match &s.state {
            LoadState::Ready(_) if v.shared.is_playing(id) => {
                if ui.button(format!("{}  Stop", icons::STOP)).clicked() {
                    actions.push(Action::Stop(id));
                }
            }
            LoadState::Ready(_) if e.play_mode.is_held() => {
                let r = ui
                    .button(format!("{}  Hold to play", icons::PLAY))
                    .on_hover_text("Plays while you hold the mouse button here");
                if r.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed()) {
                    actions.push(Action::HoldDown(id));
                }
            }
            LoadState::Ready(_) => {
                if ui.button(format!("{}  Play", icons::PLAY)).clicked() {
                    actions.push(Action::Play(id));
                }
            }
            LoadState::Loading => {
                ui.label(RichText::new("Loading\u{2026}").color(p.text_muted));
            }
            LoadState::Failed(_) => {
                if ui
                    .button(format!("{}  Locate file\u{2026}", icons::LOCATE))
                    .on_hover_text("The file was moved or deleted: point to it again")
                    .clicked()
                {
                    actions.push(Action::Locate(id));
                }
            }
        }
        if let Some(length) = length_s(&s.state) {
            ui.label(
                RichText::new(format!("{length:.1} s  \u{b7}  played {}\u{d7}", e.plays))
                    .small()
                    .color(p.text_muted),
            );
        }
    });
    if let LoadState::Failed(err) = &s.state {
        widgets::notice(ui, icons::WARNING, err, Tone::Warning);
    }

    ui.add_space(space::S);
    let mut volume = e.volume;
    if volume_slider(ui, &mut volume).changed() {
        actions.push(Action::Volume(id, volume));
    }
    if let Some(level) = s.level {
        let text = if v.settings.normalize {
            format!(
                "Loudness {:.0} dB, evened out by {:+.1} dB",
                level.loudness_db,
                level.gain_db()
            )
        } else {
            format!("Loudness {:.0} dB", level.loudness_db)
        };
        widgets::hint(ui, &text).on_hover_text(
            "\"Even out loudness\" (in the \u{22ef} menu above the clips) plays every clip at \
             about the same level",
        );
    }
    if let Some(length) = s.raw_length_s() {
        trim_section(ui, id, &e.trim, length, actions);
    }

    field(ui, "Hotkey");
    ui.horizontal(|ui| {
        if v.capturing_sound() == Some(id) {
            if widgets::hotkey_button(ui, KeySlot::Capturing)
                .on_hover_text("Press the key or combination to use. Click to cancel.")
                .clicked()
            {
                actions.push(Action::CancelHotkey);
            }
            return;
        }
        let parts = e.hotkey.as_ref().map(Hotkey::parts);
        let (slot, tip) = match &parts {
            Some(parts) => (
                KeySlot::Set(parts),
                "Click, then press another key or combination",
            ),
            None => (
                KeySlot::Empty,
                "No hotkey. Click, then press the key or combination to use",
            ),
        };
        let clicked = ui
            .add_enabled_ui(v.hotkeys_available, |ui| {
                widgets::hotkey_button(ui, slot)
                    .on_hover_text(tip)
                    .clicked()
            })
            .inner;
        if clicked {
            actions.push(Action::BeginHotkey(id));
        }
        if e.hotkey.is_some() && ui.small_button("Clear").clicked() {
            actions.push(Action::ClearHotkey(id));
        }
    });

    field(ui, "Play mode");
    let mut mode = e.play_mode;
    egui::ComboBox::from_id_salt("clip_play_mode")
        .selected_text(mode.label())
        .width(ui.available_width().min(240.0))
        .show_ui(ui, |ui| {
            for m in PlayMode::ALL {
                ui.selectable_value(&mut mode, m, m.label());
            }
        })
        .response
        .on_hover_text("Hold and loop work with the hotkey and with the mouse button.");
    if mode != e.play_mode {
        actions.push(Action::PlayMode(id, mode));
    }

    field(ui, "Colour");
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(1.0, 3.0);
        if swatch(ui, None, "None", e.color.is_none()).clicked() && e.color.is_some() {
            actions.push(Action::Color(id, None));
        }
        for (i, (name, c)) in PALETTE.iter().enumerate() {
            let this = Some(i as u8);
            if swatch(ui, Some(*c), name, e.color == this).clicked() && e.color != this {
                actions.push(Action::Color(id, this));
            }
        }
    });

    field(ui, "Folder");
    let current = e
        .folder
        .and_then(|f| v.settings.folder_name(f))
        .unwrap_or("Unsorted");
    let mut folder = e.folder;
    egui::ComboBox::from_id_salt("clip_folder")
        .selected_text(current)
        .width(ui.available_width().min(240.0))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut folder, None, "Unsorted");
            for f in &v.settings.folders {
                ui.selectable_value(&mut folder, Some(f.id), &f.name);
            }
        });
    if folder != e.folder {
        actions.push(Action::MoveTo(id, folder));
    }

    ui.add_space(space::M);
    ui.horizontal(|ui| {
        let mut favorite = e.favorite;
        if widgets::toggle(ui, &mut favorite).changed() {
            actions.push(Action::ToggleFavorite(id));
        }
        ui.label("\u{2605}  Favorite");
    });

    ui.add_space(space::L);
    ui.separator();
    let delete = ui
        .button(RichText::new(format!("{}  Delete clip", icons::DELETE)).color(p.danger))
        .on_hover_text("Takes the clip off the soundboard (the file stays). Can be undone.");
    #[cfg(test)]
    tests::remember(ui, ("edit", "delete".into()), delete.rect);
    if delete.clicked() {
        actions.push(Action::Remove(id));
        close = true;
    }
    if close {
        state.close();
    }
}

/// What lies around the page: the "Now playing" strip under it and, while a clip is being
/// edited, the edit panel on the right. Call before the page itself is laid out.
pub fn side_panels(ui: &mut Ui, v: &View, state: &mut State) -> Vec<Action> {
    let mut actions = Vec::new();
    let p = palette(ui);
    // The clip in the edit panel was deleted (here, or by an import elsewhere): close it.
    if state.editing().is_some_and(|id| v.sound(id).is_none()) {
        state.close();
    }
    now_playing(ui, v, &mut actions);
    #[cfg(test)]
    ui.ctx()
        .data_mut(|d| d.remove::<Rect>(Id::new(("test_rect", "edit", "panel".to_string()))));
    if let Some(s) = state.editing().and_then(|id| v.sound(id)) {
        // (A narrow window keeps most of its width for the clips.)
        let width = EDIT_W.min(ui.available_width() * 0.45).max(220.0);
        let shown = egui::Panel::right("clip_edit")
            .exact_size(width)
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(p.surface)
                    .inner_margin(Margin::symmetric(16, 14)),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| edit_panel(ui, v, state, s, &mut actions));
            });
        #[cfg(test)]
        tests::remember(ui, ("edit", "panel".into()), shown.response.rect);
        let _ = shown;
    }
    actions
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Notes where a widget was drawn, so UI tests can aim real mouse events at it.
    pub fn remember(ui: &Ui, key: (&str, String), rect: egui::Rect) {
        ui.ctx()
            .data_mut(|d| d.insert_temp(Id::new(("test_rect", key.0, key.1)), rect));
    }

    /// Where `remember` saw the widget last frame, e.g. `rect_of(ctx, "clip", id)`.
    pub fn rect_of(ctx: &egui::Context, kind: &str, key: impl std::fmt::Display) -> egui::Rect {
        try_rect_of(ctx, kind, &key).unwrap_or_else(|| panic!("{kind} {key} was not drawn"))
    }

    /// Like [`rect_of`], for things that are only there sometimes.
    pub fn try_rect_of(
        ctx: &egui::Context,
        kind: &str,
        key: impl std::fmt::Display,
    ) -> Option<egui::Rect> {
        ctx.data(|d| d.get_temp(Id::new(("test_rect", kind, key.to_string()))))
    }

    #[test]
    fn times_are_shown_as_minutes_and_seconds() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(0.2), "0:01", "something is still left");
        assert_eq!(clock(7.0), "0:07");
        assert_eq!(clock(61.5), "1:02");
        assert_eq!(clock(-3.0), "0:00");
        assert_eq!(time_text(None, Some(0.5)), None, "not loaded: no time");
        assert_eq!(time_text(Some(10.0), None).as_deref(), Some("0:10"));
        assert_eq!(time_text(Some(10.0), Some(0.25)).as_deref(), Some("0:08"));
        assert_eq!(time_text(Some(10.0), Some(7.0)).as_deref(), Some("0:00"));
    }

    #[test]
    fn pads_fill_the_row_and_never_get_narrower_than_asked() {
        for (available, pad) in [
            (1200.0, 150.0),
            (432.0, 110.0),
            (2400.0, 260.0),
            (700.0, 151.0),
        ] {
            let (cols, w) = grid_columns(available, pad);
            assert!(w >= pad - 0.01, "{available}/{pad}: {w}");
            let used = cols as f32 * w + (cols as f32 - 1.0) * GAP;
            assert!((used - available).abs() < 0.01, "the row ends at the edge");
            // One more column would not fit.
            assert!((cols + 1) as f32 * pad + cols as f32 * GAP > available);
        }
        // Narrower than one pad: still one column, as wide as there is room.
        assert_eq!(grid_columns(90.0, 150.0), (1, 90.0));
        assert_eq!(pad_height(150.0), 76.0);
        assert_eq!(pad_height(260.0), 112.0);
    }
}
