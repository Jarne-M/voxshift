//! Short beeps for the headphones when the voice changer switches: two rising tones for ON,
//! two falling tones for OFF. Each tone fades in and out so it never clicks.

const TONE_S: f32 = 0.07;
const GAP_S: f32 = 0.025;
const FADE_S: f32 = 0.008;
const LOW_HZ: f32 = 660.0;
const HIGH_HZ: f32 = 990.0;

#[derive(Clone, Debug)]
pub struct Cue {
    fs: f32,
    /// Samples into the cue; `None` = silent.
    pos: Option<usize>,
    on: bool,
    phase: f32,
}

impl Cue {
    pub fn new(fs: f32) -> Self {
        Self {
            fs,
            pos: None,
            on: true,
            phase: 0.0,
        }
    }

    pub fn start(&mut self, on: bool) {
        self.on = on;
        self.pos = Some(0);
        self.phase = 0.0;
    }

    pub fn active(&self) -> bool {
        self.pos.is_some()
    }

    fn len(&self) -> usize {
        ((2.0 * TONE_S + GAP_S) * self.fs) as usize
    }

    /// Next sample (mono, peak 1.0).
    #[inline]
    pub fn next(&mut self) -> f32 {
        let Some(pos) = self.pos else { return 0.0 };
        if pos >= self.len() {
            self.pos = None;
            return 0.0;
        }
        self.pos = Some(pos + 1);
        let t = pos as f32 / self.fs;
        let (tone_t, first) = if t < TONE_S {
            (t, true)
        } else if t < TONE_S + GAP_S {
            return 0.0;
        } else {
            (t - TONE_S - GAP_S, false)
        };
        let freq = match (self.on, first) {
            (true, true) | (false, false) => LOW_HZ,
            _ => HIGH_HZ,
        };
        self.phase = (self.phase + freq / self.fs).fract();
        let env = (tone_t / FADE_S)
            .min((TONE_S - tone_t) / FADE_S)
            .clamp(0.0, 1.0);
        (std::f32::consts::TAU * self.phase).sin() * env
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::pitch::tests::frequency;

    fn render(on: bool) -> Vec<f32> {
        let mut c = Cue::new(48_000.0);
        c.start(on);
        let mut out = Vec::new();
        while c.active() {
            out.push(c.next());
        }
        out
    }

    #[test]
    fn on_rises_off_falls_and_both_end_cleanly() {
        let on = render(true);
        let off = render(false);
        assert!((on.len() as f32 / 48_000.0 - 0.165).abs() < 0.01);
        let half = |v: &[f32], first: bool| {
            let n = (TONE_S * 48_000.0) as usize;
            let part = if first { &v[..n] } else { &v[v.len() - n..] };
            frequency(&[part, part].concat(), 48_000.0)
        };
        assert!(half(&on, true) < half(&on, false), "ON goes up");
        assert!(half(&off, true) > half(&off, false), "OFF goes down");
        for v in [&on, &off] {
            assert!(
                v[0].abs() < 0.01 && v.last().unwrap().abs() < 0.05,
                "no clicks"
            );
            assert!(v.iter().all(|x| x.abs() <= 1.0));
            let step = v
                .windows(2)
                .map(|w| (w[1] - w[0]).abs())
                .fold(0.0, f32::max);
            assert!(step < 0.2, "smooth: {step}");
        }
    }
}
