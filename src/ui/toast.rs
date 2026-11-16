//! Toasts: short messages in the bottom-right corner. The one way the app tells you something
//! happened: a mic switch, a saved preset, an import result, an error, or a delete you can
//! still undo.
//!
//! A toast has a kind (its colour and icon), stays a few seconds (longer for long texts and
//! errors, and for as long as the pointer rests on it), and may carry one action button such
//! as "Undo". Clicking the button hands its token back from [`Toasts::show`].

use super::icons;
use super::theme::{font, mix, palette_of, space};
use super::widgets::Tone;
use egui::{Align, Align2, Context, CornerRadius, Id, Layout, RichText, Stroke, vec2};

const INFO_S: f64 = 4.0;
const ERROR_S: f64 = 7.0;
/// How long a toast with an action (Undo) stays.
pub const ACTION_S: f64 = 8.0;
/// Extra time per character beyond a short message.
const READ_S_PER_CHAR: f64 = 0.04;
const MAX_SHOWN: usize = 5;
const FADE_IN_S: f64 = 0.12;
const FADE_OUT_S: f64 = 0.35;
const WIDTH: f32 = 380.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Info,
    Success,
    Warning,
    Error,
}

impl Kind {
    fn tone(self) -> Tone {
        match self {
            Kind::Info => Tone::Accent,
            Kind::Success => Tone::Success,
            Kind::Warning => Tone::Warning,
            Kind::Error => Tone::Danger,
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Kind::Info => icons::INFO,
            Kind::Success => icons::CHECK_CIRCLE,
            Kind::Warning => icons::WARNING,
            Kind::Error => icons::ERROR,
        }
    }
}

struct Toast {
    text: String,
    kind: Kind,
    born: f64,
    until: f64,
    /// Button label and the token handed back when it is clicked.
    action: Option<(String, u64)>,
}

#[derive(Default)]
pub struct Toasts {
    items: Vec<Toast>,
}

/// How long a message of this kind and length is shown.
fn duration(kind: Kind, text: &str, action: bool) -> f64 {
    let base = match (action, kind) {
        (true, _) => ACTION_S,
        (false, Kind::Error | Kind::Warning) => ERROR_S,
        (false, _) => INFO_S,
    };
    let extra = text.chars().count().saturating_sub(60) as f64 * READ_S_PER_CHAR;
    base + extra.min(8.0)
}

impl Toasts {
    fn add(&mut self, ctx: &Context, kind: Kind, text: String, action: Option<(String, u64)>) {
        let now = ctx.input(|i| i.time);
        let until = now + duration(kind, &text, action.is_some());
        // The same message again (a key held down, a repeated error) restarts it instead of
        // stacking up.
        if let Some(same) = self
            .items
            .iter_mut()
            .find(|t| t.text == text && t.action.is_none() && action.is_none())
        {
            same.until = until;
            same.kind = kind;
            return;
        }
        self.items.push(Toast {
            text,
            kind,
            born: now,
            until,
            action,
        });
        if self.items.len() > MAX_SHOWN {
            // The oldest one without an action goes first: an Undo must not be pushed out by
            // chatter.
            let drop = self
                .items
                .iter()
                .position(|t| t.action.is_none())
                .unwrap_or(0);
            self.items.remove(drop);
        }
        ctx.request_repaint();
    }

    /// A neutral message.
    pub fn push(&mut self, ctx: &Context, text: impl Into<String>) {
        self.add(ctx, Kind::Info, text.into(), None);
    }

    /// Something worked.
    pub fn success(&mut self, ctx: &Context, text: impl Into<String>) {
        self.add(ctx, Kind::Success, text.into(), None);
    }

    /// Something needs attention.
    pub fn warn(&mut self, ctx: &Context, text: impl Into<String>) {
        self.add(ctx, Kind::Warning, text.into(), None);
    }

    /// Something failed.
    pub fn error(&mut self, ctx: &Context, text: impl Into<String>) {
        self.add(ctx, Kind::Error, text.into(), None);
    }

    /// A message with one button (e.g. "Undo"). When it is clicked, [`Toasts::show`] returns
    /// `token`.
    #[allow(dead_code)] // first used by "Undo" on the soundboard page
    pub fn push_action(
        &mut self,
        ctx: &Context,
        kind: Kind,
        text: impl Into<String>,
        label: impl Into<String>,
        token: u64,
    ) {
        self.add(ctx, kind, text.into(), Some((label.into(), token)));
    }

    /// Removes the toast carrying this action (it was used some other way, or is void).
    #[allow(dead_code)] // first used by "Undo" on the soundboard page
    pub fn dismiss_action(&mut self, token: u64) {
        self.items
            .retain(|t| t.action.as_ref().is_none_or(|(_, k)| *k != token));
    }

    /// Whether the action with this token can still be clicked.
    #[allow(dead_code)] // first used by "Undo" on the soundboard page
    pub fn has_action(&self, token: u64) -> bool {
        self.items
            .iter()
            .any(|t| t.action.as_ref().is_some_and(|(_, k)| *k == token))
    }

    #[cfg(test)]
    pub fn contains(&self, needle: &str) -> bool {
        self.items.iter().any(|t| t.text.contains(needle))
    }

    #[cfg(test)]
    pub fn kind_of(&self, needle: &str) -> Option<Kind> {
        self.items
            .iter()
            .find(|t| t.text.contains(needle))
            .map(|t| t.kind)
    }

    /// Draws the toasts; returns the tokens of the action buttons clicked this frame.
    pub fn show(&mut self, ctx: &Context) -> Vec<u64> {
        let now = ctx.input(|i| i.time);
        self.items.retain(|t| t.until > now);
        let mut clicked = Vec::new();
        if self.items.is_empty() {
            return clicked;
        }
        let p = palette_of(ctx);
        let mut close = None;
        let dt = f64::from(ctx.input(|i| i.stable_dt));
        // The area is exactly as big as the toasts in it (a plain top-down column of fixed
        // width), so it never covers, and never takes clicks from, anything else.
        let area = egui::Area::new(Id::new("toasts"))
            .anchor(Align2::RIGHT_BOTTOM, vec2(-space::L, -space::L))
            .order(egui::Order::Foreground)
            .movable(false)
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = space::S;
                    // Oldest on top, the newest nearest the corner.
                    for (i, t) in self.items.iter_mut().enumerate() {
                        let tone = t.kind.tone().color(p);
                        let fade = ((now - t.born) / FADE_IN_S)
                            .min((t.until - now) / FADE_OUT_S)
                            .clamp(0.0, 1.0) as f32;
                        let frame = egui::Frame::new()
                            .fill(mix(p.surface_raised, tone, 0.10))
                            .stroke(Stroke::new(1.0, mix(p.border_strong, tone, 0.55)))
                            .corner_radius(CornerRadius::same(8))
                            .inner_margin(egui::Margin::symmetric(12, 9))
                            .multiply_with_opacity(fade);
                        let shown = frame.show(ui, |ui| {
                            ui.set_opacity(fade);
                            ui.set_width(WIDTH);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = space::S;
                                ui.label(RichText::new(t.kind.icon()).size(17.0).color(tone));
                                // Buttons first (right to left), the text takes what is left.
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    let x = egui::Button::new(
                                        RichText::new(icons::CLOSE).size(12.0).color(p.text_muted),
                                    )
                                    .frame_when_inactive(false)
                                    .small();
                                    if ui.add(x).on_hover_text("Dismiss").clicked() {
                                        close = Some(i);
                                    }
                                    if let Some((label, token)) = &t.action {
                                        let b = egui::Button::new(
                                            RichText::new(label).strong().color(p.accent),
                                        );
                                        let r = ui.add(b);
                                        #[cfg(test)]
                                        super::widgets::remember(
                                            ui.ctx(),
                                            "toast_action",
                                            token,
                                            r.rect,
                                        );
                                        if r.clicked() {
                                            clicked.push(*token);
                                            close = Some(i);
                                        }
                                    }
                                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&t.text)
                                                    .size(font::BODY)
                                                    .color(p.text),
                                            )
                                            .wrap(),
                                        );
                                    });
                                });
                            });
                        });
                        // Reading it (pointer on it) stops the clock.
                        if shown.response.contains_pointer() {
                            t.until += dt;
                            t.until = t.until.max(now + FADE_OUT_S + 0.5);
                        }
                    }
                });
            });
        #[cfg(test)]
        super::widgets::remember(ctx, "toast_area", 0, area.response.rect);
        #[cfg(not(test))]
        let _ = area;
        if let Some(i) = close {
            self.items.remove(i);
        }
        // Keep animating (fades and expiry) while anything is shown.
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
        clicked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(ctx: &Context, toasts: &mut Toasts, t: f64, events: Vec<egui::Event>) -> Vec<u64> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                vec2(900.0, 600.0),
            )),
            time: Some(t),
            events,
            ..Default::default()
        };
        let mut clicked = Vec::new();
        ctx.run_ui(input, |ui| clicked = toasts.show(ui.ctx()))
            .textures_delta
            .clear();
        clicked
    }

    #[test]
    fn toasts_expire_by_kind_and_long_texts_stay_longer() {
        let ctx = Context::default();
        let mut toasts = Toasts::default();
        frame(&ctx, &mut toasts, 0.0, vec![]);
        toasts.push(&ctx, "Preset saved");
        toasts.error(&ctx, "Could not import: broken file");
        assert_eq!(toasts.kind_of("Preset"), Some(Kind::Info));
        assert_eq!(toasts.kind_of("import"), Some(Kind::Error));
        frame(&ctx, &mut toasts, INFO_S + 0.1, vec![]);
        assert!(!toasts.contains("Preset saved"), "info is gone");
        assert!(toasts.contains("Could not import"), "errors stay longer");
        frame(&ctx, &mut toasts, ERROR_S + 0.1, vec![]);
        assert!(!toasts.contains("Could not import"));

        assert!(duration(Kind::Info, &"x".repeat(200), false) > INFO_S + 4.0);
        assert_eq!(duration(Kind::Info, "short", false), INFO_S);
        assert_eq!(duration(Kind::Info, "short", true), ACTION_S);
    }

    #[test]
    fn the_same_message_twice_is_shown_once_and_an_undo_is_never_pushed_out() {
        let ctx = Context::default();
        let mut toasts = Toasts::default();
        frame(&ctx, &mut toasts, 0.0, vec![]);
        toasts.push(&ctx, "Mic muted");
        toasts.push(&ctx, "Mic muted");
        assert_eq!(toasts.items.len(), 1);
        toasts.push_action(&ctx, Kind::Info, "Deleted \"airhorn\"", "Undo", 7);
        for i in 0..10 {
            toasts.push(&ctx, format!("message {i}"));
        }
        assert_eq!(toasts.items.len(), MAX_SHOWN);
        assert!(toasts.has_action(7), "the undo is still there");
        assert!(toasts.contains("message 9") && !toasts.contains("message 0"));
        toasts.dismiss_action(7);
        assert!(!toasts.has_action(7));
    }

    /// The toast layer lies on top of the window: it must be no bigger than the toasts, or it
    /// would swallow clicks and drops meant for what is underneath.
    #[test]
    fn toasts_only_cover_their_own_corner() {
        let ctx = Context::default();
        let mut toasts = Toasts::default();
        frame(&ctx, &mut toasts, 0.0, vec![]);
        toasts.push(&ctx, "Folder created");
        toasts.error(&ctx, "Could not import: ".to_string() + &"long ".repeat(40));
        frame(&ctx, &mut toasts, 0.1, vec![]);
        frame(&ctx, &mut toasts, 0.2, vec![]);
        let area = crate::ui::widgets::rect_of(&ctx, "toast_area", 0);
        assert!(area.width() <= WIDTH + 30.0, "{area:?}");
        assert!(
            area.left() > 900.0 - WIDTH - 60.0,
            "in the right corner: {area:?}"
        );
        assert!(area.top() > 300.0, "only as tall as its toasts: {area:?}");
        assert!(area.bottom() <= 600.0 - space::L + 1.0);
    }

    #[test]
    fn clicking_the_action_returns_its_token_once() {
        let ctx = Context::default();
        let mut toasts = Toasts::default();
        frame(&ctx, &mut toasts, 0.0, vec![]);
        toasts.push_action(&ctx, Kind::Info, "Deleted \"airhorn\"", "Undo", 42);
        frame(&ctx, &mut toasts, 0.3, vec![]);
        frame(&ctx, &mut toasts, 0.4, vec![]);
        let pos = crate::ui::widgets::rect_of(&ctx, "toast_action", 42).center();
        let press = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(&ctx, &mut toasts, 0.5, vec![egui::Event::PointerMoved(pos)]);
        assert!(frame(&ctx, &mut toasts, 0.6, vec![press(true)]).is_empty());
        let got = frame(&ctx, &mut toasts, 0.7, vec![press(false)]);
        assert_eq!(got, [42], "Undo was clicked");
        assert!(!toasts.has_action(42), "and the toast is gone");
        assert!(
            frame(&ctx, &mut toasts, 0.8, vec![]).is_empty(),
            "only once"
        );
    }
}
