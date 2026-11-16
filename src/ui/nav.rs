//! The navigation rail on the left: one entry per page, icon plus label, collapsible to
//! icons only. It replaces the old sidebar that stacked every setting in one long column.

use super::icons;
use super::theme::{font, palette, space};
use super::widgets;
use crate::config::Page;
use egui::{Align, FontId, Layout, Rect, Response, RichText, Sense, Ui, pos2, vec2};

pub const WIDTH: f32 = 176.0;
pub const WIDTH_COLLAPSED: f32 = 60.0;
/// Below this window width the rail shows icons only, whatever the setting.
pub const NARROW_WINDOW: f32 = 900.0;

const ITEM_HEIGHT: f32 = 40.0;
const MARGIN: f32 = 10.0;

pub fn icon(page: Page) -> &'static str {
    match page {
        Page::Voice => icons::VOICE,
        Page::Soundboard => icons::SOUNDBOARD,
        Page::Audio => icons::AUDIO,
        Page::Hotkeys => icons::KEYBOARD,
        Page::Settings => icons::SETTINGS,
    }
}

/// What the page holds, for its tooltip.
fn about(page: Page) -> &'static str {
    match page {
        Page::Voice => "Presets, the effect chain and Melody Voice",
        Page::Soundboard => "Your sound clips",
        Page::Audio => "Virtual mic, microphones, monitor and latency",
        Page::Hotkeys => "Every hotkey in one place",
        Page::Settings => "Window, startup, feedback and theme",
    }
}

/// The rail's width for the saved setting and the window width, animated between the two.
pub fn width(ui: &Ui, collapsed: bool, window_width: f32) -> f32 {
    let narrow = collapsed || window_width < NARROW_WINDOW;
    let t = ui
        .ctx()
        .animate_bool_with_time(egui::Id::new("nav_collapsed"), narrow, 0.12);
    egui::lerp(WIDTH..=WIDTH_COLLAPSED, t)
}

/// One entry. `open` (0..1) is how far the rail is expanded: the label fades in with it.
fn item(ui: &mut Ui, icon: &str, label: &str, selected: bool, open: f32) -> Response {
    let p = palette(ui);
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), ITEM_HEIGHT), Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let fill = if selected {
            p.accent_soft
        } else if r.hovered() {
            p.surface_raised
        } else {
            egui::Color32::TRANSPARENT
        };
        painter.rect_filled(rect, 8.0, fill);
        if selected {
            let bar = Rect::from_min_max(
                pos2(rect.left(), rect.top() + 9.0),
                pos2(rect.left() + 3.0, rect.bottom() - 9.0),
            );
            painter.rect_filled(bar, 2.0, p.accent);
        }
        if r.has_focus() {
            painter.rect_stroke(
                rect,
                8.0,
                egui::Stroke::new(1.5, p.accent),
                egui::StrokeKind::Inside,
            );
        }
        let (icon_color, text_color) = if selected {
            (p.accent, p.text)
        } else if r.hovered() {
            (p.text, p.text)
        } else {
            (p.text_muted, p.text_muted)
        };
        // The icon keeps its place while the rail opens and closes.
        let icon_x = rect.left() + (WIDTH_COLLAPSED - 2.0 * MARGIN) / 2.0;
        painter.text(
            pos2(icon_x, rect.center().y),
            egui::Align2::CENTER_CENTER,
            icon,
            FontId::proportional(20.0),
            icon_color,
        );
        if open > 0.3 {
            let fade = ((open - 0.3) / 0.7).clamp(0.0, 1.0);
            painter.text(
                pos2(
                    rect.left() + WIDTH_COLLAPSED - 2.0 * MARGIN + 4.0,
                    rect.center().y,
                ),
                egui::Align2::LEFT_CENTER,
                label,
                FontId::proportional(font::BODY + 0.5),
                text_color.gamma_multiply(fade),
            );
        }
    }
    r.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Draws the rail. Clicking an entry changes `page`; the button at the bottom flips
/// `collapsed` (it is hidden while the window itself is too narrow for labels).
pub fn show(ui: &mut Ui, page: &mut Page, collapsed: &mut bool, window_width: f32) {
    let p = palette(ui);
    let forced = window_width < NARROW_WINDOW;
    let open = ((ui.available_width() + 2.0 * MARGIN - WIDTH_COLLAPSED)
        / (WIDTH - WIDTH_COLLAPSED))
        .clamp(0.0, 1.0);
    ui.spacing_mut().item_spacing.y = space::XS;
    ui.add_space(space::S);
    for entry in Page::ALL {
        let r = item(ui, icon(entry), entry.label(), *page == entry, open);
        #[cfg(test)]
        widgets::remember(ui.ctx(), "nav", format!("{entry:?}"), r.rect);
        let r = if open < 0.5 {
            r.on_hover_text(format!("{}\n{}", entry.label(), about(entry)))
        } else {
            r.on_hover_text(about(entry))
        };
        if r.clicked() {
            *page = entry;
        }
    }
    ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
        ui.add_space(space::S);
        if !forced {
            let tip = if *collapsed {
                "Show the page names"
            } else {
                "Show icons only"
            };
            ui.horizontal(|ui| {
                ui.add_space((WIDTH_COLLAPSED - 2.0 * MARGIN - 28.0) / 2.0);
                if widgets::icon_button(ui, icons::NAV_COLLAPSE, tip).clicked() {
                    *collapsed = !*collapsed;
                }
                if open > 0.8 {
                    ui.label(
                        RichText::new(concat!("v", env!("CARGO_PKG_VERSION")))
                            .small()
                            .color(p.text_faint),
                    );
                }
            });
        }
    });
}

/// The inner margin of the rail's panel.
pub fn margin() -> egui::Margin {
    egui::Margin::symmetric(MARGIN as i8, 0)
}
