//! Keyboard shortcuts inside the window (the global hotkeys live in `soundboard::hotkeys`):
//! Ctrl+1..5 switch pages, Ctrl+F searches, Space switches the voice changer, Delete removes
//! the clip in the edit panel, and `?` (or F1) shows the list of them.
//!
//! The single-key shortcuts only count while no text field has the keyboard, so typing a
//! space in a clip name never switches the voice changer.

use super::theme::{font, palette, space};
use super::widgets;
use crate::config::Page;
use egui::{Context, Key, Modifiers, RichText};

/// What a shortcut asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shortcut {
    Page(Page),
    /// Ctrl+F away from a page with a search box: the soundboard's search.
    Search,
    ToggleVoice,
    /// Delete: remove the clip that is open in the edit panel (it can be undone).
    DeleteClip,
    Help,
}

/// One line of the overlay: the key combinations (any of them works) and what they do.
pub struct Entry {
    pub keys: &'static [&'static [&'static str]],
    pub what: &'static str,
}

/// Every shortcut, as the overlay lists it.
pub const LIST: &[Entry] = &[
    Entry {
        keys: &[&["Ctrl", "1\u{2026}5"]],
        what: "Switch page (Voice, Soundboard, Audio setup, Hotkeys, Settings)",
    },
    Entry {
        keys: &[&["Ctrl", "F"]],
        what: "Search clips (or hotkeys, on the Hotkeys page)",
    },
    Entry {
        keys: &[&["Space"]],
        what: "Switch the voice changer on or off",
    },
    Entry {
        keys: &[&["Delete"]],
        what: "Delete the clip open in the edit panel (Undo brings it back)",
    },
    Entry {
        keys: &[&["?"], &["F1"]],
        what: "Show this list",
    },
    Entry {
        keys: &[&["Esc"]],
        what: "Close a dialog or this list",
    },
];

/// A text field has the keyboard (its edit state exists for the focused widget).
pub fn typing(ctx: &Context) -> bool {
    ctx.memory(|m| m.focused())
        .is_some_and(|id| egui::text_edit::TextEditState::load(ctx, id).is_some())
}

/// The shortcuts pressed this frame. Keys that only mean something here (Space, Delete, ?)
/// are taken out of the input so a focused button doesn't also react to them. Call before
/// the pages are drawn; `blocked` (a dialog or a hotkey capture is open) turns everything off.
pub fn read(ctx: &Context, page: Page, blocked: bool) -> Vec<Shortcut> {
    let mut out = Vec::new();
    if blocked {
        return out;
    }
    let typing = typing(ctx);
    ctx.input_mut(|i| {
        let digits = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5];
        for (key, page) in digits.into_iter().zip(Page::ALL) {
            if i.consume_key(Modifiers::COMMAND, key) {
                out.push(Shortcut::Page(page));
            }
        }
        // Pages with a search box focus it themselves.
        if !matches!(page, Page::Soundboard | Page::Hotkeys)
            && i.consume_key(Modifiers::COMMAND, Key::F)
        {
            out.push(Shortcut::Search);
        }
        if typing {
            return;
        }
        if i.consume_key(Modifiers::NONE, Key::Space) {
            out.push(Shortcut::ToggleVoice);
        }
        if page == Page::Soundboard && i.consume_key(Modifiers::NONE, Key::Delete) {
            out.push(Shortcut::DeleteClip);
        }
        let question = i.consume_key(Modifiers::SHIFT, Key::Questionmark)
            || i.consume_key(Modifiers::NONE, Key::Questionmark)
            || i.consume_key(Modifiers::NONE, Key::F1);
        // (Some layouts only send the typed character.)
        let typed = i
            .events
            .iter()
            .any(|e| matches!(e, egui::Event::Text(t) if t == "?"));
        if question || typed {
            out.push(Shortcut::Help);
        }
    });
    out
}

/// The `?` overlay. Esc, a click outside or the button closes it.
pub fn overlay(ctx: &Context, open: &mut bool) {
    if !*open {
        return;
    }
    let mut close = false;
    let response = egui::Modal::new(egui::Id::new("shortcuts_overlay")).show(ctx, |ui| {
        let p = palette(ui);
        ui.set_width(460.0);
        ui.label(
            RichText::new("Keyboard shortcuts")
                .size(font::SECTION)
                .strong()
                .color(p.text),
        );
        ui.add_space(space::XS);
        ui.label(
            RichText::new("Inside this window. Global hotkeys are on the Hotkeys page.")
                .size(font::SMALL)
                .color(p.text_muted),
        );
        ui.add_space(space::M);
        egui::Grid::new("shortcut_list")
            .num_columns(2)
            .spacing([space::L, space::S])
            .show(ui, |ui| {
                for entry in LIST {
                    ui.horizontal(|ui| {
                        for (n, combo) in entry.keys.iter().enumerate() {
                            if n > 0 {
                                ui.label(RichText::new("or").color(p.text_faint));
                            }
                            let parts: Vec<String> = combo.iter().map(|s| s.to_string()).collect();
                            widgets::keycaps(ui, &parts);
                        }
                    });
                    ui.label(RichText::new(entry.what).color(p.text));
                    ui.end_row();
                }
            });
        ui.add_space(space::M);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::primary_button(ui, "Close").clicked() {
                close = true;
            }
        });
    });
    if close || response.should_close() {
        *open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(key: Key, modifiers: Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    fn run(ctx: &Context, events: Vec<egui::Event>, page: Page, blocked: bool) -> Vec<Shortcut> {
        let mut got = Vec::new();
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        ctx.run_ui(input, |ui| got = read(ui.ctx(), page, blocked))
            .textures_delta
            .clear();
        got
    }

    #[test]
    fn page_keys_space_delete_and_help() {
        let ctx = Context::default();
        let got = run(
            &ctx,
            vec![press(Key::Num3, Modifiers::COMMAND)],
            Page::Voice,
            false,
        );
        assert_eq!(got, vec![Shortcut::Page(Page::Audio)]);
        let got = run(
            &ctx,
            vec![press(Key::Space, Modifiers::NONE)],
            Page::Voice,
            false,
        );
        assert_eq!(got, vec![Shortcut::ToggleVoice]);
        // Delete means something on the soundboard only.
        let del = || vec![press(Key::Delete, Modifiers::NONE)];
        assert!(run(&ctx, del(), Page::Voice, false).is_empty());
        assert_eq!(
            run(&ctx, del(), Page::Soundboard, false),
            vec![Shortcut::DeleteClip]
        );
        let got = run(
            &ctx,
            vec![press(Key::F1, Modifiers::NONE)],
            Page::Hotkeys,
            false,
        );
        assert_eq!(got, vec![Shortcut::Help]);
        let got = run(
            &ctx,
            vec![egui::Event::Text("?".into())],
            Page::Settings,
            false,
        );
        assert_eq!(got, vec![Shortcut::Help]);
        // Ctrl+F jumps to the soundboard search from a page without a search box only.
        let f = || vec![press(Key::F, Modifiers::COMMAND)];
        assert_eq!(run(&ctx, f(), Page::Voice, false), vec![Shortcut::Search]);
        assert!(run(&ctx, f(), Page::Hotkeys, false).is_empty());
        // A dialog or a hotkey capture turns them all off.
        let got = run(
            &ctx,
            vec![press(Key::Space, Modifiers::NONE)],
            Page::Voice,
            true,
        );
        assert!(got.is_empty());
    }

    #[test]
    fn space_in_a_text_field_is_just_a_space() {
        let ctx = Context::default();
        let mut text = String::new();
        let id = egui::Id::new("field");
        let mut frame = |events: Vec<egui::Event>| {
            let mut got = Vec::new();
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            ctx.run_ui(input, |ui| {
                got = read(ui.ctx(), Page::Soundboard, false);
                let r = ui.add(egui::TextEdit::singleline(&mut text).id(id));
                r.request_focus();
            })
            .textures_delta
            .clear();
            got
        };
        frame(vec![]);
        frame(vec![]);
        let got = frame(vec![
            press(Key::Space, Modifiers::NONE),
            press(Key::Delete, Modifiers::NONE),
            egui::Event::Text("?".into()),
        ]);
        assert!(got.is_empty(), "{got:?}");
        // Ctrl+1 still works while typing.
        let got = frame(vec![press(Key::Num1, Modifiers::COMMAND)]);
        assert_eq!(got, vec![Shortcut::Page(Page::Voice)]);
    }
}
