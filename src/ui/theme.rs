//! Look and feel: dark theme with a violet accent.

use egui::{Color32, Context, CornerRadius, FontId, Stroke, TextStyle};

pub const ACCENT: Color32 = Color32::from_rgb(150, 110, 255);
pub const ON: Color32 = Color32::from_rgb(40, 170, 100);
pub const OFF: Color32 = Color32::from_rgb(90, 90, 96);
pub const WARN: Color32 = Color32::from_rgb(235, 170, 50);
pub const BAD: Color32 = Color32::from_rgb(230, 70, 70);
pub const CARD: Color32 = Color32::from_gray(32);

pub fn apply(ctx: &Context) {
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        for (ts, size) in [
            (TextStyle::Small, 11.5),
            (TextStyle::Body, 14.0),
            (TextStyle::Button, 14.0),
            (TextStyle::Monospace, 13.0),
            (TextStyle::Heading, 18.0),
        ] {
            style.text_styles.insert(ts, FontId::proportional(size));
        }
        let sp = &mut style.spacing;
        sp.item_spacing = egui::vec2(8.0, 6.0);
        sp.button_padding = egui::vec2(9.0, 4.0);
        sp.interact_size.y = 24.0;
        sp.slider_width = 150.0;

        let v = &mut style.visuals;
        v.panel_fill = Color32::from_gray(22);
        v.window_fill = Color32::from_gray(30);
        v.extreme_bg_color = Color32::from_gray(14);
        v.selection.bg_fill = Color32::from_rgb(80, 55, 150);
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.hyperlink_color = ACCENT;
        let radius = CornerRadius::same(5);
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = radius;
        }
        v.widgets.inactive.bg_fill = Color32::from_gray(44);
        v.widgets.inactive.weak_bg_fill = Color32::from_gray(44);
        v.widgets.hovered.bg_fill = Color32::from_gray(62);
        v.widgets.hovered.weak_bg_fill = Color32::from_gray(62);
        v.widgets.active.bg_fill = Color32::from_rgb(100, 70, 180);
        v.widgets.active.weak_bg_fill = Color32::from_rgb(100, 70, 180);
    });
}

/// A rounded panel used to group controls.
pub fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(CARD)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
}
