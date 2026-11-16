//! Look and feel in one place: colour tokens, spacing, corner radii and font sizes, for a dark
//! and a light theme. Everything else in `ui` takes its colours from [`palette`], so a theme
//! change reaches every widget.
//!
//! Colour carries meaning: green = good, orange = warning, red = danger / panic, and the
//! violet accent = "this is active".

use egui::{Color32, Context, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Ui};
use serde::{Deserialize, Serialize};

/// Which theme to use (saved in the config).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeChoice {
    #[default]
    Dark,
    Light,
    /// Follow the Windows app mode.
    System,
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 3] = [ThemeChoice::Dark, ThemeChoice::Light, ThemeChoice::System];

    pub fn label(self) -> &'static str {
        match self {
            ThemeChoice::Dark => "Dark",
            ThemeChoice::Light => "Light",
            ThemeChoice::System => "Same as Windows",
        }
    }
}

/// The colour tokens of one theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    /// The window behind everything.
    pub bg: Color32,
    /// Cards and panels.
    pub surface: Color32,
    /// Things lying on a card: buttons, inputs on hover, popups.
    pub surface_raised: Color32,
    /// Wells: meter backgrounds, slider rails, text fields.
    pub sunken: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub text_faint: Color32,
    /// "Active": the VoxShift violet.
    pub accent: Color32,
    pub accent_hover: Color32,
    /// A quiet tint of the accent, for the background of active things.
    pub accent_soft: Color32,
    /// Text and icons on an accent, success or danger fill.
    pub on_accent: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub danger: Color32,
    /// The panic button and the safe-mode banner: a deeper red than `danger`.
    pub panic: Color32,
}

pub const DARK: Palette = Palette {
    bg: Color32::from_rgb(17, 17, 21),
    surface: Color32::from_rgb(26, 26, 32),
    surface_raised: Color32::from_rgb(38, 38, 47),
    sunken: Color32::from_rgb(11, 11, 14),
    border: Color32::from_rgb(46, 46, 57),
    border_strong: Color32::from_rgb(72, 72, 88),
    text: Color32::from_rgb(236, 236, 242),
    text_muted: Color32::from_rgb(160, 160, 176),
    text_faint: Color32::from_rgb(108, 108, 124),
    accent: Color32::from_rgb(150, 110, 255),
    accent_hover: Color32::from_rgb(171, 138, 255),
    accent_soft: Color32::from_rgb(46, 38, 78),
    on_accent: Color32::WHITE,
    success: Color32::from_rgb(52, 199, 123),
    warning: Color32::from_rgb(240, 170, 50),
    danger: Color32::from_rgb(238, 82, 82),
    panic: Color32::from_rgb(200, 30, 36),
};

pub const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(243, 243, 247),
    surface: Color32::from_rgb(255, 255, 255),
    surface_raised: Color32::from_rgb(236, 236, 243),
    sunken: Color32::from_rgb(226, 226, 234),
    border: Color32::from_rgb(216, 216, 226),
    border_strong: Color32::from_rgb(176, 176, 192),
    text: Color32::from_rgb(24, 24, 31),
    text_muted: Color32::from_rgb(88, 88, 104),
    text_faint: Color32::from_rgb(136, 136, 150),
    accent: Color32::from_rgb(118, 78, 232),
    accent_hover: Color32::from_rgb(98, 58, 212),
    accent_soft: Color32::from_rgb(234, 227, 255),
    on_accent: Color32::WHITE,
    success: Color32::from_rgb(20, 148, 84),
    warning: Color32::from_rgb(190, 118, 0),
    danger: Color32::from_rgb(208, 48, 48),
    panic: Color32::from_rgb(190, 25, 30),
};

impl Palette {
    pub fn of(dark: bool) -> &'static Palette {
        if dark { &DARK } else { &LIGHT }
    }
}

/// The colours of the theme this `Ui` is drawn in.
pub fn palette(ui: &Ui) -> &'static Palette {
    Palette::of(ui.visuals().dark_mode)
}

/// Like [`palette`], for code that only has the context (dialogs, toasts).
pub fn palette_of(ctx: &Context) -> &'static Palette {
    Palette::of(ctx.global_style().visuals.dark_mode)
}

/// The spacing scale. Everything is laid out on these steps (the scale is kept whole, also
/// where a step has no user yet).
#[allow(dead_code)]
pub mod space {
    pub const XS: f32 = 4.0;
    pub const S: f32 = 8.0;
    pub const M: f32 = 12.0;
    pub const L: f32 = 16.0;
    pub const XL: f32 = 24.0;
    pub const XXL: f32 = 32.0;
}

/// Corner radii: small controls and cards. (Pills are fully round: half their height.)
pub const RADIUS_S: u8 = 6;
pub const RADIUS_M: u8 = 10;

/// Font sizes.
pub mod font {
    pub const TITLE: f32 = 20.0;
    pub const SECTION: f32 = 15.5;
    pub const BODY: f32 = 14.0;
    pub const SMALL: f32 = 12.0;
}

/// `a` with `t` (0..1) of `b` mixed in.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let ch = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgba_premultiplied(
        ch(a.r(), b.r()),
        ch(a.g(), b.g()),
        ch(a.b(), b.b()),
        ch(a.a(), b.a()),
    )
}

/// Black or white, whichever reads better on `bg` (in any theme).
pub fn text_on(bg: Color32) -> Color32 {
    let l = 0.2126 * f32::from(bg.r()) + 0.7152 * f32::from(bg.g()) + 0.0722 * f32::from(bg.b());
    if l > 150.0 {
        Color32::BLACK
    } else {
        Color32::WHITE
    }
}

fn style(style: &mut egui::Style, p: &Palette, dark: bool) {
    for (ts, size) in [
        (TextStyle::Small, font::SMALL),
        (TextStyle::Body, font::BODY),
        (TextStyle::Button, font::BODY),
        (TextStyle::Monospace, 13.0),
        (TextStyle::Heading, font::TITLE),
    ] {
        let family = if ts == TextStyle::Monospace {
            FontFamily::Monospace
        } else {
            FontFamily::Proportional
        };
        style.text_styles.insert(ts, FontId::new(size, family));
    }
    let sp = &mut style.spacing;
    sp.item_spacing = egui::vec2(space::S, space::S);
    sp.button_padding = egui::vec2(space::M, 6.0);
    sp.interact_size.y = 28.0;
    sp.slider_width = 150.0;
    sp.menu_margin = egui::Margin::same(6);
    sp.window_margin = egui::Margin::same(16);
    sp.scroll.bar_width = 8.0;
    sp.scroll.floating = true;
    sp.tooltip_width = 360.0;

    let v = &mut style.visuals;
    v.dark_mode = dark;
    v.panel_fill = p.bg;
    v.window_fill = p.surface_raised;
    v.window_stroke = Stroke::new(1.0, p.border_strong);
    v.window_corner_radius = CornerRadius::same(RADIUS_M);
    v.menu_corner_radius = CornerRadius::same(RADIUS_S + 2);
    v.extreme_bg_color = p.sunken;
    v.faint_bg_color = p.surface;
    v.code_bg_color = p.sunken;
    v.hyperlink_color = p.accent;
    v.warn_fg_color = p.warning;
    v.error_fg_color = p.danger;
    v.weak_text_color = Some(p.text_muted);
    v.selection.bg_fill = mix(p.surface_raised, p.accent, if dark { 0.45 } else { 0.30 });
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.popup_shadow.color = Color32::from_black_alpha(if dark { 110 } else { 40 });
    v.window_shadow.color = Color32::from_black_alpha(if dark { 130 } else { 50 });

    let radius = CornerRadius::same(RADIUS_S);
    let w = &mut v.widgets;
    for state in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        state.corner_radius = radius;
        state.expansion = 0.0;
    }
    // Labels, separators and frames.
    w.noninteractive.bg_fill = p.surface;
    w.noninteractive.weak_bg_fill = p.surface;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    // Buttons, combo boxes, checkboxes.
    w.inactive.bg_fill = p.surface_raised;
    w.inactive.weak_bg_fill = p.surface_raised;
    // (A little stronger than a card's border, so an empty checkbox is still easy to find.)
    w.inactive.bg_stroke = Stroke::new(1.0, mix(p.border, p.border_strong, 0.6));
    w.inactive.fg_stroke = Stroke::new(1.0, p.text);
    let hover = mix(p.surface_raised, p.text, 0.08);
    w.hovered.bg_fill = hover;
    w.hovered.weak_bg_fill = hover;
    w.hovered.bg_stroke = Stroke::new(1.0, p.border_strong);
    w.hovered.fg_stroke = Stroke::new(1.5, p.text);
    let pressed = mix(p.surface_raised, p.accent, 0.35);
    w.active.bg_fill = pressed;
    w.active.weak_bg_fill = pressed;
    w.active.bg_stroke = Stroke::new(1.0, p.accent);
    w.active.fg_stroke = Stroke::new(1.5, p.text);
    w.open.bg_fill = hover;
    w.open.weak_bg_fill = hover;
    w.open.bg_stroke = Stroke::new(1.0, p.accent);
    w.open.fg_stroke = Stroke::new(1.0, p.text);
}

/// Installs the icon font next to the text font, so icons can be written inside any label.
fn install_fonts(ctx: &Context) {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);
}

/// Sets up fonts and both themes, and switches to `choice`. Call once at start; use
/// [`set_choice`] to switch later.
pub fn apply(ctx: &Context, choice: ThemeChoice) {
    install_fonts(ctx);
    ctx.style_mut_of(egui::Theme::Dark, |s| style(s, &DARK, true));
    ctx.style_mut_of(egui::Theme::Light, |s| style(s, &LIGHT, false));
    set_choice(ctx, choice);
}

pub fn set_choice(ctx: &Context, choice: ThemeChoice) {
    ctx.set_theme(match choice {
        ThemeChoice::Dark => egui::ThemePreference::Dark,
        ThemeChoice::Light => egui::ThemePreference::Light,
        ThemeChoice::System => egui::ThemePreference::System,
    });
}

// The fixed colours of the layout before the redesign; the pages that still use them move to
// [`palette`] one by one.
pub const ACCENT: Color32 = DARK.accent;
pub const ON: Color32 = Color32::from_rgb(40, 170, 100);
pub const OFF: Color32 = Color32::from_rgb(90, 90, 96);
pub const WARN: Color32 = DARK.warning;
pub const BAD: Color32 = DARK.danger;

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(c: Color32) -> f32 {
        let lin = |v: u8| {
            let v = f32::from(v) / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
    }

    /// WCAG contrast ratio.
    fn contrast(a: Color32, b: Color32) -> f32 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    #[test]
    fn text_is_readable_on_every_surface_in_both_themes() {
        for (name, p) in [("dark", &DARK), ("light", &LIGHT)] {
            for (what, bg) in [
                ("bg", p.bg),
                ("surface", p.surface),
                ("raised", p.surface_raised),
            ] {
                assert!(contrast(p.text, bg) > 7.0, "{name}: text on {what}");
                assert!(contrast(p.text_muted, bg) > 4.5, "{name}: muted on {what}");
                assert!(contrast(p.text_faint, bg) > 2.6, "{name}: faint on {what}");
                for (tone, c) in [
                    ("accent", p.accent),
                    ("success", p.success),
                    ("warning", p.warning),
                    ("danger", p.danger),
                ] {
                    assert!(
                        contrast(c, bg) > 3.0,
                        "{name}: {tone} on {what}: {}",
                        contrast(c, bg)
                    );
                }
            }
            assert!(contrast(p.on_accent, p.accent) > 3.0, "{name}: on accent");
            assert!(contrast(p.on_accent, p.panic) > 4.5, "{name}: on panic");
            assert!(contrast(p.text, p.accent_soft) > 7.0, "{name}: on soft");
        }
    }

    #[test]
    fn mixing_and_text_colour_choice() {
        assert_eq!(mix(Color32::BLACK, Color32::WHITE, 0.0), Color32::BLACK);
        assert_eq!(mix(Color32::BLACK, Color32::WHITE, 1.0), Color32::WHITE);
        assert_eq!(
            mix(Color32::BLACK, Color32::WHITE, 0.5),
            Color32::from_rgb(128, 128, 128)
        );
        assert_eq!(text_on(Color32::from_gray(52)), Color32::WHITE);
        assert_eq!(text_on(Color32::from_rgb(230, 200, 60)), Color32::BLACK);
    }

    #[test]
    fn both_themes_apply_and_switch() {
        let ctx = Context::default();
        apply(&ctx, ThemeChoice::Dark);
        assert!(ctx.global_style().visuals.dark_mode);
        assert_eq!(palette_of(&ctx), &DARK);
        set_choice(&ctx, ThemeChoice::Light);
        assert!(!ctx.global_style().visuals.dark_mode);
        assert_eq!(palette_of(&ctx), &LIGHT);
        assert_eq!(ctx.global_style().visuals.panel_fill, LIGHT.bg);
    }
}
