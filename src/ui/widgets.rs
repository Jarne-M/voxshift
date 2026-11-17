//! The building blocks every page uses: toggle switch, slider, card, keycaps, chips and
//! buttons. They take their colours from [`theme::palette`], so they follow the theme.

use super::theme::{Palette, RADIUS_M, RADIUS_S, font, mix, palette, space};
use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{
    Align, Align2, Color32, CornerRadius, CursorIcon, FontId, Frame, Galley, InnerResponse, Key,
    Margin, PointerButton, Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, Vec2,
    WidgetInfo, WidgetType, pos2, vec2,
};
use std::ops::RangeInclusive;
use std::sync::Arc;

// ------------------------------------------------------------------------------------------
// Text helpers
// ------------------------------------------------------------------------------------------

/// `text` laid out on at most `rows` lines of `max_width`, ending in "…" if it does not fit.
pub fn elided(
    ui: &Ui,
    text: &str,
    font: FontId,
    color: Color32,
    max_width: f32,
    rows: usize,
) -> Arc<Galley> {
    let mut job = LayoutJob::single_section(text.to_string(), TextFormat::simple(font, color));
    job.wrap = TextWrapping {
        max_width,
        max_rows: rows.max(1),
        break_anywhere: false,
        overflow_character: Some('\u{2026}'),
    };
    ui.painter().layout_job(job)
}

// ------------------------------------------------------------------------------------------
// Toggle switch
// ------------------------------------------------------------------------------------------

pub const TOGGLE_SIZE: Vec2 = vec2(36.0, 20.0);

/// An on/off switch. `changed()` is set on the frame it was flipped.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let (rect, mut r) = ui.allocate_exact_size(TOGGLE_SIZE, Sense::click());
    if r.clicked() {
        *on = !*on;
        r.mark_changed();
    }
    let (enabled, state) = (ui.is_enabled(), *on);
    r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, enabled, state, ""));
    if ui.is_rect_visible(rect) {
        let p = palette(ui);
        let t = ui.ctx().animate_bool_responsive(r.id, *on);
        let off = if r.hovered() {
            p.border_strong
        } else {
            mix(p.border, p.border_strong, 0.5)
        };
        let lit = if r.hovered() {
            p.accent_hover
        } else {
            p.accent
        };
        let radius = rect.height() / 2.0;
        let painter = ui.painter();
        painter.rect_filled(rect, radius, mix(off, lit, t));
        if r.has_focus() {
            painter.rect_stroke(
                rect.expand(2.0),
                radius + 2.0,
                Stroke::new(1.5, p.accent),
                StrokeKind::Outside,
            );
        }
        let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
        painter.circle_filled(pos2(x, rect.center().y), radius - 3.0, Color32::WHITE);
    }
    r.on_hover_cursor(CursorIcon::PointingHand)
}

// ------------------------------------------------------------------------------------------
// Slider
// ------------------------------------------------------------------------------------------

/// Radius of the slider's knob.
const KNOB: f32 = 7.0;
/// Scroll distance for one wheel tick (half a wheel notch).
const WHEEL_TICK: f32 = 20.0;
/// Ctrl+drag moves the value this much slower than the pointer.
const FINE: f32 = 0.1;

/// The value being typed into a slider's value box.
#[derive(Clone)]
struct Typing {
    text: String,
    /// The text field still has to take the focus (first frame).
    focus: bool,
}

/// One slider row, the same everywhere: label on the left, the track, and the value with its
/// unit on the right.
///
/// - Drag or click the track to set the value; **Ctrl+drag** moves it ten times finer.
/// - **Double-click or right-click** resets it to the default, which is marked on the track.
/// - The **mouse wheel** adjusts it, but only after the pointer was moved onto the slider:
///   scrolling a page past a slider must not change it.
/// - Click the value to type one (Enter applies, Esc cancels).
/// - Arrow keys adjust it when it has the keyboard focus.
pub struct Slider<'a> {
    value: &'a mut f32,
    min: f32,
    max: f32,
    label: &'a str,
    unit: &'a str,
    default: Option<f32>,
    decimals: usize,
    log: bool,
    step: f32,
    percent: bool,
    tip: &'a str,
    label_width: f32,
    /// Just a track of this width: no label, no value box (for toolbars).
    compact: Option<f32>,
}

/// A slider row never grows wider than this: on a wide card a longer track is harder to
/// read against its value, not easier.
const SLIDER_MAX_W: f32 = 560.0;

impl<'a> Slider<'a> {
    pub fn new(value: &'a mut f32, range: RangeInclusive<f32>) -> Self {
        Self {
            value,
            min: *range.start(),
            max: *range.end(),
            label: "",
            unit: "",
            default: None,
            decimals: 0,
            log: false,
            step: 0.0,
            percent: false,
            tip: "",
            label_width: 120.0,
            compact: None,
        }
    }

    /// Shown left of the track; also tells sliders in the same card apart.
    pub fn label(mut self, label: &'a str) -> Self {
        self.label = label;
        self
    }

    pub fn unit(mut self, unit: &'a str) -> Self {
        self.unit = unit;
        self
    }

    /// The value double-click / right-click goes back to (marked on the track).
    pub fn default_value(mut self, default: f32) -> Self {
        self.default = Some(default);
        self
    }

    pub fn decimals(mut self, decimals: usize) -> Self {
        self.decimals = decimals;
        self
    }

    /// Logarithmic track (frequencies, times). The range must be above zero.
    pub fn logarithmic(mut self, log: bool) -> Self {
        self.log = log && self.min > 0.0;
        self
    }

    /// Values snap to multiples of `step` (0 = continuous).
    pub fn step(mut self, step: f32) -> Self {
        self.step = step.max(0.0);
        self
    }

    /// Show a 0..1 value as 0..100 %.
    pub fn percent(mut self, percent: bool) -> Self {
        self.percent = percent;
        self
    }

    /// What the control does, shown when hovering it.
    pub fn tip(mut self, tip: &'a str) -> Self {
        self.tip = tip;
        self
    }

    /// Room for the label (longer labels end in "…").
    pub fn label_width(mut self, width: f32) -> Self {
        self.label_width = width;
        self
    }

    /// Only the track, `width` points wide: for a toolbar, where the label is an icon next
    /// to it and the value shows in what it changes. (The label still names it for ids.)
    pub fn compact(mut self, width: f32) -> Self {
        self.compact = Some(width);
        self
    }

    fn position_of(&self, v: f32) -> f32 {
        to_norm(v, self.min, self.max, self.log)
    }

    /// The (snapped) value at position `t` (0..1) of the track.
    fn value_at(&self, t: f32) -> f32 {
        snap(
            from_norm(t.clamp(0.0, 1.0), self.min, self.max, self.log),
            self.step,
        )
        .clamp(self.min, self.max)
    }

    /// One wheel tick or arrow-key press, in `direction` (+1 / -1).
    fn nudge(&self, v: f32, direction: f32) -> f32 {
        if self.step > 0.0 {
            let ticks = ((self.max - self.min) * 0.01 / self.step).round().max(1.0);
            snap(v + direction * ticks * self.step, self.step).clamp(self.min, self.max)
        } else {
            self.value_at(self.position_of(v) + direction * 0.01)
        }
    }

    fn text(&self, v: f32) -> String {
        format_value(v, self.decimals, self.unit, self.percent, self.min < 0.0)
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let p = palette(ui);
        let compact = self.compact.is_some();
        let width = self
            .compact
            .unwrap_or_else(|| ui.available_width().clamp(140.0, SLIDER_MAX_W));
        let (row, mut response) = ui.allocate_exact_size(vec2(width, 26.0), Sense::hover());
        let id = ui.id().with(("vox_slider", self.label));
        let enabled = ui.is_enabled();

        let value_w = if compact { 0.0 } else { 64.0 };
        let label_w = if self.label.is_empty() || compact {
            0.0
        } else {
            self.label_width.min(width * 0.42)
        };
        let gap = if label_w > 0.0 { space::S + 2.0 } else { 0.0 };
        let value_gap = if compact { 0.0 } else { space::S };
        let label_rect = Rect::from_min_max(row.min, pos2(row.left() + label_w, row.bottom()));
        let value_rect = Rect::from_min_max(pos2(row.right() - value_w, row.top()), row.max);
        let track_rect = Rect::from_min_max(
            pos2(label_rect.right() + gap, row.top()),
            pos2(value_rect.left() - value_gap, row.bottom()),
        );
        let span = (track_rect.width() - 2.0 * KNOB).max(1.0);
        let x_of = |t: f32| track_rect.left() + KNOB + t.clamp(0.0, 1.0) * span;

        let before = *self.value;
        let mut v = before;

        // --- the track -----------------------------------------------------------------
        let track = ui.interact(track_rect, id.with("track"), Sense::click_and_drag());
        let primary_down = ui.input(|i| i.pointer.primary_down());
        let fine = ui.input(|i| i.modifiers.command);
        if (track.double_clicked() || track.secondary_clicked())
            && let Some(d) = self.default
        {
            v = d;
        } else if primary_down && track.is_pointer_button_down_on() {
            if fine {
                if track.dragged_by(PointerButton::Primary) {
                    let t = self.position_of(v) + track.drag_delta().x / span * FINE;
                    // Fine moves are smaller than a step would allow: only snap when asked to.
                    v = from_norm(t.clamp(0.0, 1.0), self.min, self.max, self.log);
                    if self.step > 0.0 {
                        v = snap(v, self.step * FINE);
                    }
                }
            } else if let Some(pos) = track.interact_pointer_pos() {
                v = self.value_at((pos.x - track_rect.left() - KNOB) / span);
            }
        }
        if track.has_focus() {
            let (left, right) = ui.input(|i| {
                (
                    i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::ArrowDown),
                    i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::ArrowUp),
                )
            });
            if left {
                v = self.nudge(v, -1.0);
            }
            if right {
                v = self.nudge(v, 1.0);
            }
        }

        // --- the mouse wheel -----------------------------------------------------------
        // Armed only once the pointer itself moved over this row. When the page scrolls and
        // the slider merely passes under a resting pointer, the wheel keeps scrolling the page.
        let over = enabled && ui.rect_contains_pointer(row);
        let armed_id = id.with("armed");
        let mut armed = over && ui.data(|d| d.get_temp::<bool>(armed_id)).unwrap_or(false);
        if over && ui.input(|i| i.pointer.delta() != Vec2::ZERO) {
            armed = true;
        }
        ui.data_mut(|d| d.insert_temp(armed_id, armed));
        if armed {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                ui.input_mut(|i| i.smooth_scroll_delta = Vec2::ZERO);
                let acc_id = id.with("wheel");
                let mut acc = ui.data(|d| d.get_temp::<f32>(acc_id)).unwrap_or(0.0) + scroll;
                while acc.abs() >= WHEEL_TICK {
                    v = self.nudge(v, acc.signum());
                    acc -= acc.signum() * WHEEL_TICK;
                }
                ui.data_mut(|d| d.insert_temp(acc_id, acc));
            }
        }

        // --- the value box -------------------------------------------------------------
        let typing_id = id.with("typing");
        let typing = ui.data(|d| d.get_temp::<Typing>(typing_id));
        // (A compact slider has no value box: nothing to click there.)
        let value_sense = if compact {
            Sense::hover()
        } else {
            Sense::click()
        };
        let value_click = ui.interact(value_rect, id.with("value"), value_sense);
        if compact {
            // Nothing to type into.
        } else if let Some(mut typing) = typing {
            let edit = egui::TextEdit::singleline(&mut typing.text)
                .id(id.with("value_edit"))
                .horizontal_align(Align::RIGHT)
                .margin(Margin::symmetric(4, 2))
                .desired_width(value_w);
            let r = ui.put(value_rect.shrink2(vec2(0.0, 2.0)), edit);
            if std::mem::take(&mut typing.focus) {
                r.request_focus();
            }
            if r.lost_focus() {
                // Enter and a click elsewhere apply; Esc leaves the value alone.
                if !ui.input(|i| i.key_pressed(Key::Escape))
                    && let Some(typed) = parse_value(&typing.text, self.percent)
                {
                    v = snap(typed, self.step).clamp(self.min, self.max);
                }
                ui.data_mut(|d| d.remove::<Typing>(typing_id));
            } else {
                ui.data_mut(|d| d.insert_temp(typing_id, typing));
            }
        } else if value_click.clicked() {
            let text = plain_number(v, self.decimals, self.percent);
            ui.data_mut(|d| d.insert_temp(typing_id, Typing { text, focus: true }));
        }

        if !v.is_finite() {
            v = before;
        }
        if v != before {
            *self.value = v;
            response.mark_changed();
        }

        // --- painting ------------------------------------------------------------------
        if ui.is_rect_visible(row) {
            let painter = ui.painter();
            if label_w > 0.0 {
                let galley = elided(
                    ui,
                    self.label,
                    FontId::proportional(font::BODY),
                    p.text_muted,
                    label_w,
                    1,
                );
                let y = row.center().y - galley.size().y / 2.0;
                painter.galley(pos2(label_rect.left(), y), galley, p.text_muted);
            }

            let cy = track_rect.center().y;
            let rail = Rect::from_min_max(
                pos2(track_rect.left() + KNOB - 2.0, cy - 2.0),
                pos2(track_rect.right() - KNOB + 2.0, cy + 2.0),
            );
            painter.rect_filled(rail, 2.0, mix(p.border, p.border_strong, 0.35));
            let active = track.hovered() || track.dragged() || track.has_focus();
            let lit = if active { p.accent_hover } else { p.accent };
            // Ranges around zero (gain, pitch) fill from zero, the others from the left.
            let origin = if self.min < 0.0 && self.max > 0.0 && !self.log {
                self.position_of(0.0)
            } else {
                0.0
            };
            let x = x_of(self.position_of(v));
            let x0 = x_of(origin);
            let fill = Rect::from_min_max(pos2(x.min(x0), cy - 2.0), pos2(x.max(x0), cy + 2.0));
            painter.rect_filled(fill, 2.0, lit);
            if let Some(d) = self.default {
                let xd = x_of(self.position_of(d));
                let tick = Rect::from_center_size(pos2(xd, cy), vec2(2.0, 11.0));
                painter.rect_filled(tick, 1.0, p.text_faint);
            }
            let knob_r = if track.dragged() { KNOB + 1.0 } else { KNOB };
            if track.has_focus() {
                painter.circle_stroke(pos2(x, cy), knob_r + 3.0, Stroke::new(1.5, p.accent));
            }
            painter.circle_filled(pos2(x, cy), knob_r, Color32::WHITE);
            let ring = if active { lit } else { p.border_strong };
            painter.circle_stroke(pos2(x, cy), knob_r, Stroke::new(1.5, ring));

            let is_typing = ui.data(|d| d.get_temp::<Typing>(typing_id)).is_some();
            if !is_typing && !compact {
                if value_click.hovered() {
                    painter.rect(
                        value_rect.shrink2(vec2(0.0, 2.0)),
                        CornerRadius::same(4),
                        p.surface_raised,
                        Stroke::new(1.0, p.border_strong),
                        StrokeKind::Inside,
                    );
                }
                painter.text(
                    pos2(value_rect.right() - 5.0, row.center().y),
                    Align2::RIGHT_CENTER,
                    self.text(v),
                    FontId::proportional(font::BODY),
                    p.text,
                );
            }
        }

        let label = self.label;
        response.widget_info(|| WidgetInfo::slider(enabled, f64::from(v), label));
        let default_text = self.default.map(|d| self.text(d));
        let tip = self.tip;
        track
            .on_hover_cursor(CursorIcon::ResizeHorizontal)
            .on_hover_ui(|ui| slider_tip(ui, tip, default_text.as_deref()));
        if !compact {
            value_click
                .on_hover_cursor(CursorIcon::Text)
                .on_hover_text("Click to type a value");
        }
        response
    }
}

impl egui::Widget for Slider<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        self.show(ui)
    }
}

fn slider_tip(ui: &mut Ui, tip: &str, default: Option<&str>) {
    let p = palette(ui);
    ui.set_max_width(320.0);
    if !tip.is_empty() {
        ui.label(tip);
    }
    let reset = match default {
        Some(d) => format!("Double-click or right-click: back to {d}\n"),
        None => String::new(),
    };
    ui.label(
        RichText::new(format!(
            "{reset}Ctrl+drag: fine adjustment  \u{b7}  Mouse wheel: step"
        ))
        .small()
        .color(p.text_muted),
    );
}

/// Position (0..1) of `v` on a track from `min` to `max`.
pub fn to_norm(v: f32, min: f32, max: f32, log: bool) -> f32 {
    let t = if log && min > 0.0 {
        (v.max(min) / min).ln() / (max / min).ln()
    } else {
        (v - min) / (max - min)
    };
    if t.is_finite() {
        t.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// The value at position `t` (0..1) of a track from `min` to `max`.
pub fn from_norm(t: f32, min: f32, max: f32, log: bool) -> f32 {
    if log && min > 0.0 {
        min * (max / min).powf(t)
    } else {
        min + t * (max - min)
    }
}

/// `v` rounded to a multiple of `step` (0 = no snapping).
pub fn snap(v: f32, step: f32) -> f32 {
    if step > 0.0 {
        (v / step).round() * step
    } else {
        v
    }
}

/// A value as shown next to a slider: "+3.0 dB", "35 %", "1200 Hz".
pub fn format_value(v: f32, decimals: usize, unit: &str, percent: bool, signed: bool) -> String {
    let number = plain_number(v, decimals, percent);
    // "-0.0" and "+0" read badly: zero has no sign.
    let zero = number
        .trim_start_matches('-')
        .chars()
        .all(|c| c == '0' || c == '.');
    let number = if zero {
        number.trim_start_matches('-').to_string()
    } else if signed && v > 0.0 {
        format!("+{number}")
    } else {
        number
    };
    match (percent, unit.is_empty()) {
        (true, _) => format!("{number} %"),
        (false, true) => number,
        (false, false) => format!("{number} {unit}"),
    }
}

/// Just the number, as put into the value box for typing.
fn plain_number(v: f32, decimals: usize, percent: bool) -> String {
    if percent {
        format!("{:.0}", v * 100.0)
    } else {
        format!("{v:.decimals$}")
    }
}

/// Reads a typed value: "12,5", "-3 dB", "+4", "35%". A decimal comma is fine.
pub fn parse_value(text: &str, percent: bool) -> Option<f32> {
    let cleaned: String = text
        .trim()
        .chars()
        .map(|c| if c == ',' { '.' } else { c })
        .filter(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+'))
        .collect();
    let v: f32 = cleaned.parse().ok()?;
    v.is_finite().then_some(if percent { v / 100.0 } else { v })
}

// ------------------------------------------------------------------------------------------
// Cards and titles
// ------------------------------------------------------------------------------------------

/// The frame of a card: a surface with a subtle border.
pub fn card_frame(p: &Palette) -> Frame {
    Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(RADIUS_M))
        .inner_margin(Margin::same(14))
}

/// A dialog over a dimmed window. egui's default modal frame is a popup's, whose 6-point
/// margin crowds the text against the edge; dialogs get a window's margin instead.
pub fn modal(ctx: &egui::Context, id: &str) -> egui::Modal {
    egui::Modal::new(egui::Id::new(id)).frame(Frame::window(&ctx.global_style()))
}

/// A rounded panel that groups controls, as wide as the space it is in.
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    card_frame(palette(ui)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui)
    })
}

/// A card's title, with an optional icon in the accent colour.
pub fn section_title(ui: &mut Ui, icon: &str, title: &str) -> Response {
    let p = palette(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::S;
        if !icon.is_empty() {
            ui.label(
                RichText::new(icon)
                    .size(font::SECTION + 2.5)
                    .color(p.accent),
            );
        }
        ui.label(
            RichText::new(title)
                .size(font::SECTION)
                .strong()
                .color(p.text),
        )
    })
    .inner
}

/// The title at the top of a page, with one line saying what the page is for.
pub fn page_title(ui: &mut Ui, title: &str, about: &str) {
    let p = palette(ui);
    ui.label(
        RichText::new(title)
            .size(font::TITLE)
            .strong()
            .color(p.text),
    );
    ui.label(RichText::new(about).size(font::BODY).color(p.text_muted));
    ui.add_space(space::M);
}

/// A note across the width of its place ("Voice changer is off — others hear your clean
/// voice"), tinted by its tone.
pub fn notice(ui: &mut Ui, icon: &str, text: &str, tone: Tone) -> Response {
    let p = palette(ui);
    let color = tone.color(p);
    Frame::new()
        .fill(mix(p.surface, color, 0.10))
        .stroke(Stroke::new(1.0, mix(p.border, color, 0.35)))
        .corner_radius(CornerRadius::same(RADIUS_S))
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::S;
                if !icon.is_empty() {
                    ui.label(RichText::new(icon).size(16.0).color(color));
                }
                ui.add(egui::Label::new(RichText::new(text).color(p.text)).wrap());
            });
        })
        .response
}

/// A label in a column of fixed width, left-aligned and cut short with "…" if it is longer.
pub fn column_label(ui: &mut Ui, width: f32, height: f32, text: RichText) -> Response {
    ui.allocate_ui_with_layout(
        vec2(width, height),
        egui::Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_min_size(vec2(width, height));
            ui.add(egui::Label::new(text).truncate())
        },
    )
    .inner
}

/// A short explanation under a title or next to a control.
pub fn hint(ui: &mut Ui, text: &str) -> Response {
    let p = palette(ui);
    ui.label(RichText::new(text).small().color(p.text_muted))
}

// ------------------------------------------------------------------------------------------
// Keycaps
// ------------------------------------------------------------------------------------------

const KEYCAP_H: f32 = 20.0;
const KEYCAP_PAD: f32 = 6.0;
const KEYCAP_PLUS_GAP: f32 = 3.0;

/// A key combination laid out as keycaps, ready to paint.
struct Keycaps {
    caps: Vec<Arc<Galley>>,
    plus: Arc<Galley>,
}

impl Keycaps {
    fn new(ui: &Ui, parts: &[String]) -> Self {
        let p = palette(ui);
        let font = FontId::proportional(font::SMALL);
        let caps = parts
            .iter()
            .map(|s| ui.painter().layout_no_wrap(s.clone(), font.clone(), p.text))
            .collect();
        let plus = ui
            .painter()
            .layout_no_wrap("+".to_string(), font, p.text_faint);
        Self { caps, plus }
    }

    fn cap_width(g: &Galley) -> f32 {
        (g.size().x + 2.0 * KEYCAP_PAD).max(KEYCAP_H)
    }

    fn plus_width(&self) -> f32 {
        self.plus.size().x + 2.0 * KEYCAP_PLUS_GAP
    }

    fn size(&self) -> Vec2 {
        let caps: f32 = self.caps.iter().map(|g| Self::cap_width(g)).sum();
        let pluses = self.plus_width() * self.caps.len().saturating_sub(1) as f32;
        vec2((caps + pluses).max(1.0), KEYCAP_H)
    }

    fn paint(self, ui: &Ui, min: Pos2) {
        let p = palette(ui);
        let painter = ui.painter();
        let plus_w = self.plus_width();
        let cy = min.y + KEYCAP_H / 2.0;
        let mut x = min.x;
        for (i, g) in self.caps.into_iter().enumerate() {
            if i > 0 {
                let at = pos2(x + KEYCAP_PLUS_GAP, cy - self.plus.size().y / 2.0);
                painter.galley(at, self.plus.clone(), p.text_faint);
                x += plus_w;
            }
            let w = Self::cap_width(&g);
            let cap = Rect::from_min_size(pos2(x, min.y), vec2(w, KEYCAP_H));
            // A darker bottom edge makes it read as a key.
            painter.rect_filled(cap, 5.0, p.border_strong);
            painter.rect(
                Rect::from_min_max(cap.min, cap.max - vec2(0.0, 1.5)),
                5.0,
                mix(p.surface_raised, p.text, 0.06),
                Stroke::new(1.0, p.border_strong),
                StrokeKind::Inside,
            );
            let at = pos2(
                cap.center().x - g.size().x / 2.0,
                cy - g.size().y / 2.0 - 0.75,
            );
            painter.galley(at, g, p.text);
            x += w;
        }
    }
}

/// A key combination as keycap chips: `[Ctrl] + [Shift] + [F8]`.
pub fn keycaps(ui: &mut Ui, parts: &[String]) -> Response {
    let caps = Keycaps::new(ui, parts);
    let (rect, r) = ui.allocate_exact_size(caps.size(), Sense::hover());
    if ui.is_rect_visible(rect) {
        caps.paint(ui, rect.min);
    }
    r
}

/// What a [`hotkey_button`] shows.
pub enum KeySlot<'a> {
    /// A key is assigned: its parts, e.g. `["Ctrl", "F8"]`.
    Set(&'a [String]),
    /// Nothing assigned.
    Empty,
    /// Waiting for the user to press the key to assign.
    Capturing,
}

/// The button that shows an action's hotkey as keycaps and, when clicked, starts assigning
/// one (or cancels while it is waiting for a key).
pub fn hotkey_button(ui: &mut Ui, slot: KeySlot) -> Response {
    let p = palette(ui);
    let pad = vec2(6.0, 4.0);
    let text = |s: &str, color| {
        ui.painter().layout_no_wrap(
            s.to_string(),
            FontId::proportional(font::SMALL + 0.5),
            color,
        )
    };
    enum Content {
        Caps(Keycaps),
        Text(Arc<Galley>),
    }
    let (content, capturing) = match slot {
        KeySlot::Set(parts) => (Content::Caps(Keycaps::new(ui, parts)), false),
        KeySlot::Empty => (Content::Text(text("Set key", p.text_muted)), false),
        KeySlot::Capturing => (
            Content::Text(text("Press a key\u{2026}  Esc cancels", p.accent)),
            true,
        ),
    };
    let inner = match &content {
        Content::Caps(c) => c.size(),
        Content::Text(g) => vec2(g.size().x + 8.0, KEYCAP_H),
    };
    let (rect, r) = ui.allocate_exact_size(inner + 2.0 * pad, Sense::click());
    if ui.is_rect_visible(rect) {
        let hovered = r.hovered() && ui.is_enabled();
        let (fill, stroke) = if capturing {
            // A slow pulse says "waiting for you".
            let t = ui.input(|i| i.time);
            let pulse = (0.5 + 0.5 * (t * 4.0).sin()) as f32;
            (
                mix(p.accent_soft, p.surface, 0.5 * pulse),
                Stroke::new(1.5, p.accent),
            )
        } else if hovered {
            (p.surface_raised, Stroke::new(1.0, p.border_strong))
        } else if matches!(content, Content::Text(_)) {
            (Color32::TRANSPARENT, Stroke::new(1.0, p.border))
        } else {
            (Color32::TRANSPARENT, Stroke::NONE)
        };
        ui.painter().rect(
            rect,
            CornerRadius::same(RADIUS_S),
            fill,
            stroke,
            StrokeKind::Inside,
        );
        match content {
            Content::Caps(c) => c.paint(ui, rect.min + pad),
            Content::Text(g) => {
                let at = rect.center() - g.size() / 2.0;
                ui.painter().galley(at, g, p.text);
            }
        }
    }
    r.on_hover_cursor(CursorIcon::PointingHand)
}

// ------------------------------------------------------------------------------------------
// Chips and badges
// ------------------------------------------------------------------------------------------

/// What a chip or message is about; picks its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Accent,
    Success,
    Warning,
    Danger,
}

impl Tone {
    pub fn color(self, p: &Palette) -> Color32 {
        match self {
            Tone::Neutral => p.text_muted,
            Tone::Accent => p.accent,
            Tone::Success => p.success,
            Tone::Warning => p.warning,
            Tone::Danger => p.danger,
        }
    }
}

fn pill(ui: &mut Ui, text: String, tone: Tone, size: f32, pad: Vec2, sense: Sense) -> Response {
    let p = palette(ui);
    let fg = tone.color(p);
    let galley = ui
        .painter()
        .layout_no_wrap(text, FontId::proportional(size), fg);
    let (rect, r) = ui.allocate_exact_size(galley.size() + 2.0 * pad, sense);
    if ui.is_rect_visible(rect) {
        let clickable = sense.senses_click();
        let strength = if clickable && r.hovered() { 0.28 } else { 0.16 };
        ui.painter().rect(
            rect,
            rect.height() / 2.0,
            mix(p.surface, fg, strength),
            Stroke::new(1.0, mix(p.surface, fg, 0.45)),
            StrokeKind::Inside,
        );
        let at = rect.center() - galley.size() / 2.0;
        ui.painter().galley(at, galley, fg);
    }
    r
}

/// A clickable status chip ("VB-CABLE not found"): a pill in the colour of its tone.
pub fn chip(ui: &mut Ui, icon: &str, text: &str, tone: Tone) -> Response {
    let content = if icon.is_empty() {
        text.to_string()
    } else {
        format!("{icon}  {text}")
    };
    pill(
        ui,
        content,
        tone,
        font::SMALL + 0.5,
        vec2(10.0, 5.0),
        Sense::click(),
    )
    .on_hover_cursor(CursorIcon::PointingHand)
}

/// A small label-like marker ("HOLD", "ON"); not clickable.
pub fn badge(ui: &mut Ui, text: &str, tone: Tone) -> Response {
    pill(
        ui,
        text.to_string(),
        tone,
        font::SMALL - 1.0,
        vec2(6.0, 2.0),
        Sense::hover(),
    )
}

/// A row of mutually exclusive choices drawn as one segmented control. Returns true when
/// the choice changed.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let p = palette(ui);
    let mut changed = false;
    Frame::new()
        .fill(p.sunken)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(RADIUS_S + 1))
        .inner_margin(Margin::same(2))
        .show(ui, |ui| {
            ui.spacing_mut().interact_size.y = 22.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.spacing_mut().button_padding.y = 2.0;
                for &(option, label) in options {
                    let on = *value == option;
                    let text = RichText::new(label).color(if on { p.on_accent } else { p.text });
                    let fill = if on { p.accent } else { Color32::TRANSPARENT };
                    let r = ui.add(
                        egui::Button::new(text)
                            .fill(fill)
                            .frame_when_inactive(on)
                            .corner_radius(CornerRadius::same(RADIUS_S))
                            // 22 + margin + border = a row's 28, so a label beside the
                            // control lines up with it.
                            .min_size(vec2(0.0, 22.0)),
                    );
                    if r.clicked() && !on {
                        *value = option;
                        changed = true;
                    }
                }
            });
        });
    changed
}

/// A coloured status dot (green / orange / red ...), `size` points across.
pub fn status_dot(ui: &mut Ui, tone: Tone, size: f32) -> Response {
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let c = tone.color(palette(ui));
    ui.painter()
        .circle_filled(rect.center(), size / 2.0 - 1.0, c);
    ui.painter().circle_stroke(
        rect.center(),
        size / 2.0 + 1.5,
        Stroke::new(2.0, mix(palette(ui).surface, c, 0.35)),
    );
    r
}

// ------------------------------------------------------------------------------------------
// Buttons
// ------------------------------------------------------------------------------------------

fn filled_button(ui: &mut Ui, text: &str, fill: Color32, on: Color32) -> Response {
    let fill = if ui.is_enabled() {
        fill
    } else {
        mix(fill, palette(ui).surface, 0.5)
    };
    ui.add(egui::Button::new(RichText::new(text).color(on).strong()).fill(fill))
}

/// The main action of a dialog or card.
pub fn primary_button(ui: &mut Ui, text: &str) -> Response {
    let p = palette(ui);
    filled_button(ui, text, p.accent, p.on_accent)
}

/// An action that removes or stops something.
pub fn danger_button(ui: &mut Ui, text: &str) -> Response {
    let p = palette(ui);
    filled_button(ui, text, p.danger, p.on_accent)
}

/// A square button with only an icon; it gets a background when hovered.
pub fn icon_button(ui: &mut Ui, icon: &str, tip: &str) -> Response {
    let p = palette(ui);
    let r = ui.add(
        egui::Button::new(RichText::new(icon).size(16.0).color(p.text_muted))
            .frame_when_inactive(false)
            .min_size(vec2(28.0, 28.0))
            .corner_radius(CornerRadius::same(RADIUS_S)),
    );
    if tip.is_empty() {
        r
    } else {
        r.on_hover_text(tip)
    }
}

/// An icon button that opens a menu (the "more" button of a card).
pub fn menu_icon_button<R>(
    ui: &mut Ui,
    icon: &str,
    tip: &str,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> Response {
    let p = palette(ui);
    let button = egui::Button::new(RichText::new(icon).size(18.0).color(p.text_muted))
        .frame_when_inactive(false)
        .min_size(vec2(28.0, 28.0))
        .corner_radius(CornerRadius::same(RADIUS_S));
    let (response, _) =
        egui::containers::menu::MenuButton::from_button(button).ui(ui, add_contents);
    if tip.is_empty() {
        response
    } else {
        response.on_hover_text(tip)
    }
}

/// Where a point lies between two others (0 at `a`, 1 at `b`), for hit tests in tests.
#[cfg(test)]
pub fn track_x(rect: Rect, t: f32) -> Pos2 {
    pos2(
        rect.left() + KNOB + t * (rect.width() - 2.0 * KNOB),
        rect.center().y,
    )
}

/// Notes where something was drawn, so UI tests can aim real mouse events at it (see
/// [`rect_of`]).
#[cfg(test)]
pub fn remember(ctx: &egui::Context, kind: &str, key: impl std::fmt::Display, rect: Rect) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(("test_rect", kind, key.to_string())), rect));
}

/// Forgets where something was drawn (call before drawing what is only there sometimes, so
/// [`try_rect_of`] can tell whether it is there now).
#[cfg(test)]
pub fn forget(ctx: &egui::Context, kind: &str, key: impl std::fmt::Display) {
    ctx.data_mut(|d| d.remove::<Rect>(egui::Id::new(("test_rect", kind, key.to_string()))));
}

/// Where [`remember`] saw the widget, if it was drawn.
#[cfg(test)]
pub fn try_rect_of(ctx: &egui::Context, kind: &str, key: impl std::fmt::Display) -> Option<Rect> {
    ctx.data(|d| d.get_temp(egui::Id::new(("test_rect", kind, key.to_string()))))
}

/// Where [`remember`] saw the widget last frame.
#[cfg(test)]
pub fn rect_of(ctx: &egui::Context, kind: &str, key: impl std::fmt::Display) -> Rect {
    ctx.data(|d| d.get_temp(egui::Id::new(("test_rect", kind, key.to_string()))))
        .unwrap_or_else(|| panic!("{kind} {key} was not drawn"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_positions_round_trip_for_linear_and_log_ranges() {
        for (min, max, log) in [
            (-24.0, 24.0, false),
            (0.0, 1.0, false),
            (20.0, 20_000.0, true),
        ] {
            for t in [0.0, 0.25, 0.5, 0.9, 1.0] {
                let v = from_norm(t, min, max, log);
                assert!(
                    (to_norm(v, min, max, log) - t).abs() < 1e-4,
                    "{min}..{max} at {t}"
                );
            }
            assert_eq!(to_norm(min - 5.0, min, max, log), 0.0, "clamped below");
            assert_eq!(
                to_norm(max * 2.0 + 5.0, min, max, log),
                1.0,
                "clamped above"
            );
        }
        // The middle of a log track is the geometric mean.
        assert!((from_norm(0.5, 20.0, 20_000.0, true) - 632.5).abs() < 1.0);
        assert_eq!(snap(0.26, 0.1), 0.3);
        assert_eq!(snap(0.26, 0.0), 0.26);
    }

    #[test]
    fn values_are_shown_and_read_back() {
        assert_eq!(format_value(3.0, 1, "dB", false, true), "+3.0 dB");
        assert_eq!(format_value(-3.0, 1, "dB", false, true), "-3.0 dB");
        assert_eq!(format_value(0.0, 1, "dB", false, true), "0.0 dB");
        assert_eq!(
            format_value(-0.01, 1, "dB", false, true),
            "0.0 dB",
            "no minus zero"
        );
        assert_eq!(format_value(0.35, 2, "", true, false), "35 %");
        assert_eq!(format_value(1200.0, 0, "Hz", false, false), "1200 Hz");
        assert_eq!(format_value(0.5, 2, "", false, false), "0.50");

        assert_eq!(parse_value("12,5", false), Some(12.5));
        assert_eq!(parse_value(" -3 dB", false), Some(-3.0));
        assert_eq!(parse_value("+4", false), Some(4.0));
        assert_eq!(parse_value("35%", true), Some(0.35));
        assert_eq!(parse_value("1200 Hz", false), Some(1200.0));
        assert_eq!(parse_value("abc", false), None);
        assert_eq!(parse_value("", false), None);
    }

    /// Runs `frames` frames of a UI that only holds one slider.
    fn run_slider(ctx: &egui::Context, value: &mut f32, events: Vec<egui::Event>, t: f64) -> Rect {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 200.0))),
            time: Some(t),
            events,
            ..Default::default()
        };
        let mut rect = Rect::NOTHING;
        ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let r = Slider::new(value, -12.0..=12.0)
                    .label("Pitch")
                    .unit("st")
                    .decimals(1)
                    .step(0.1)
                    .default_value(0.0)
                    .show(ui);
                rect = r.rect;
            });
        })
        .textures_delta
        .clear();
        rect
    }

    /// The track's rectangle inside a slider row (see `Slider::show`).
    fn track_of(row: Rect) -> Rect {
        Rect::from_min_max(
            pos2(row.left() + 120.0 + space::S + 2.0, row.top()),
            pos2(row.right() - 64.0 - space::S, row.bottom()),
        )
    }

    fn button(pos: Pos2, button: PointerButton, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: Default::default(),
        }
    }

    #[test]
    fn slider_click_sets_the_value_and_right_click_or_double_click_resets_it() {
        let ctx = egui::Context::default();
        let mut v = 0.0;
        let row = run_slider(&ctx, &mut v, vec![], 0.0);
        let track = track_of(row);
        let at = track_x(track, 0.75); // three quarters: +6 st
        run_slider(&ctx, &mut v, vec![egui::Event::PointerMoved(at)], 0.1);
        run_slider(
            &ctx,
            &mut v,
            vec![button(at, PointerButton::Primary, true)],
            0.2,
        );
        run_slider(
            &ctx,
            &mut v,
            vec![button(at, PointerButton::Primary, false)],
            0.3,
        );
        assert!((v - 6.0).abs() < 0.11, "clicked to {v}");
        assert_eq!((v * 10.0).round(), v * 10.0, "snapped to 0.1");

        // Right-click: back to the default, without the value jumping to the pointer first.
        let far = track_x(track, 0.1);
        run_slider(&ctx, &mut v, vec![egui::Event::PointerMoved(far)], 1.0);
        run_slider(
            &ctx,
            &mut v,
            vec![button(far, PointerButton::Secondary, true)],
            1.1,
        );
        assert!(
            (v - 6.0).abs() < 0.11,
            "a right press alone changes nothing"
        );
        run_slider(
            &ctx,
            &mut v,
            vec![button(far, PointerButton::Secondary, false)],
            1.2,
        );
        assert_eq!(v, 0.0, "right-click resets");

        // Double-click resets too.
        v = 5.0;
        let mut t = 2.0;
        for pressed in [true, false, true, false] {
            run_slider(
                &ctx,
                &mut v,
                vec![button(at, PointerButton::Primary, pressed)],
                t,
            );
            t += 0.05;
        }
        assert_eq!(v, 0.0, "double-click resets");
    }

    #[test]
    fn the_wheel_only_adjusts_a_slider_the_pointer_was_moved_onto() {
        let ctx = egui::Context::default();
        let mut v = 0.0;
        let row = run_slider(&ctx, &mut v, vec![], 0.0);
        let at = track_of(row).center();
        let wheel = |y: f32| egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, y),
            phase: egui::TouchPhase::Move,
            modifiers: Default::default(),
        };
        // The pointer appears over the slider without having moved there (the page scrolled
        // under it): the wheel is left to the page.
        let warp = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 200.0))),
            time: Some(0.1),
            events: vec![egui::Event::PointerMoved(at)],
            ..Default::default()
        };
        ctx.run_ui(warp, |_| {}).textures_delta.clear();
        for i in 0..20 {
            run_slider(&ctx, &mut v, vec![wheel(60.0)], 0.2 + f64::from(i) * 0.02);
        }
        assert_eq!(v, 0.0, "not armed: nothing changes");

        // Move onto it, then scroll: it steps.
        let nudge = at + vec2(3.0, 0.0);
        run_slider(&ctx, &mut v, vec![egui::Event::PointerMoved(nudge)], 1.0);
        for i in 0..20 {
            run_slider(&ctx, &mut v, vec![wheel(60.0)], 1.1 + f64::from(i) * 0.02);
        }
        assert!(v > 0.5, "scrolled up to {v}");
        assert!(v <= 12.0);
    }

    #[test]
    fn toggle_flips_on_click() {
        let ctx = egui::Context::default();
        let mut on = false;
        let frame = |on: &mut bool, events: Vec<egui::Event>, t: f64| {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(300.0, 100.0))),
                time: Some(t),
                events,
                ..Default::default()
            };
            let mut rect = Rect::NOTHING;
            ctx.run_ui(input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| rect = toggle(ui, on).rect);
            })
            .textures_delta
            .clear();
            rect
        };
        let at = frame(&mut on, vec![], 0.0).center();
        frame(&mut on, vec![egui::Event::PointerMoved(at)], 0.1);
        frame(&mut on, vec![button(at, PointerButton::Primary, true)], 0.2);
        frame(
            &mut on,
            vec![button(at, PointerButton::Primary, false)],
            0.3,
        );
        assert!(on);
        frame(&mut on, vec![button(at, PointerButton::Primary, true)], 0.9);
        frame(
            &mut on,
            vec![button(at, PointerButton::Primary, false)],
            1.0,
        );
        assert!(!on);
    }
}
