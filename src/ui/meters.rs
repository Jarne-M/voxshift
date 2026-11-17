//! Level meters: segmented, green / yellow / red by level, with a peak that holds for a
//! moment before falling. Hovering one shows its dB scale.

use super::theme::{Palette, font, mix, palette};
use crate::audio::shared::to_db;
use egui::{Align2, Color32, FontId, Id, Rect, Response, RichText, Sense, Stroke, Ui, Vec2, vec2};

/// Scale bottom for the input/output meters.
pub const PROGRAM_FLOOR_DB: f32 = -60.0;
/// Mic meters reach lower, so a room's noise floor and the silence threshold both show.
pub const MIC_FLOOR_DB: f32 = -100.0;

/// How long a peak stays before it falls, and how fast it falls.
const PEAK_HOLD_S: f64 = 1.2;
const PEAK_FALL_DB_PER_S: f32 = 36.0;
/// Width of one segment and of the gap after it.
const SEGMENT: f32 = 3.0;
const GAP: f32 = 1.0;

/// 0..1 position of a dB value on a meter whose scale starts at `floor_db`.
pub fn meter_pos(db: f32, floor_db: f32) -> f32 {
    ((db - floor_db) / -floor_db).clamp(0.0, 1.0)
}

/// The highest recent level of one meter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Peak {
    pub db: f32,
    /// When it was reached.
    pub at: f64,
}

impl Peak {
    /// The peak after seeing level `db` at time `now`: a higher level replaces it at once, a
    /// lower one lets it hold for [`PEAK_HOLD_S`] and then fall.
    pub fn update(self, db: f32, now: f64) -> Peak {
        // (Time going backwards means a new session: start over.)
        if now < self.at || db >= self.display_db(now) {
            Peak { db, at: now }
        } else {
            self
        }
    }

    /// Where the peak marker is drawn at time `now`.
    pub fn display_db(self, now: f64) -> f32 {
        let age = now - self.at;
        if age <= PEAK_HOLD_S {
            self.db
        } else {
            self.db - (age - PEAK_HOLD_S) as f32 * PEAK_FALL_DB_PER_S
        }
    }
}

fn level_color(p: Palette, db: f32) -> Color32 {
    if db > -3.0 {
        p.danger
    } else if db > -12.0 {
        p.warning
    } else {
        p.success
    }
}

fn hold_peak(ui: &Ui, id: Id, db: f32, now: f64) -> f32 {
    let key = id.with("peak");
    let old = ui
        .data(|d| d.get_temp::<Peak>(key))
        .unwrap_or(Peak { db, at: now });
    let peak = old.update(db, now);
    ui.data_mut(|d| d.insert_temp(key, peak));
    peak.display_db(now)
}

fn paint(ui: &Ui, rect: Rect, db: f32, peak_db: f32, floor_db: f32, marker_db: Option<f32>) {
    let p = palette(ui);
    let painter = ui.painter_at(rect.expand(1.0));
    let n = (((rect.width() + GAP) / (SEGMENT + GAP)).floor() as usize).max(1);
    let pitch = rect.width() / n as f32;
    let lit_to = meter_pos(db, floor_db) * n as f32;
    let peak_at = (meter_pos(peak_db, floor_db) * n as f32).ceil() as usize;
    let dark = mix(p.sunken, p.border, 0.55);
    for i in 0..n {
        let x = rect.left() + i as f32 * pitch;
        let seg = Rect::from_min_max(
            egui::pos2(x, rect.top()),
            egui::pos2(x + (pitch - GAP).max(1.0), rect.bottom()),
        );
        let seg_db = floor_db + (i as f32 + 0.5) / n as f32 * -floor_db;
        let lit = (i as f32 + 0.5) < lit_to;
        let is_peak = peak_at > 0 && i + 1 == peak_at && peak_db > floor_db;
        let color = if lit || is_peak {
            level_color(p, seg_db)
        } else {
            dark
        };
        painter.rect_filled(seg, 1.0, color);
    }
    if let Some(m) = marker_db {
        let x = rect.left() + rect.width() * meter_pos(m, floor_db);
        painter.vline(
            x,
            (rect.top() - 1.0)..=(rect.bottom() + 1.0),
            Stroke::new(2.0, p.text),
        );
    }
}

/// The dB scale shown when hovering a meter.
fn scale_tip(ui: &mut Ui, db: f32, peak_db: f32, floor_db: f32, marker_db: Option<f32>) {
    let p = palette(ui);
    let show = |v: f32| {
        if v <= floor_db {
            "silent".to_string()
        } else {
            format!("{v:.0} dB")
        }
    };
    ui.label(format!("Now {}  \u{b7}  peak {}", show(db), show(peak_db)));
    if let Some(m) = marker_db {
        ui.label(
            RichText::new(format!("White line: the threshold, {m:.0} dB"))
                .small()
                .color(p.text_muted),
        );
    }
    let (rect, _) = ui.allocate_exact_size(vec2(260.0, 30.0), Sense::hover());
    let bar = Rect::from_min_max(rect.min, egui::pos2(rect.right(), rect.top() + 10.0));
    paint(ui, bar, db, peak_db, floor_db, marker_db);
    let step = if floor_db < -70.0 { 20.0 } else { 12.0 };
    let mut tick = 0.0f32;
    while tick >= floor_db {
        let x = bar.left() + bar.width() * meter_pos(tick, floor_db);
        ui.painter().vline(
            x.clamp(bar.left() + 0.5, bar.right() - 0.5),
            (bar.bottom() + 2.0)..=(bar.bottom() + 6.0),
            Stroke::new(1.0, p.text_faint),
        );
        let align = if tick == 0.0 {
            Align2::RIGHT_TOP
        } else if tick <= floor_db {
            Align2::LEFT_TOP
        } else {
            Align2::CENTER_TOP
        };
        ui.painter().text(
            egui::pos2(x, bar.bottom() + 7.0),
            align,
            format!("{tick:.0}"),
            FontId::proportional(font::SMALL - 1.5),
            p.text_muted,
        );
        tick -= step;
    }
}

/// A horizontal meter for a linear level, with an optional marker (a threshold, in dB).
pub fn level_meter(
    ui: &mut Ui,
    level: f32,
    size: Vec2,
    floor_db: f32,
    marker_db: Option<f32>,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    meter_in(ui, rect, resp, level, floor_db, marker_db)
}

/// A meter at a place the caller laid out itself (`id_salt` tells its meters apart).
pub fn level_meter_at(
    ui: &Ui,
    rect: Rect,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    level: f32,
    floor_db: f32,
    marker_db: Option<f32>,
) -> Response {
    let resp = ui.interact(rect, ui.id().with(id_salt), Sense::hover());
    meter_in(ui, rect, resp, level, floor_db, marker_db)
}

fn meter_in(
    ui: &Ui,
    rect: Rect,
    resp: Response,
    level: f32,
    floor_db: f32,
    marker_db: Option<f32>,
) -> Response {
    let db = to_db(level);
    let now = ui.input(|i| i.time);
    let peak_db = hold_peak(ui, resp.id, db, now);
    if ui.is_rect_visible(rect) {
        paint(ui, rect, db, peak_db, floor_db, marker_db);
    }
    resp.on_hover_ui(|ui| scale_tip(ui, db, peak_db, floor_db, marker_db))
}

/// A small "CLIP" lamp: red while lit, an outline otherwise.
pub fn clip_lamp(ui: &mut Ui, lit: bool) -> Response {
    let p = palette(ui);
    let font = FontId::proportional(font::SMALL - 1.5);
    let color = if lit { p.on_status } else { p.text_faint };
    let galley = ui.painter().layout_no_wrap("CLIP".to_string(), font, color);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + vec2(10.0, 4.0), Sense::hover());
    if ui.is_rect_visible(rect) {
        let (fill, stroke) = if lit {
            (p.danger, p.danger)
        } else {
            (Color32::TRANSPARENT, p.border_strong)
        };
        ui.painter().rect(
            rect,
            4.0,
            fill,
            Stroke::new(1.0, stroke),
            egui::StrokeKind::Inside,
        );
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, color);
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meter_scale() {
        assert_eq!(meter_pos(0.0, PROGRAM_FLOOR_DB), 1.0);
        assert_eq!(meter_pos(-60.0, PROGRAM_FLOOR_DB), 0.0);
        assert_eq!(meter_pos(-120.0, PROGRAM_FLOOR_DB), 0.0);
        assert!((meter_pos(-30.0, PROGRAM_FLOOR_DB) - 0.5).abs() < 1e-6);
        // The default silence threshold is visible on the mic meters.
        assert!(meter_pos(-85.0, MIC_FLOOR_DB) > 0.1);
    }

    #[test]
    fn the_peak_holds_then_falls_and_a_louder_level_replaces_it_at_once() {
        let peak = Peak { db: -6.0, at: 10.0 };
        // Quieter right after: the peak stays where it was.
        let held = peak.update(-30.0, 10.5);
        assert_eq!(held, peak);
        assert_eq!(held.display_db(10.5), -6.0);
        assert_eq!(held.display_db(10.0 + PEAK_HOLD_S), -6.0);
        // After the hold it falls, steadily.
        let later = held.update(-30.0, 10.0 + PEAK_HOLD_S + 0.25);
        let shown = later.display_db(10.0 + PEAK_HOLD_S + 0.25);
        assert!(
            (shown - (-6.0 - 0.25 * PEAK_FALL_DB_PER_S)).abs() < 1e-3,
            "{shown}"
        );
        // It never falls below the current level.
        let landed = later.update(-30.0, 20.0);
        assert_eq!(landed.display_db(20.0), -30.0);
        // Louder than the peak: it jumps up without waiting.
        let louder = held.update(-3.0, 10.6);
        assert_eq!(louder, Peak { db: -3.0, at: 10.6 });
        // Time going backwards (a new egui context in tests) starts over.
        assert_eq!(peak.update(-40.0, 1.0), Peak { db: -40.0, at: 1.0 });
    }

    #[test]
    fn colours_follow_the_level() {
        let p = crate::ui::theme::DARK;
        assert_eq!(level_color(p, -30.0), p.success);
        assert_eq!(level_color(p, -8.0), p.warning);
        assert_eq!(level_color(p, -1.0), p.danger);
    }
}
