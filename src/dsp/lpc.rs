//! Linear prediction (LPC) for formant preservation.
//!
//! A voice is roughly a buzz (the vocal folds: pitch) through a filter (the throat and mouth:
//! formants, which make vowels sound like vowels). LPC estimates that filter from the last
//! ~20 ms of audio. Running the audio through the *inverse* filter "whitens" it, leaving the
//! buzz (the residual). We pitch-shift only the residual and then put the original filter back,
//! so the pitch moves but the formants stay: no chipmunk or monster timbre.
//!
//! The filters are lattice filters driven by reflection coefficients `k` from the
//! Levinson-Durbin recursion. With |k| < 1 the synthesis filter is guaranteed stable, and the
//! coefficients can be interpolated between analysis frames without breaking that.
//!
//! Formant *shifting* (Melody Voice): the colouring filter can come from a second analysis of
//! the same history read `warp` times faster (or slower). A frame squeezed in time has every
//! frequency, and so every formant, `warp` times higher. Whiten with the real envelope, colour
//! with the squeezed one, and the formants move without any extra delay.

pub const ORDER: usize = 20;
/// Analysis window and hop, in seconds.
const WINDOW_S: f32 = 0.025;
const HOP_S: f32 = 0.005;
const MAX_K: f32 = 0.995;
/// Largest formant warp the history is sized for (+6 semitones).
pub const MAX_WARP: f32 = 1.42;

/// Autocorrelation -> reflection coefficients. Returns the prediction error energy.
pub fn levinson(r: &[f32; ORDER + 1], k: &mut [f32; ORDER]) -> f32 {
    let mut a = [0.0f32; ORDER + 1];
    let mut tmp = [0.0f32; ORDER + 1];
    a[0] = 1.0;
    let mut err = r[0];
    for i in 1..=ORDER {
        if err <= r[0] * 1e-9 {
            k[i - 1..].fill(0.0);
            break;
        }
        let mut acc = r[i];
        for j in 1..i {
            acc += a[j] * r[i - j];
        }
        let ki = (-acc / err).clamp(-MAX_K, MAX_K);
        k[i - 1] = ki;
        tmp[..=i].copy_from_slice(&a[..=i]);
        for j in 1..i {
            a[j] = tmp[j] + ki * tmp[i - j];
        }
        a[i] = ki;
        err *= 1.0 - ki * ki;
    }
    err
}

/// Analysis (whitening) lattice: x -> residual.
#[derive(Clone, Debug, Default)]
pub struct Whitener {
    b: [f32; ORDER],
}

impl Whitener {
    #[inline]
    pub fn process(&mut self, x: f32, k: &[f32; ORDER]) -> f32 {
        let mut f = x;
        let mut b_in = x; // b_0[n]
        for (b, &ki) in self.b.iter_mut().zip(k.iter()) {
            let b_prev = *b; // b_i[n-1]
            *b = b_in;
            let f_next = f + ki * b_prev;
            b_in = b_prev + ki * f;
            f = f_next;
        }
        f
    }
}

/// Synthesis (colouring) lattice: residual -> signal with the formants back.
#[derive(Clone, Debug, Default)]
pub struct Colorer {
    b: [f32; ORDER + 1],
}

impl Colorer {
    #[inline]
    pub fn process(&mut self, e: f32, k: &[f32; ORDER]) -> f32 {
        let mut f = e;
        for i in (0..ORDER).rev() {
            f -= k[i] * self.b[i];
            self.b[i + 1] = self.b[i] + k[i] * f;
        }
        self.b[0] = f;
        // Safety net against denormals/blow-ups on silence.
        if !f.is_finite() {
            self.b = [0.0; ORDER + 1];
            return 0.0;
        }
        f
    }
}

/// Tracks the spectral envelope of the input and provides interpolated coefficients.
pub struct Lpc {
    hist: Vec<f32>,
    pos: usize,
    window: Vec<f32>,
    frame: Vec<f32>,
    hop: usize,
    until_next: usize,
    k: [f32; ORDER],
    k_step: [f32; ORDER],
    /// Colouring coefficients (the warped envelope; equal to `k` when `warp` is 1).
    kc: [f32; ORDER],
    kc_step: [f32; ORDER],
    warp: f32,
    fs: f32,
}

impl Lpc {
    pub fn new(fs: f32) -> Self {
        let n = (WINDOW_S * fs) as usize;
        let window = (0..n)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos())
            .collect();
        let hop = ((HOP_S * fs) as usize).max(1);
        Self {
            hist: vec![0.0; (n as f32 * MAX_WARP).ceil() as usize + 2],
            pos: 0,
            window,
            frame: vec![0.0; n],
            hop,
            until_next: hop,
            k: [0.0; ORDER],
            k_step: [0.0; ORDER],
            kc: [0.0; ORDER],
            kc_step: [0.0; ORDER],
            warp: 1.0,
            fs,
        }
    }

    /// Formant shift factor for [`colour`](Self::colour) (1 = none), from the next analysis on.
    pub fn set_warp(&mut self, warp: f32) {
        self.warp = warp.clamp(1.0 / MAX_WARP, MAX_WARP);
    }

    /// Feeds one input sample; returns the coefficients to use for it.
    #[inline]
    pub fn push(&mut self, x: f32) -> &[f32; ORDER] {
        self.hist[self.pos] = x;
        self.pos = (self.pos + 1) % self.hist.len();
        for (k, s) in self.k.iter_mut().zip(self.k_step.iter()) {
            *k += s;
        }
        for (k, s) in self.kc.iter_mut().zip(self.kc_step.iter()) {
            *k += s;
        }
        self.until_next -= 1;
        if self.until_next == 0 {
            self.until_next = self.hop;
            self.analyze();
        }
        &self.k
    }

    /// Whitening and colouring coefficients for the sample just pushed (they differ only
    /// while a formant warp is set).
    #[inline]
    pub fn coeffs(&self) -> (&[f32; ORDER], &[f32; ORDER]) {
        (&self.k, &self.kc)
    }

    /// Fills the frame with the last window of history read `warp` times faster, windowed.
    fn fill_frame(&mut self, warp: f32) {
        let (n, h) = (self.frame.len(), self.hist.len());
        let newest = self.pos + h - 1;
        for i in 0..n {
            let age = (n - 1 - i) as f32 * warp;
            let a = age as usize;
            let frac = age - a as f32;
            let s0 = self.hist[(newest - a) % h];
            let s1 = self.hist[(newest - a - 1) % h];
            self.frame[i] = (s0 + (s1 - s0) * frac) * self.window[i];
        }
    }

    /// LPC of the current frame into `target`; false on silence.
    fn envelope(&self, target: &mut [f32; ORDER]) -> bool {
        let n = self.frame.len();
        let mut r = [0.0f32; ORDER + 1];
        for (lag, r) in r.iter_mut().enumerate() {
            let mut acc = 0.0;
            for i in lag..n {
                acc += self.frame[i] * self.frame[i - lag];
            }
            *r = acc;
        }
        if r[0] < 1e-9 {
            return false;
        }
        // A white-noise floor (-25 dB) and a Gaussian lag window (~100 Hz bandwidth expansion)
        // keep the estimate well-conditioned: without them a steady tone yields razor-sharp
        // resonances with enormous gain.
        r[0] *= 1.003;
        let bw = 2.0 * std::f32::consts::PI * 100.0 / self.fs;
        for (lag, v) in r.iter_mut().enumerate().skip(1) {
            let g = bw * lag as f32;
            *v *= (-0.5 * g * g).exp();
        }
        levinson(&r, target);
        true
    }

    fn analyze(&mut self) {
        let hop = self.hop as f32;
        let glide = |steps: &mut [f32; ORDER], from: &[f32; ORDER], to: Option<&[f32; ORDER]>| {
            for i in 0..ORDER {
                // Silence: glide toward a flat (neutral) filter.
                steps[i] = (to.map_or(0.0, |t| t[i]) - from[i]) / hop;
            }
        };
        let mut target = [0.0; ORDER];
        self.fill_frame(1.0);
        let voiced = self.envelope(&mut target);
        glide(&mut self.k_step, &self.k, voiced.then_some(&target));
        if self.warp != 1.0 {
            self.fill_frame(self.warp);
            let voiced = self.envelope(&mut target);
            glide(&mut self.kc_step, &self.kc, voiced.then_some(&target));
        } else {
            glide(&mut self.kc_step, &self.kc, voiced.then_some(&target));
        }
    }

    pub fn reset(&mut self) {
        self.hist.fill(0.0);
        self.k = [0.0; ORDER];
        self.k_step = [0.0; ORDER];
        self.kc = [0.0; ORDER];
        self.kc_step = [0.0; ORDER];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pseudo-random noise.
    fn noise(n: usize) -> Vec<f32> {
        let mut s = 12345u32;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                (s >> 8) as f32 / (1u32 << 24) as f32 - 0.5
            })
            .collect()
    }

    #[test]
    fn lattice_whitening_then_colouring_is_lossless() {
        let mut k = [0.0; ORDER];
        for (i, v) in k.iter_mut().enumerate() {
            *v = 0.6 * (-(i as f32) * 0.3).exp() * if i % 2 == 0 { 1.0 } else { -1.0 };
        }
        let (mut w, mut c) = (Whitener::default(), Colorer::default());
        for (i, x) in noise(2000).into_iter().enumerate() {
            let y = c.process(w.process(x, &k), &k);
            assert!((y - x).abs() < 1e-4, "sample {i}: {x} -> {y}");
        }
    }

    #[test]
    fn whitening_flattens_a_resonant_signal() {
        // Noise through an all-pole resonance (a "formant" at 1 kHz), the kind of signal LPC
        // models exactly.
        let fs = 48_000.0;
        let (r, w) = (0.995f32, 2.0 * std::f32::consts::PI * 1000.0 / fs);
        let (mut y1, mut y2) = (0.0f32, 0.0f32);
        let input: Vec<f32> = noise(48_000)
            .into_iter()
            .map(|e| {
                let y = 0.05 * e + 2.0 * r * w.cos() * y1 - r * r * y2;
                y2 = y1;
                y1 = y;
                y
            })
            .collect();
        let mut lpc = Lpc::new(fs);
        let mut w = Whitener::default();
        let (mut e_in, mut e_out) = (0.0f64, 0.0f64);
        for (i, &x) in input.iter().enumerate() {
            let k = *lpc.push(x);
            let e = w.process(x, &k);
            if i > 4800 {
                e_in += f64::from(x * x);
                e_out += f64::from(e * e);
            }
        }
        let gain_db = 10.0 * (e_out / e_in).log10();
        assert!(gain_db < -15.0, "prediction gain only {gain_db} dB");
    }

    #[test]
    fn levinson_reflection_coefficients_stay_inside_the_unit_circle() {
        let mut r = [0.0; ORDER + 1];
        for (i, v) in r.iter_mut().enumerate() {
            *v = 0.99f32.powi(i as i32); // nearly singular
        }
        let mut k = [0.0; ORDER];
        levinson(&r, &mut k);
        assert!(k.iter().all(|k| k.abs() <= MAX_K));
    }
}
