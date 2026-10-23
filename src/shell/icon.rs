//! The app icon, drawn in code: a coloured disc with a white microphone. The colour tells the
//! state at a glance (used for the tray icon and the window icon).

use super::Health;

/// RGBA colour of the disc for each state.
pub fn disc_color(h: Health) -> [u8; 3] {
    match h {
        Health::Ok => [130, 90, 240],
        Health::VoiceOff | Health::Muted => [110, 110, 118],
        Health::NoMic => [235, 150, 40],
        Health::NoOutput => [220, 50, 50],
    }
}

const SLASH: [u8; 3] = [235, 55, 55];

/// Anti-aliased coverage of a shape from its signed distance (negative = inside), in pixels.
fn cover(signed_dist: f32) -> f32 {
    (0.5 - signed_dist).clamp(0.0, 1.0)
}

/// Distance from p to the segment a-b.
fn seg_dist(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy).max(1e-6)).clamp(0.0, 1.0);
    let (cx, cy) = (a.0 + t * dx - p.0, a.1 + t * dy - p.1);
    (cx * cx + cy * cy).sqrt()
}

/// Signed distance to an axis-aligned box given by centre and half-size.
fn box_dist(p: (f32, f32), c: (f32, f32), half: (f32, f32)) -> f32 {
    let dx = (p.0 - c.0).abs() - half.0;
    let dy = (p.1 - c.1).abs() - half.1;
    let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt();
    outside + dx.max(dy).min(0.0)
}

/// Coverage of the white microphone glyph at pixel centre p (all sizes relative to `n`).
fn mic(p: (f32, f32), n: f32) -> f32 {
    let c = n / 2.0;
    // Capsule (the mic head).
    let w = 0.12 * n;
    let head = seg_dist(p, (c, 0.26 * n), (c, 0.47 * n)) - w;
    // U-shaped holder: lower half of a ring around the head.
    let ring_c = (c, 0.47 * n);
    let r = ((p.0 - ring_c.0).powi(2) + (p.1 - ring_c.1).powi(2)).sqrt();
    let ring = (r - 0.21 * n).abs() - 0.035 * n;
    let ring = if p.1 >= ring_c.1 { ring } else { f32::MAX };
    // Stem and base.
    let stem = box_dist(p, (c, 0.73 * n), (0.035 * n, 0.06 * n));
    let base = box_dist(p, (c, 0.79 * n), (0.15 * n, 0.03 * n));
    cover(head.min(ring).min(stem).min(base))
}

/// Renders the icon as straight (non-premultiplied) RGBA, `size` x `size`.
pub fn render(h: Health, size: u32) -> Vec<u8> {
    let n = size as f32;
    let mut out = vec![0u8; (size * size * 4) as usize];
    let color = disc_color(h);
    for y in 0..size {
        for x in 0..size {
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            let d = ((p.0 - n / 2.0).powi(2) + (p.1 - n / 2.0).powi(2)).sqrt();
            let disc = cover(d - 0.47 * n);
            if disc <= 0.0 {
                continue;
            }
            let mut rgb = color.map(f32::from);
            let m = mic(p, n);
            for (v, white) in rgb.iter_mut().zip([255.0; 3]) {
                *v += (white - *v) * m;
            }
            if h == Health::Muted {
                let s = cover(seg_dist(p, (0.22 * n, 0.22 * n), (0.78 * n, 0.78 * n)) - 0.06 * n);
                for (v, red) in rgb.iter_mut().zip(SLASH.map(f32::from)) {
                    *v += (red - *v) * s;
                }
            }
            let i = ((y * size + x) * 4) as usize;
            out[i] = rgb[0] as u8;
            out[i + 1] = rgb[1] as u8;
            out[i + 2] = rgb[2] as u8;
            out[i + 3] = (disc * 255.0) as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(img: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * size + x) * 4) as usize;
        [img[i], img[i + 1], img[i + 2], img[i + 3]]
    }

    #[test]
    fn corners_are_transparent_and_the_disc_has_the_state_colour() {
        for h in [
            Health::Ok,
            Health::VoiceOff,
            Health::NoMic,
            Health::NoOutput,
        ] {
            let img = render(h, 32);
            assert_eq!(img.len(), 32 * 32 * 4);
            assert_eq!(px(&img, 32, 0, 0)[3], 0);
            // Left edge of the disc, away from the white glyph.
            let p = px(&img, 32, 3, 16);
            assert_eq!(p[3], 255);
            assert_eq!(&p[..3], &disc_color(h));
        }
    }

    #[test]
    fn glyph_is_white_and_muted_has_a_red_slash() {
        let img = render(Health::Ok, 64);
        assert_eq!(&px(&img, 64, 32, 22)[..3], &[255, 255, 255], "mic head");
        let muted = render(Health::Muted, 64);
        let p = px(&muted, 64, 18, 18);
        assert!(p[0] > 200 && p[1] < 100, "slash: {p:?}");
    }
}
