//! Level meters.

use crate::audio::shared::to_db;
use egui::{Color32, Rect, Sense, Ui, vec2};

const FLOOR_DB: f32 = -60.0;

/// 0..1 position of a dB value on the meter scale.
pub fn meter_pos(db: f32) -> f32 {
    ((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0)
}

/// Horizontal meter for a linear level, with an optional marker (e.g. a threshold, in dB).
pub fn level_bar(ui: &mut Ui, level: f32, width: f32, marker_db: Option<f32>) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 12.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, Color32::from_gray(12));
    let db = to_db(level);
    let pos = meter_pos(db);
    let fill = Rect::from_min_size(rect.min, vec2(rect.width() * pos, rect.height()));
    let color = if db > -3.0 {
        Color32::from_rgb(230, 70, 70)
    } else if db > -12.0 {
        Color32::from_rgb(230, 190, 60)
    } else {
        Color32::from_rgb(60, 190, 110)
    };
    painter.rect_filled(fill, 3.0, color);
    if let Some(m) = marker_db {
        let x = rect.left() + rect.width() * meter_pos(m);
        painter.vline(x, rect.y_range(), egui::Stroke::new(2.0, Color32::WHITE));
    }
    resp.on_hover_text(format!("{db:.0} dBFS"))
}

/// A small "CLIP" lamp.
pub fn clip_lamp(ui: &mut Ui, lit: bool) -> egui::Response {
    let color = if lit {
        Color32::from_rgb(230, 50, 50)
    } else {
        Color32::from_gray(50)
    };
    let text = egui::RichText::new("CLIP").small().strong().color(if lit {
        Color32::WHITE
    } else {
        Color32::from_gray(120)
    });
    egui::Frame::new()
        .fill(color)
        .corner_radius(3)
        .inner_margin(egui::Margin::symmetric(5, 1))
        .show(ui, |ui| ui.label(text))
        .response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meter_scale() {
        assert_eq!(meter_pos(0.0), 1.0);
        assert_eq!(meter_pos(-60.0), 0.0);
        assert_eq!(meter_pos(-120.0), 0.0);
        assert!((meter_pos(-30.0) - 0.5).abs() < 1e-6);
    }
}
