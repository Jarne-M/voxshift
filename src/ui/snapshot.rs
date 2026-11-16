//! Test-only: paints egui frames into PNG files with a small software rasterizer, so the
//! pages can be looked at (any size, either theme) without opening a window or touching an
//! audio device.
//!
//! Set `VOXSHIFT_SHOTS` to a folder and run the ignored tests to get the pictures:
//!
//! ```text
//! $env:VOXSHIFT_SHOTS = "C:\temp\shots"; cargo test shots -- --ignored
//! ```
//!
//! The rasterizer does what egui's own renderers do: it fills the triangles egui tessellates,
//! multiplying the interpolated vertex colour with the texture (the font atlas) and blending
//! with premultiplied alpha in gamma space.

use super::theme::{self, ThemeChoice};
use egui::epaint::textures::TexturesDelta;
use egui::epaint::{ClippedPrimitive, Primitive, Vertex};
use egui::{Color32, ColorImage, Context, ImageData, Pos2, Rect, TextureId, Ui, Vec2};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The folder screenshots go to, if `VOXSHIFT_SHOTS` names one.
pub fn out_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("VOXSHIFT_SHOTS")?);
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// The textures egui has handed out so far (in practice: the font atlas).
#[derive(Default)]
pub struct Textures(HashMap<TextureId, ColorImage>);

impl Textures {
    pub fn apply(&mut self, delta: &TexturesDelta) {
        for (id, deltas) in &delta.set {
            for d in deltas.iter() {
                let ImageData::Color(img) = &d.image;
                match d.pos {
                    None => {
                        self.0.insert(*id, (**img).clone());
                    }
                    Some([x0, y0]) => {
                        let Some(dst) = self.0.get_mut(id) else {
                            continue;
                        };
                        for y in 0..img.size[1] {
                            for x in 0..img.size[0] {
                                let (dx, dy) = (x0 + x, y0 + y);
                                if dx < dst.size[0] && dy < dst.size[1] {
                                    dst.pixels[dy * dst.size[0] + dx] =
                                        img.pixels[y * img.size[0] + x];
                                }
                            }
                        }
                    }
                }
            }
        }
        for id in &delta.free {
            self.0.remove(id);
        }
    }
}

/// Premultiplied RGBA, 0..1.
type Rgba = [f32; 4];

fn rgba(c: Color32) -> Rgba {
    [
        f32::from(c.r()) / 255.0,
        f32::from(c.g()) / 255.0,
        f32::from(c.b()) / 255.0,
        f32::from(c.a()) / 255.0,
    ]
}

/// Bilinear sample at normalised `uv`, clamped at the edges.
fn sample(tex: &ColorImage, uv: Pos2) -> Rgba {
    let (w, h) = (tex.size[0], tex.size[1]);
    if w == 0 || h == 0 {
        return [1.0; 4];
    }
    let x = uv.x * w as f32 - 0.5;
    let y = uv.y * h as f32 - 0.5;
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let at = |xi: f32, yi: f32| {
        let xi = (xi.max(0.0) as usize).min(w - 1);
        let yi = (yi.max(0.0) as usize).min(h - 1);
        rgba(tex.pixels[yi * w + xi])
    };
    let (a, b, c, d) = (
        at(x0, y0),
        at(x0 + 1.0, y0),
        at(x0, y0 + 1.0),
        at(x0 + 1.0, y0 + 1.0),
    );
    std::array::from_fn(|i| {
        let top = a[i] + (b[i] - a[i]) * fx;
        let bottom = c[i] + (d[i] - c[i]) * fx;
        top + (bottom - top) * fy
    })
}

/// An opaque picture being painted.
pub struct Canvas {
    pub width: usize,
    pub height: usize,
    /// RGB, 0..1.
    rgb: Vec<[f32; 3]>,
}

impl Canvas {
    pub fn new(width: usize, height: usize, background: Color32) -> Self {
        let bg = rgba(background);
        Self {
            width,
            height,
            rgb: vec![[bg[0], bg[1], bg[2]]; width * height],
        }
    }

    pub fn pixel(&self, x: usize, y: usize) -> Color32 {
        let p = self.rgb[y * self.width + x];
        let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        Color32::from_rgb(c(p[0]), c(p[1]), c(p[2]))
    }

    fn triangle(&mut self, v: [&Vertex; 3], tex: Option<&ColorImage>, clip: [usize; 4], ppp: f32) {
        let p = v.map(|v| (v.pos.x * ppp, v.pos.y * ppp));
        let edge = |a: (f32, f32), b: (f32, f32), c: (f32, f32)| {
            (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
        };
        let area = edge(p[0], p[1], p[2]);
        if area.abs() < 1e-9 {
            return;
        }
        let min = |f: fn(&(f32, f32)) -> f32| p.iter().map(f).fold(f32::MAX, f32::min);
        let max = |f: fn(&(f32, f32)) -> f32| p.iter().map(f).fold(f32::MIN, f32::max);
        let x0 = (min(|q| q.0).floor().max(0.0) as usize).max(clip[0]);
        let y0 = (min(|q| q.1).floor().max(0.0) as usize).max(clip[1]);
        let x1 = (max(|q| q.0).ceil().max(0.0) as usize).min(clip[2]);
        let y1 = (max(|q| q.1).ceil().max(0.0) as usize).min(clip[3]);
        let colors = v.map(|v| rgba(v.color));
        // Every vertex on the atlas' white pixel: a plain fill, no need to sample.
        let flat = v.iter().all(|v| v.uv == Pos2::ZERO);
        for y in y0..y1 {
            for x in x0..x1 {
                // Sample a hair off the pixel centre, so a centre never lies exactly on an
                // edge two triangles share (it would be painted twice, or not at all).
                let s = (x as f32 + 0.5 + 0.000_37, y as f32 + 0.5 + 0.000_73);
                let w0 = edge(p[1], p[2], s) / area;
                let w1 = edge(p[2], p[0], s) / area;
                let w2 = edge(p[0], p[1], s) / area;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let mut src: Rgba = std::array::from_fn(|i| {
                    colors[0][i] * w0 + colors[1][i] * w1 + colors[2][i] * w2
                });
                if !flat && let Some(tex) = tex {
                    let uv = Pos2::new(
                        v[0].uv.x * w0 + v[1].uv.x * w1 + v[2].uv.x * w2,
                        v[0].uv.y * w0 + v[1].uv.y * w1 + v[2].uv.y * w2,
                    );
                    let t = sample(tex, uv);
                    for (s, t) in src.iter_mut().zip(t) {
                        *s *= t;
                    }
                }
                let dst = &mut self.rgb[y * self.width + x];
                for (d, s) in dst.iter_mut().zip(src) {
                    *d = s + *d * (1.0 - src[3]);
                }
            }
        }
    }

    /// Paints what egui tessellated for one frame.
    pub fn draw(&mut self, primitives: &[ClippedPrimitive], textures: &Textures, ppp: f32) {
        for prim in primitives {
            let Primitive::Mesh(mesh) = &prim.primitive else {
                continue;
            };
            let px = |v: f32, limit: usize| ((v * ppp).round().max(0.0) as usize).min(limit);
            let clip = [
                px(prim.clip_rect.left(), self.width),
                px(prim.clip_rect.top(), self.height),
                px(prim.clip_rect.right(), self.width),
                px(prim.clip_rect.bottom(), self.height),
            ];
            let tex = textures.0.get(&mesh.texture_id);
            for tri in mesh.indices.as_chunks::<3>().0 {
                let v = [0, 1, 2].map(|i| &mesh.vertices[tri[i] as usize]);
                self.triangle(v, tex, clip, ppp);
            }
        }
    }

    pub fn save_png(&self, path: &Path) {
        let file = std::fs::File::create(path).expect("create png");
        let mut encoder = png::Encoder::new(
            std::io::BufWriter::new(file),
            self.width as u32,
            self.height as u32,
        );
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("png header");
        let bytes: Vec<u8> = self
            .rgb
            .iter()
            .flat_map(|p| p.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8))
            .collect();
        writer.write_image_data(&bytes).expect("png data");
    }
}

/// A headless window: runs frames of a UI and paints them.
pub struct Shot {
    pub ctx: Context,
    textures: Textures,
    /// Window size in physical pixels.
    pixels: (usize, usize),
    zoom: f32,
    time: f64,
}

impl Shot {
    /// A window of `width` x `height` pixels in `theme`, at UI scale `zoom` (1 = 100 %).
    pub fn new(width: usize, height: usize, choice: ThemeChoice, zoom: f32) -> Self {
        let ctx = Context::default();
        theme::apply(&ctx, choice);
        ctx.set_zoom_factor(zoom);
        Self {
            ctx,
            textures: Textures::default(),
            pixels: (width, height),
            zoom,
            time: 0.0,
        }
    }

    /// The window's size in points (what layout works in).
    pub fn points(&self) -> Vec2 {
        Vec2::new(self.pixels.0 as f32, self.pixels.1 as f32) / self.zoom
    }

    fn run(&mut self, events: Vec<egui::Event>, ui: &mut dyn FnMut(&mut Ui)) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.points())),
            time: Some(self.time),
            events,
            ..Default::default()
        };
        self.time += 1.0 / 30.0;
        let mut out = self.ctx.run_ui(input, |u| ui(u));
        self.textures.apply(&out.textures_delta);
        out.textures_delta.clear(); // applied: egui insists on being told
        out
    }

    /// Runs one frame without painting it (layout needs a frame or two to settle, and
    /// pointer events take effect on the frame after).
    pub fn step(&mut self, events: Vec<egui::Event>, mut ui: impl FnMut(&mut Ui)) {
        self.run(events, &mut ui);
    }

    /// Lets `seconds` pass (animations, tooltip delays) in frames of `ui`.
    pub fn wait(&mut self, seconds: f64, mut ui: impl FnMut(&mut Ui)) {
        for _ in 0..(seconds * 30.0).ceil() as usize {
            self.run(Vec::new(), &mut ui);
        }
    }

    /// Runs one more frame and paints it.
    pub fn shoot(&mut self, mut ui: impl FnMut(&mut Ui)) -> Canvas {
        let out = self.run(Vec::new(), &mut ui);
        let ppp = out.pixels_per_point;
        let background = self.ctx.global_style().visuals.panel_fill;
        let mut canvas = Canvas::new(self.pixels.0, self.pixels.1, background);
        let primitives = self.ctx.tessellate(out.shapes, ppp);
        canvas.draw(&primitives, &self.textures, ppp);
        canvas
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::{DARK, LIGHT, palette};
    use crate::ui::toast::{Kind, Toasts};
    use crate::ui::widgets::{self, KeySlot, Slider, Tone};
    use crate::ui::{icons, meters};
    use egui::{RichText, pos2, vec2};

    #[test]
    fn the_rasterizer_paints_what_egui_draws() {
        let mut shot = Shot::new(200, 100, ThemeChoice::Dark, 1.0);
        let ui = |ui: &mut Ui| {
            let painter = ui.painter();
            painter.rect_filled(
                Rect::from_min_max(pos2(20.0, 20.0), pos2(80.0, 60.0)),
                0.0,
                DARK.accent,
            );
            // Half-transparent white over the right half of it.
            painter.rect_filled(
                Rect::from_min_max(pos2(50.0, 10.0), pos2(120.0, 70.0)),
                0.0,
                Color32::from_white_alpha(128),
            );
            painter.text(
                pos2(150.0, 50.0),
                egui::Align2::CENTER_CENTER,
                "Hi",
                egui::FontId::proportional(30.0),
                Color32::WHITE,
            );
        };
        shot.step(vec![], ui);
        let canvas = shot.shoot(ui);
        assert_eq!((canvas.width, canvas.height), (200, 100));
        assert_eq!(canvas.pixel(5, 5), DARK.bg, "background");
        assert_eq!(canvas.pixel(30, 40), DARK.accent, "a plain fill");
        // The overlap is the accent with about half white on top; no seam along the diagonal
        // the two triangles of the rectangle share.
        let mixed = canvas.pixel(65, 40);
        let want = |a: u8| (f32::from(a) * (1.0 - 128.0 / 255.0) + 128.0).round() as i32;
        for (got, a) in [
            (mixed.r(), DARK.accent.r()),
            (mixed.g(), DARK.accent.g()),
            (mixed.b(), DARK.accent.b()),
        ] {
            assert!((i32::from(got) - want(a)).abs() <= 2, "{mixed:?}");
        }
        for d in 0..40 {
            let on_diagonal = canvas.pixel(85 + d / 2, 15 + d / 2);
            assert_eq!(on_diagonal, canvas.pixel(100, 15), "seam at {d}");
        }
        // Text: some pixels near the middle of "Hi" are (nearly) white.
        let lit = (130..170)
            .flat_map(|x| (35..65).map(move |y| (x, y)))
            .filter(|&(x, y)| canvas.pixel(x, y).r() > 200)
            .count();
        assert!(lit > 40, "text was painted: {lit} bright pixels");
    }

    #[test]
    fn zoom_scales_the_points_not_the_picture() {
        let mut shot = Shot::new(300, 150, ThemeChoice::Light, 1.5);
        assert_eq!(shot.points(), vec2(200.0, 100.0));
        let ui = |ui: &mut Ui| {
            ui.painter().rect_filled(
                Rect::from_min_max(pos2(100.0, 50.0), pos2(200.0, 100.0)),
                0.0,
                LIGHT.accent,
            );
        };
        shot.step(vec![], ui);
        let canvas = shot.shoot(ui);
        assert_eq!((canvas.width, canvas.height), (300, 150));
        assert_eq!(canvas.pixel(10, 10), LIGHT.bg);
        assert_eq!(canvas.pixel(160, 80), LIGHT.accent, "100 pt is 150 px");
        assert_eq!(canvas.pixel(140, 80), LIGHT.bg);
    }

    /// Every reusable widget in every state, to look at.
    fn gallery(ui: &mut Ui, state: &mut GalleryState) {
        let p = palette(ui);
        egui::CentralPanel::default().show(ui, |ui| {
            ui.columns(2, |cols| {
                let ui = &mut cols[0];
                widgets::card(ui, |ui| {
                    widgets::section_title(ui, icons::MIC, "Sliders and toggles");
                    widgets::hint(
                        ui,
                        "One slider widget everywhere; the tick marks the default.",
                    );
                    ui.horizontal(|ui| {
                        widgets::toggle(ui, &mut state.on);
                        ui.label("On");
                        widgets::toggle(ui, &mut state.off);
                        ui.label("Off");
                        ui.add_enabled_ui(false, |ui| {
                            widgets::toggle(ui, &mut state.on);
                            ui.label("Disabled");
                        });
                    });
                    Slider::new(&mut state.gain, -24.0..=24.0)
                        .label("Input gain")
                        .unit("dB")
                        .decimals(1)
                        .default_value(0.0)
                        .show(ui);
                    Slider::new(&mut state.pitch, -12.0..=12.0)
                        .label("Pitch")
                        .unit("st")
                        .decimals(1)
                        .step(0.1)
                        .default_value(0.0)
                        .show(ui);
                    Slider::new(&mut state.mix, 0.0..=1.0)
                        .label("Mix")
                        .percent(true)
                        .default_value(0.3)
                        .show(ui);
                    Slider::new(&mut state.cut, 20.0..=20_000.0)
                        .label("High cut")
                        .unit("Hz")
                        .logarithmic(true)
                        .default_value(20_000.0)
                        .show(ui);
                    ui.add_enabled_ui(false, |ui| {
                        Slider::new(&mut state.mix, 0.0..=1.0)
                            .label("Disabled")
                            .percent(true)
                            .show(ui);
                    });
                });
                ui.add_space(theme::space::M);
                widgets::card(ui, |ui| {
                    widgets::section_title(ui, icons::KEYBOARD, "Hotkeys");
                    let combo = ["Ctrl".to_string(), "Shift".to_string(), "F8".to_string()];
                    let num = ["Num 7".to_string()];
                    ui.horizontal(|ui| {
                        widgets::keycaps(ui, &combo);
                        widgets::keycaps(ui, &num);
                    });
                    ui.horizontal(|ui| {
                        widgets::hotkey_button(ui, KeySlot::Set(&combo));
                        widgets::hotkey_button(ui, KeySlot::Empty);
                        widgets::hotkey_button(ui, KeySlot::Capturing);
                    });
                });
                let ui = &mut cols[1];
                widgets::card(ui, |ui| {
                    widgets::section_title(ui, icons::LATENCY, "Meters");
                    for (level, marker) in [(0.02, None), (0.2, Some(-40.0)), (0.9, None)] {
                        ui.horizontal(|ui| {
                            meters::level_meter(ui, level, vec2(200.0, 10.0), -60.0, marker);
                            meters::clip_lamp(ui, level > 0.8);
                        });
                    }
                });
                ui.add_space(theme::space::M);
                widgets::card(ui, |ui| {
                    widgets::section_title(ui, "", "Chips, badges and buttons");
                    ui.horizontal_wrapped(|ui| {
                        widgets::chip(ui, icons::WARNING, "VB-CABLE not found", Tone::Danger);
                        widgets::chip(ui, icons::MIC, "Using fallback mic", Tone::Warning);
                        widgets::chip(ui, icons::CHECK_CIRCLE, "All good", Tone::Success);
                        widgets::chip(ui, "", "Neutral", Tone::Neutral);
                        widgets::chip(ui, icons::INFO, "Accent", Tone::Accent);
                    });
                    ui.horizontal(|ui| {
                        widgets::badge(ui, "HOLD", Tone::Accent);
                        widgets::badge(ui, "LOOP", Tone::Neutral);
                        widgets::badge(ui, "MISSING", Tone::Danger);
                    });
                    ui.horizontal(|ui| {
                        widgets::primary_button(ui, "Primary");
                        widgets::danger_button(ui, "Stop all");
                        let _ = ui.button("Secondary");
                        widgets::icon_button(ui, icons::CLOSE, "Close");
                        ui.add_enabled_ui(false, |ui| {
                            widgets::primary_button(ui, "Disabled");
                            let _ = ui.button("Disabled");
                        });
                    });
                    ui.label(RichText::new("Muted text").color(p.text_muted));
                    ui.label(RichText::new("Faint text").color(p.text_faint));
                    ui.hyperlink_to("A link", "https://example.com");
                });
            });
        });
        state.toasts.show(ui.ctx());
    }

    struct GalleryState {
        on: bool,
        off: bool,
        gain: f32,
        pitch: f32,
        mix: f32,
        cut: f32,
        toasts: Toasts,
    }

    #[test]
    #[ignore = "writes pictures to VOXSHIFT_SHOTS; run with --ignored"]
    fn shots_of_the_widget_gallery() {
        let Some(dir) = out_dir() else {
            eprintln!("VOXSHIFT_SHOTS is not set: nothing to do");
            return;
        };
        for (name, choice, zoom) in [
            ("gallery-dark", ThemeChoice::Dark, 1.0),
            ("gallery-light", ThemeChoice::Light, 1.0),
            ("gallery-dark-150", ThemeChoice::Dark, 1.5),
        ] {
            let mut shot = Shot::new(1100, 620, choice, zoom);
            let mut state = GalleryState {
                on: true,
                off: false,
                gain: 6.0,
                pitch: -5.0,
                mix: 0.65,
                cut: 4500.0,
                toasts: Toasts::default(),
            };
            shot.step(vec![], |ui| gallery(ui, &mut state));
            state.toasts.push(&shot.ctx, "Switched to the primary mic");
            state.toasts.success(&shot.ctx, "Saved preset \"My robot\"");
            state
                .toasts
                .error(&shot.ctx, "Could not import: the file is not a .voxpack");
            state
                .toasts
                .push_action(&shot.ctx, Kind::Info, "Deleted \"airhorn\"", "Undo", 1);
            shot.wait(0.5, |ui| gallery(ui, &mut state));
            let canvas = shot.shoot(|ui| gallery(ui, &mut state));
            canvas.save_png(&dir.join(format!("{name}.png")));
        }
    }

    #[test]
    #[ignore = "writes pictures to VOXSHIFT_SHOTS; run with --ignored"]
    fn shots_of_the_app() {
        let Some(dir) = out_dir() else {
            eprintln!("VOXSHIFT_SHOTS is not set: nothing to do");
            return;
        };
        // Every page at the smallest size it must look good at.
        for page in Page::ALL {
            let name = format!("{page:?}-1280x720").to_lowercase();
            app_shot(
                &dir,
                &name,
                (1280, 720),
                ThemeChoice::Dark,
                1.0,
                |app, _| {
                    app.cfg.page = page;
                },
            );
        }
        let voice = |app: &mut VoxApp, _: &Context| app.cfg.page = Page::Voice;
        app_shot(
            &dir,
            "voice-2560x1080",
            (2560, 1080),
            ThemeChoice::Dark,
            1.0,
            voice,
        );
        app_shot(
            &dir,
            "voice-920x600",
            (920, 600),
            ThemeChoice::Dark,
            1.0,
            voice,
        );
        app_shot(
            &dir,
            "voice-1280x720-150",
            (1280, 720),
            ThemeChoice::Dark,
            1.5,
            voice,
        );
        app_shot(
            &dir,
            "voice-light",
            (1280, 720),
            ThemeChoice::Light,
            1.0,
            voice,
        );
        app_shot(
            &dir,
            "voice-off",
            (1280, 720),
            ThemeChoice::Dark,
            1.0,
            |app, _| {
                app.shared.voice_on.store(false, Relaxed);
            },
        );
        app_shot(
            &dir,
            "safe-mode",
            (1280, 720),
            ThemeChoice::Dark,
            1.0,
            |app, _| {
                app.feedback.set_safe(true);
            },
        );
        // The state from before: no VB-CABLE, on the fallback mic, muted.
        app_shot(
            &dir,
            "problems",
            (1280, 720),
            ThemeChoice::Dark,
            1.0,
            |app, _| {
                app.manager = ManagerHandle::detached(crate::ui::tests::fake_status());
                app.shared.mic_muted.store(true, Relaxed);
            },
        );
        // Melody Voice singing, own presets (one a favourite), a modified preset.
        app_shot(
            &dir,
            "voice-melody-on",
            (1280, 1500),
            ThemeChoice::Dark,
            1.0,
            |app, ctx| {
                use crate::dsp::params::P;
                use crate::ui::presets::Action;
                for name in ["Stream voice", "Late night"] {
                    app.apply_preset_action(ctx, Action::Save(name.into()));
                }
                app.apply_preset_action(ctx, Action::Favorite("Late night".into(), true));
                app.fx.set(P::MelodyOn, 1.0);
                app.fx.set(P::PitchOn, 1.0);
                app.fx.set(P::PitchSemis, -3.0);
                app.effects_changed();
                app.shared.melody_note.store(9, Relaxed);
                app.shared.melody_beat.set(9.4);
                app.shared.melody_detected.set(57.2);
                app.shared.melody_target.set(64.0);
            },
        );
    }

    use crate::audio::device_manager::{DeviceStatus, Endpoint, LinkState, ManagerHandle};
    use crate::audio::shared::{ActiveMic, Shared};
    use crate::config::{Config, Page};
    use crate::ui::VoxApp;
    use std::sync::Arc;
    use std::sync::atomic::Ordering::Relaxed;

    /// A setup where everything works: VB-CABLE, a primary and a fallback mic, a monitor.
    fn healthy_status() -> DeviceStatus {
        let running = |name: &str| Endpoint {
            device: Some(name.to_string()),
            state: LinkState::Running,
            rate: 48_000,
            period: 480,
        };
        let cable = "CABLE Input (VB-Audio Virtual Cable)";
        let (primary, fallback) = (
            "Microphone (HyperX Cloud II Wireless)",
            "Microphone (USB PnP Audio Device)",
        );
        let headphones = "Speakers (HyperX Cloud II Wireless)";
        DeviceStatus {
            scanned: true,
            inputs: vec![primary.into(), fallback.into()],
            outputs: vec![cable.into(), headphones.into()],
            default_input: Some(primary.into()),
            cable: Some(cable.into()),
            output: running(cable),
            monitor: running(headphones),
            mics: [running(primary), running(fallback)],
            active: ActiveMic::Primary,
            reason: String::new(),
            latency_ms: Some(38.0),
        }
    }

    /// An app that looks used: devices set, a preset loaded, some clips, levels on the meters.
    fn demo_app(ctx: &Context) -> VoxApp {
        let status = healthy_status();
        let mut cfg = Config::default();
        cfg.devices.output = status.cable.clone().unwrap();
        cfg.devices.primary = status.inputs[0].clone();
        cfg.devices.fallback = status.inputs[1].clone();
        cfg.devices.monitor = status.outputs[1].clone();
        cfg.devices.monitor_enabled = true;
        cfg.voice_keys.toggle = Some(crate::soundboard::hotkeys::Hotkey::new(
            0x77, true, true, false, false, "",
        ));
        cfg.voice_keys.panic = Some(crate::soundboard::hotkeys::Hotkey::new(
            0x67,
            false,
            false,
            false,
            false,
            "7 (PAVE NUM.)",
        ));
        let mut app = VoxApp::with_manager(
            cfg,
            Arc::new(Shared::default()),
            ManagerHandle::detached(status),
        );
        let cave = crate::config::presets::builtin()
            .into_iter()
            .find(|p| p.name == "Cave")
            .unwrap();
        app.apply_preset(ctx, &cave);
        app.add_sounds(
            ctx,
            [
                "airhorn.mp3",
                "freesound_community-180610-house-fly-close-buzzing-62408.mp3",
                "bruh.wav",
                "sad trombone.ogg",
                "Windows XP shutdown.wav",
                "vine_boom_sound_effect.mp3",
                "crickets.flac",
                "rimshot.wav",
            ]
            .map(|f| format!("C:/sounds/{f}").into())
            .to_vec(),
        );
        app.cfg.page = Page::Voice;
        app.shared.input_level.set(0.12);
        app.shared.output_level.set(0.3);
        app.shared.mic_level[0].set(0.1);
        app.shared.mic_level[1].set(0.002);
        app
    }

    /// One picture of the app at `size` pixels, after `setup` put it in the state to show.
    fn app_shot(
        dir: &Path,
        name: &str,
        size: (usize, usize),
        choice: ThemeChoice,
        zoom: f32,
        setup: impl FnOnce(&mut VoxApp, &Context),
    ) {
        let mut shot = Shot::new(size.0, size.1, choice, zoom);
        let mut app = demo_app(&shot.ctx);
        setup(&mut app, &shot.ctx);
        let mut frame = |ui: &mut Ui| {
            app.tick(ui.ctx());
            app.draw(ui);
        };
        // (The toasts from setting the demo up are gone after a few seconds.)
        shot.wait(9.0, &mut frame);
        shot.shoot(&mut frame)
            .save_png(&dir.join(format!("{name}.png")));
    }
}
