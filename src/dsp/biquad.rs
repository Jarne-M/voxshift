//! Second-order IIR filters, coefficients from Robert Bristow-Johnson's "Audio EQ Cookbook".
//! Implemented as transposed direct form II, which behaves well when coefficients change.

use std::f32::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coeffs {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

/// Shared cookbook intermediates for frequency `f` at sample rate `fs`.
fn prep(fs: f32, f: f32, q: f32) -> (f32, f32, f32) {
    let f = f.clamp(10.0, fs * 0.45);
    let w0 = 2.0 * PI * f / fs;
    let (sin, cos) = w0.sin_cos();
    (cos, sin, sin / (2.0 * q.max(0.05)))
}

impl Coeffs {
    pub const IDENTITY: Self = Self {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    };

    fn normalized(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> Self {
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    pub fn lowpass(fs: f32, f: f32, q: f32) -> Self {
        let (cos, _, alpha) = prep(fs, f, q);
        let b1 = 1.0 - cos;
        Self::normalized(b1 / 2.0, b1, b1 / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha)
    }

    pub fn highpass(fs: f32, f: f32, q: f32) -> Self {
        let (cos, _, alpha) = prep(fs, f, q);
        let b1 = -(1.0 + cos);
        Self::normalized(
            -b1 / 2.0,
            b1,
            -b1 / 2.0,
            1.0 + alpha,
            -2.0 * cos,
            1.0 - alpha,
        )
    }

    pub fn peaking(fs: f32, f: f32, q: f32, gain_db: f32) -> Self {
        let (cos, _, alpha) = prep(fs, f, q);
        let a = 10f32.powf(gain_db / 40.0);
        Self::normalized(
            1.0 + alpha * a,
            -2.0 * cos,
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * cos,
            1.0 - alpha / a,
        )
    }

    /// Shelf with slope 1 (Q = 1/sqrt 2).
    pub fn low_shelf(fs: f32, f: f32, gain_db: f32) -> Self {
        let (cos, _, alpha) = prep(fs, f, std::f32::consts::FRAC_1_SQRT_2);
        let a = 10f32.powf(gain_db / 40.0);
        let sa = 2.0 * a.sqrt() * alpha;
        Self::normalized(
            a * ((a + 1.0) - (a - 1.0) * cos + sa),
            2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
            a * ((a + 1.0) - (a - 1.0) * cos - sa),
            (a + 1.0) + (a - 1.0) * cos + sa,
            -2.0 * ((a - 1.0) + (a + 1.0) * cos),
            (a + 1.0) + (a - 1.0) * cos - sa,
        )
    }

    pub fn high_shelf(fs: f32, f: f32, gain_db: f32) -> Self {
        let (cos, _, alpha) = prep(fs, f, std::f32::consts::FRAC_1_SQRT_2);
        let a = 10f32.powf(gain_db / 40.0);
        let sa = 2.0 * a.sqrt() * alpha;
        Self::normalized(
            a * ((a + 1.0) + (a - 1.0) * cos + sa),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
            a * ((a + 1.0) + (a - 1.0) * cos - sa),
            (a + 1.0) - (a - 1.0) * cos + sa,
            2.0 * ((a - 1.0) - (a + 1.0) * cos),
            (a + 1.0) - (a - 1.0) * cos - sa,
        )
    }

    /// |H| at frequency `f`.
    #[cfg(test)]
    pub fn magnitude(&self, fs: f32, f: f32) -> f32 {
        let w = 2.0 * PI * f / fs;
        // H(z) with z = e^{jw}; evaluate numerator and denominator as complex numbers.
        let (c1, s1) = (w.cos(), -w.sin());
        let (c2, s2) = ((2.0 * w).cos(), -(2.0 * w).sin());
        let nr = self.b0 + self.b1 * c1 + self.b2 * c2;
        let ni = self.b1 * s1 + self.b2 * s2;
        let dr = 1.0 + self.a1 * c1 + self.a2 * c2;
        let di = self.a1 * s1 + self.a2 * s2;
        ((nr * nr + ni * ni) / (dr * dr + di * di)).sqrt()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    pub c: Coeffs,
    z1: f32,
    z2: f32,
}

impl Default for Biquad {
    fn default() -> Self {
        Self {
            c: Coeffs::IDENTITY,
            z1: 0.0,
            z2: 0.0,
        }
    }
}

impl Biquad {
    pub fn new(c: Coeffs) -> Self {
        Self {
            c,
            ..Default::default()
        }
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let c = &self.c;
        let y = c.b0 * x + self.z1;
        self.z1 = c.b1 * x - c.a1 * y + self.z2;
        self.z2 = c.b2 * x - c.a2 * y;
        y
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    pub fn db(x: f32) -> f32 {
        20.0 * x.log10()
    }

    /// Measured gain of a filter on a steady sine (after the transient).
    pub fn measure(mut f: impl FnMut(f32) -> f32, freq: f32) -> f32 {
        let n = 48_000;
        let mut peak = 0.0f32;
        for i in 0..n {
            let x = (2.0 * PI * freq * i as f32 / FS).sin();
            let y = f(x);
            if i > n / 2 {
                peak = peak.max(y.abs());
            }
        }
        peak
    }

    #[test]
    fn lowpass_and_highpass_cut_the_right_side() {
        let lp = Coeffs::lowpass(FS, 1000.0, 0.707);
        assert!((db(lp.magnitude(FS, 100.0))).abs() < 0.1);
        assert!(
            (db(lp.magnitude(FS, 1000.0)) + 3.0).abs() < 0.2,
            "-3 dB at cutoff"
        );
        assert!(db(lp.magnitude(FS, 10_000.0)) < -35.0);
        let hp = Coeffs::highpass(FS, 1000.0, 0.707);
        assert!(db(hp.magnitude(FS, 100.0)) < -35.0);
        assert!((db(hp.magnitude(FS, 10_000.0))).abs() < 0.1);
    }

    #[test]
    fn peaking_and_shelves_hit_their_gain() {
        let pk = Coeffs::peaking(FS, 1000.0, 1.0, 6.0);
        assert!((db(pk.magnitude(FS, 1000.0)) - 6.0).abs() < 0.05);
        assert!(db(pk.magnitude(FS, 50.0)).abs() < 0.2);
        let ls = Coeffs::low_shelf(FS, 200.0, -9.0);
        assert!((db(ls.magnitude(FS, 20.0)) + 9.0).abs() < 0.3);
        assert!(db(ls.magnitude(FS, 5000.0)).abs() < 0.2);
        let hs = Coeffs::high_shelf(FS, 4000.0, 9.0);
        assert!((db(hs.magnitude(FS, 18_000.0)) - 9.0).abs() < 0.5);
        assert!(db(hs.magnitude(FS, 100.0)).abs() < 0.2);
    }

    #[test]
    fn time_domain_matches_the_magnitude_response() {
        let c = Coeffs::peaking(FS, 2000.0, 0.9, -8.0);
        let mut f = Biquad {
            c,
            ..Default::default()
        };
        let measured = measure(|x| f.process(x), 2000.0);
        assert!((db(measured) - db(c.magnitude(FS, 2000.0))).abs() < 0.1);
    }
}
