//! Look and feel in one place: colour tokens, spacing, corner radii and font sizes, for a dark
//! and a light theme. Everything else in `ui` takes its colours from [`palette`], so a theme
//! or accent change reaches every widget.
//!
//! Colour carries meaning: green = good, orange = warning, red = danger / panic, and the
//! accent (teal unless you pick another in Settings) = "this is active".
//!
//! The accent you pick is turned into a full set by [`Palette::new`]: nudged lighter or darker
//! until it reads on the theme's surfaces, hover and soft variants derived from it, and black
//! or white text on it, whichever contrasts more. The context's style holds the result (as its
//! hyperlink colour, which is the accent anyway), so [`palette`] needs no global state and
//! every egui context has its own.

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
    /// "Active": the accent colour you picked (made readable on this theme).
    pub accent: Color32,
    pub accent_hover: Color32,
    /// A quiet tint of the accent, for the background of active things.
    pub accent_soft: Color32,
    /// Text and icons on an accent, success or danger fill.
    pub on_accent: Color32,
    /// Text and icons on a success, danger or panic fill (those don't follow the accent).
    pub on_status: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub danger: Color32,
    /// The panic button and the safe-mode banner: a deeper red than `danger`.
    pub panic: Color32,
}

/// The dark theme's base. Its accent fields are the default (teal) set; [`Palette::new`]
/// replaces them with the ones for the accent you picked.
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
    accent: Color32::from_rgb(20, 184, 166),
    accent_hover: Color32::from_rgb(62, 197, 182),
    accent_soft: Color32::from_rgb(25, 58, 59),
    on_accent: Color32::BLACK,
    on_status: Color32::WHITE,
    success: Color32::from_rgb(52, 199, 123),
    warning: Color32::from_rgb(240, 170, 50),
    danger: Color32::from_rgb(238, 82, 82),
    panic: Color32::from_rgb(200, 30, 36),
};

/// The light theme's base; see [`DARK`] about the accent fields.
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
    accent: Color32::from_rgb(16, 147, 133),
    accent_hover: Color32::from_rgb(13, 121, 109),
    accent_soft: Color32::from_rgb(222, 240, 238),
    on_accent: Color32::WHITE,
    on_status: Color32::WHITE,
    success: Color32::from_rgb(20, 148, 84),
    warning: Color32::from_rgb(190, 118, 0),
    danger: Color32::from_rgb(208, 48, 48),
    panic: Color32::from_rgb(190, 25, 30),
};

/// The accent colours offered in Settings, as quick picks. The first is the default.
pub const ACCENTS: [(&str, Color32); 5] = [
    ("Teal", Color32::from_rgb(0x14, 0xB8, 0xA6)),
    ("Blue", Color32::from_rgb(0x3B, 0x82, 0xF6)),
    ("Indigo", Color32::from_rgb(0x63, 0x66, 0xF1)),
    ("Graphite", Color32::from_rgb(0xA1, 0xA1, 0xAA)),
    ("Violet", Color32::from_rgb(150, 110, 255)),
];

pub const DEFAULT_ACCENT: Color32 = ACCENTS[0].1;

/// "#14B8A6" (or "14b8a6", or the short "#1BA") as a colour.
pub fn parse_hex(text: &str) -> Option<Color32> {
    let hex = text.trim().trim_start_matches('#');
    let digit = |i: usize| u8::from_str_radix(hex.get(i..i + 1)?, 16).ok();
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    match hex.len() {
        6 => Some(Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?)),
        3 => Some(Color32::from_rgb(
            digit(0)? * 17,
            digit(1)? * 17,
            digit(2)? * 17,
        )),
        _ => None,
    }
}

/// A colour as "#RRGGBB".
pub fn to_hex(c: Color32) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b())
}

/// The accent a config holds; a missing or broken value gives the default.
pub fn accent_of(hex: &str) -> Color32 {
    parse_hex(hex).unwrap_or(DEFAULT_ACCENT)
}

/// Relative luminance (WCAG).
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

/// WCAG contrast ratio between two colours (1 to 21).
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// How much contrast the accent needs against the theme's backgrounds (as for icons and
/// large text).
const ACCENT_CONTRAST: f32 = 3.1;

/// White text on the accent is kept from this contrast up (bold and button-sized text).
const ON_ACCENT_CONTRAST: f32 = 3.0;

/// `c`, moved towards white (dark theme) or black (light theme) just enough to stand out on
/// every background of `base`. A colour that already does is returned as it is, so applying
/// this twice changes nothing.
fn readable_accent(base: &Palette, c: Color32, dark: bool) -> Color32 {
    let c = Color32::from_rgb(c.r(), c.g(), c.b());
    let toward = if dark { Color32::WHITE } else { Color32::BLACK };
    let worst = |c: Color32| {
        [base.bg, base.surface, base.surface_raised]
            .into_iter()
            .map(|b| contrast(c, b))
            .fold(f32::MAX, f32::min)
    };
    let mut out = c;
    let mut t = 0.0;
    while worst(out) < ACCENT_CONTRAST && t < 1.0 {
        t += 0.02;
        out = mix(c, toward, t);
    }
    out
}

impl Palette {
    /// The theme's colours with `accent` as the accent, and everything derived from it.
    pub fn new(dark: bool, accent: Color32) -> Palette {
        let base = if dark { DARK } else { LIGHT };
        let accent = readable_accent(&base, accent, dark);
        let soft = if dark { 0.20 } else { 0.14 };
        // White text where it reads well enough (as on most saturated colours), black on
        // light colours such as teal, graphite or yellow.
        let on_accent = if contrast(Color32::WHITE, accent) >= ON_ACCENT_CONTRAST
            || contrast(Color32::WHITE, accent) >= contrast(Color32::BLACK, accent)
        {
            Color32::WHITE
        } else {
            Color32::BLACK
        };
        // Hover moves away from the text colour, so the text only gets easier to read.
        let away = if on_accent == Color32::WHITE {
            Color32::BLACK
        } else {
            Color32::WHITE
        };
        Palette {
            accent,
            accent_hover: mix(accent, away, 0.18),
            accent_soft: mix(base.surface, accent, soft),
            on_accent,
            ..base
        }
    }
}

/// Hue (0..360), saturation and value (0..1) of a colour.
fn hsv(c: Color32) -> (f32, f32, f32) {
    let [r, g, b] = [c.r(), c.g(), c.b()].map(|v| f32::from(v) / 255.0);
    let max = r.max(g).max(b);
    let d = max - r.min(g).min(b);
    let hue = if d <= f32::EPSILON {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let sat = if max > 0.0 { d / max } else { 0.0 };
    (hue, sat, max)
}

/// Which status colour (red, orange or green) an accent would be mistaken for, if any. Those
/// colours mean panic / errors, warnings and "all good", so an accent close to one of them
/// makes the window harder to read at a glance.
pub fn resembles_status(c: Color32) -> Option<&'static str> {
    let (hue, sat, val) = hsv(c);
    if sat < 0.35 || val < 0.35 {
        return None; // greys and very dark colours read as neutral
    }
    [
        ("red (panic and errors)", DARK.danger),
        ("orange (warnings)", DARK.warning),
        ("green (all good)", DARK.success),
    ]
    .into_iter()
    .find(|(_, status)| {
        let d = (hue - hsv(*status).0).abs();
        d.min(360.0 - d) < 16.0
    })
    .map(|(name, _)| name)
}

/// The colours of the theme this `Ui` is drawn in, with the accent in use.
pub fn palette(ui: &Ui) -> Palette {
    let v = ui.visuals();
    Palette::new(v.dark_mode, v.hyperlink_color)
}

/// Like [`palette`], for code that only has the context (dialogs, toasts).
pub fn palette_of(ctx: &Context) -> Palette {
    let style = ctx.global_style();
    Palette::new(style.visuals.dark_mode, style.visuals.hyperlink_color)
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
/// [`set_choice`] and [`set_accent`] to change them later.
pub fn apply(ctx: &Context, choice: ThemeChoice) {
    install_fonts(ctx);
    set_accent(ctx, DEFAULT_ACCENT);
    set_choice(ctx, choice);
}

/// Restyles both themes for this accent colour; every widget follows from the next frame.
pub fn set_accent(ctx: &Context, accent: Color32) {
    ctx.style_mut_of(egui::Theme::Dark, |s| {
        style(s, &Palette::new(true, accent), true)
    });
    ctx.style_mut_of(egui::Theme::Light, |s| {
        style(s, &Palette::new(false, accent), false)
    });
}

pub fn set_choice(ctx: &Context, choice: ThemeChoice) {
    ctx.set_theme(match choice {
        ThemeChoice::Dark => egui::ThemePreference::Dark,
        ThemeChoice::Light => egui::ThemePreference::Light,
        ThemeChoice::System => egui::ThemePreference::System,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The quick picks and some awkward colours of your own: white, black, yellow, a mid grey,
    /// a dark navy.
    fn accents_to_try() -> Vec<Color32> {
        ACCENTS
            .iter()
            .map(|a| a.1)
            .chain([
                Color32::WHITE,
                Color32::BLACK,
                Color32::from_rgb(255, 235, 0),
                Color32::from_rgb(128, 128, 128),
                Color32::from_rgb(10, 20, 60),
            ])
            .collect()
    }

    #[test]
    fn text_is_readable_on_every_surface_in_both_themes() {
        for (accent, dark) in accents_to_try()
            .into_iter()
            .flat_map(|a| [(a, true), (a, false)])
        {
            let p = &Palette::new(dark, accent);
            let name = format!("{} {}", if dark { "dark" } else { "light" }, to_hex(accent));
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
            assert!(
                contrast(p.on_accent, p.accent) >= ON_ACCENT_CONTRAST,
                "{name}: text on the accent"
            );
            assert!(
                // (Pure white or black has nowhere further to go: then it stays the same.)
                contrast(p.on_accent, p.accent_hover) >= contrast(p.on_accent, p.accent),
                "{name}: hover makes the text easier to read"
            );
            assert!(contrast(p.on_status, p.panic) > 4.5, "{name}: on panic");
            assert!(contrast(p.on_status, p.danger) > 3.0, "{name}: on danger");
            assert!(contrast(p.text, p.accent_soft) > 7.0, "{name}: on soft");
        }
    }

    #[test]
    fn the_base_palettes_are_the_default_accents_set() {
        assert_eq!(Palette::new(true, DEFAULT_ACCENT), DARK);
        assert_eq!(Palette::new(false, DEFAULT_ACCENT), LIGHT);
    }

    #[test]
    fn a_light_accent_gets_dark_text_and_a_readable_accent_is_kept_as_it_is() {
        let teal = Palette::new(true, DEFAULT_ACCENT);
        assert_eq!(teal.accent, DEFAULT_ACCENT, "teal reads on dark as it is");
        assert_eq!(teal.on_accent, Color32::BLACK, "teal is light: black text");
        assert_eq!(Palette::new(true, ACCENTS[2].1).on_accent, Color32::WHITE);
        // Too light for a white page: darkened, and making it readable again changes nothing.
        let light = Palette::new(false, DEFAULT_ACCENT);
        assert_ne!(light.accent, DEFAULT_ACCENT);
        assert_eq!(Palette::new(false, light.accent), light, "idempotent");
        // Hover moves away from the text: lighter under black text, darker under white.
        assert!(luminance(teal.accent_hover) > luminance(teal.accent));
        assert_eq!(light.on_accent, Color32::WHITE);
        assert!(luminance(light.accent_hover) < luminance(light.accent));
        // A pale accent (yellow) gets black text in both themes.
        for dark in [true, false] {
            let yellow = Palette::new(dark, Color32::from_rgb(255, 235, 0));
            if dark {
                assert_eq!(yellow.on_accent, Color32::BLACK);
            }
            assert!(contrast(yellow.on_accent, yellow.accent) >= ON_ACCENT_CONTRAST);
        }
    }


    #[test]
    fn hex_codes_parse_and_print() {
        assert_eq!(parse_hex("#14B8A6"), Some(DEFAULT_ACCENT));
        assert_eq!(parse_hex(" 14b8a6 "), Some(DEFAULT_ACCENT));
        assert_eq!(parse_hex("#fff"), Some(Color32::WHITE));
        for bad in ["", "#12345", "#GGGGGG", "#1234567", "teal", "#é1"] {
            assert_eq!(parse_hex(bad), None, "{bad:?}");
        }
        assert_eq!(to_hex(Color32::from_rgb(59, 130, 246)), "#3B82F6");
        assert_eq!(accent_of("nonsense"), DEFAULT_ACCENT);
        for (_, c) in ACCENTS {
            assert_eq!(parse_hex(&to_hex(c)), Some(c));
        }
    }

    #[test]
    fn only_colours_like_red_orange_or_green_get_a_warning() {
        for (_, c) in ACCENTS {
            assert_eq!(resembles_status(c), None, "{}", to_hex(c));
        }
        for (hex, what) in [
            ("#E53935", "red"),
            ("#F4511E", "red"),
            ("#FB8C00", "orange"),
            ("#22C55E", "green"),
        ] {
            let found = resembles_status(parse_hex(hex).unwrap());
            assert!(
                found.is_some_and(|f| f.starts_with(what)),
                "{hex}: {found:?}"
            );
        }
        // Greys and very dark colours aren't mistaken for anything.
        assert_eq!(resembles_status(Color32::from_rgb(130, 118, 118)), None);
        assert_eq!(resembles_status(Color32::from_rgb(60, 10, 10)), None);
    }

    #[test]
    fn a_new_accent_restyles_both_themes() {
        let ctx = Context::default();
        apply(&ctx, ThemeChoice::Dark);
        assert_eq!(palette_of(&ctx), DARK);
        let blue = ACCENTS[1].1;
        set_accent(&ctx, blue);
        assert_eq!(palette_of(&ctx), Palette::new(true, blue));
        assert_eq!(ctx.global_style().visuals.selection.stroke.color, blue);
        set_choice(&ctx, ThemeChoice::Light);
        assert_eq!(palette_of(&ctx), Palette::new(false, blue));
    }

    #[test]
    fn colours_mix() {
        assert_eq!(mix(Color32::BLACK, Color32::WHITE, 0.0), Color32::BLACK);
        assert_eq!(mix(Color32::BLACK, Color32::WHITE, 1.0), Color32::WHITE);
        assert_eq!(
            mix(Color32::BLACK, Color32::WHITE, 0.5),
            Color32::from_rgb(128, 128, 128)
        );
    }

    #[test]
    fn both_themes_apply_and_switch() {
        let ctx = Context::default();
        apply(&ctx, ThemeChoice::Dark);
        assert!(ctx.global_style().visuals.dark_mode);
        assert_eq!(palette_of(&ctx), DARK);
        set_choice(&ctx, ThemeChoice::Light);
        assert!(!ctx.global_style().visuals.dark_mode);
        assert_eq!(palette_of(&ctx), LIGHT);
        assert_eq!(ctx.global_style().visuals.panel_fill, LIGHT.bg);
    }
}
