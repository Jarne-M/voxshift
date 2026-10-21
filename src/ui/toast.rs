//! Short notifications in the bottom-right corner.

use egui::{Align2, Color32, Context, Id};

const SHOW_S: f64 = 4.0;

#[derive(Default)]
pub struct Toasts {
    items: Vec<(String, f64)>,
}

impl Toasts {
    pub fn push(&mut self, ctx: &Context, text: impl Into<String>) {
        let now = ctx.input(|i| i.time);
        self.items.push((text.into(), now + SHOW_S));
        if self.items.len() > 4 {
            self.items.remove(0);
        }
    }

    pub fn show(&mut self, ctx: &Context) {
        let now = ctx.input(|i| i.time);
        self.items.retain(|(_, until)| *until > now);
        if self.items.is_empty() {
            return;
        }
        egui::Area::new(Id::new("toasts"))
            .anchor(Align2::RIGHT_BOTTOM, egui::vec2(-14.0, -14.0))
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                for (text, _) in &self.items {
                    egui::Frame::new()
                        .fill(Color32::from_rgb(45, 38, 70))
                        .stroke(egui::Stroke::new(1.0, super::theme::ACCENT))
                        .corner_radius(6)
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(text).color(Color32::WHITE))
                        });
                    ui.add_space(4.0);
                }
            });
    }
}
