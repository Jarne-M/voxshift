//! Tone: low cut and high cut (24 dB/octave each, for radio/telephone sounds) plus a 3-band EQ
//! (low shelf 200 Hz, mid peak, high shelf 4 kHz).

use super::Block;
use super::biquad::{Biquad, Coeffs};
use super::params::{P, Snapshot};
use super::smooth::Smoother;
use crate::audio::block::BLOCK;

const BUTTERWORTH_Q: [f32; 2] = [0.541, 1.307]; // two stages = 4th-order Butterworth
const MID_Q: f32 = 0.9;
const LOW_SHELF_HZ: f32 = 200.0;
const HIGH_SHELF_HZ: f32 = 4000.0;

pub struct Tone {
    fs: f32,
    filters: [Biquad; 7],
    // Smoothed controls: low cut, high cut, low, mid, mid freq, high.
    smooth: [Smoother; 6],
    last: [f32; 6],
}

impl Tone {
    pub fn new(fs: f32) -> Self {
        let init = |id: P| id.info().default;
        let ids = [
            P::LowCut,
            P::HighCut,
            P::LowGain,
            P::MidGain,
            P::MidFreq,
            P::HighGain,
        ];
        let mut t = Self {
            fs,
            filters: [Biquad::default(); 7],
            smooth: ids.map(|id| Smoother::new(fs, 0.03, init(id))),
            last: [f32::NAN; 6],
        };
        t.update_coeffs(ids.map(init));
        t
    }

    fn update_coeffs(&mut self, v: [f32; 6]) {
        if v == self.last {
            return;
        }
        self.last = v;
        let [low_cut, high_cut, low, mid, mid_f, high] = v;
        let fs = self.fs;
        for (i, q) in BUTTERWORTH_Q.iter().enumerate() {
            self.filters[i].c = Coeffs::highpass(fs, low_cut, *q);
            self.filters[2 + i].c = Coeffs::lowpass(fs, high_cut, *q);
        }
        self.filters[4].c = Coeffs::low_shelf(fs, LOW_SHELF_HZ, low);
        self.filters[5].c = Coeffs::peaking(fs, mid_f, MID_Q, mid);
        self.filters[6].c = Coeffs::high_shelf(fs, HIGH_SHELF_HZ, high);
    }

    pub fn process(&mut self, block: &mut Block, p: &Snapshot) {
        let targets = [
            P::LowCut,
            P::HighCut,
            P::LowGain,
            P::MidGain,
            P::MidFreq,
            P::HighGain,
        ]
        .map(|id| p.get(id));
        let mut v = [0.0; 6];
        for i in 0..6 {
            v[i] = self.smooth[i].next_n(targets[i], BLOCK);
        }
        self.update_coeffs(v);
        for s in block.iter_mut() {
            let mut x = *s;
            for f in &mut self.filters {
                x = f.process(x);
            }
            *s = x;
        }
    }

    pub fn reset(&mut self) {
        for f in &mut self.filters {
            f.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::biquad::tests::db;

    fn gain_at(tone: &mut Tone, p: &Snapshot, freq: f32) -> f32 {
        let mut peak = 0.0f32;
        let mut i = 0usize;
        for blk in 0..750 {
            let mut b = [0.0; BLOCK];
            for s in b.iter_mut() {
                *s = (2.0 * std::f32::consts::PI * freq * i as f32 / 48_000.0).sin();
                i += 1;
            }
            tone.process(&mut b, p);
            if blk > 400 {
                peak = b.iter().fold(peak, |m, x| m.max(x.abs()));
            }
        }
        peak
    }

    #[test]
    fn flat_settings_pass_the_signal_unchanged() {
        let p = Snapshot::defaults();
        for f in [100.0, 1000.0, 8000.0] {
            let g = db(gain_at(&mut Tone::new(48_000.0), &p, f));
            assert!(g.abs() < 0.3, "{f} Hz: {g} dB");
        }
    }

    #[test]
    fn telephone_band_cuts_lows_and_highs_hard() {
        let mut p = Snapshot::defaults();
        p.set(P::LowCut, 300.0);
        p.set(P::HighCut, 3400.0);
        assert!(db(gain_at(&mut Tone::new(48_000.0), &p, 1000.0)).abs() < 1.0);
        assert!(db(gain_at(&mut Tone::new(48_000.0), &p, 75.0)) < -40.0);
        assert!(db(gain_at(&mut Tone::new(48_000.0), &p, 12_000.0)) < -40.0);
    }
}
