//! Reverb after Jezar's public-domain "Freeverb": eight parallel feedback comb filters with
//! damping (the dense, decaying tail) followed by four series all-pass filters (diffusion).

use super::Block;
use super::params::{P, Snapshot};
use super::smooth::Smoother;
use crate::audio::block::BLOCK;

/// Freeverb's tuning, in samples at 44.1 kHz.
const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASSES: [usize; 4] = [556, 441, 341, 225];
const INPUT_GAIN: f32 = 0.015;
const WET_GAIN: f32 = 3.0;

struct Comb {
    buf: Vec<f32>,
    idx: usize,
    store: f32,
}

impl Comb {
    #[inline]
    fn process(&mut self, x: f32, feedback: f32, damp: f32) -> f32 {
        let out = self.buf[self.idx];
        self.store = out * (1.0 - damp) + self.store * damp;
        self.buf[self.idx] = x + self.store * feedback;
        self.idx = (self.idx + 1) % self.buf.len();
        out
    }
}

struct AllPass {
    buf: Vec<f32>,
    idx: usize,
}

impl AllPass {
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let delayed = self.buf[self.idx];
        self.buf[self.idx] = x + delayed * 0.5;
        self.idx = (self.idx + 1) % self.buf.len();
        delayed - x
    }
}

pub struct Reverb {
    combs: Vec<Comb>,
    allpasses: Vec<AllPass>,
    size: Smoother,
    damping: Smoother,
    mix: Smoother,
}

impl Reverb {
    pub fn new(fs: f32) -> Self {
        let scale = |n: usize| ((n as f32 * fs / 44_100.0) as usize).max(1);
        Self {
            combs: COMBS
                .iter()
                .map(|&n| Comb {
                    buf: vec![0.0; scale(n)],
                    idx: 0,
                    store: 0.0,
                })
                .collect(),
            allpasses: ALLPASSES
                .iter()
                .map(|&n| AllPass {
                    buf: vec![0.0; scale(n)],
                    idx: 0,
                })
                .collect(),
            size: Smoother::new(fs, 0.05, P::ReverbSize.info().default),
            damping: Smoother::new(fs, 0.05, P::ReverbDamping.info().default),
            mix: Smoother::new(fs, 0.02, P::ReverbMix.info().default),
        }
    }

    pub fn process(&mut self, block: &mut Block, p: &Snapshot) {
        let feedback = self.size.next_n(p.get(P::ReverbSize), BLOCK) * 0.28 + 0.7;
        let damp = self.damping.next_n(p.get(P::ReverbDamping), BLOCK) * 0.4;
        let mix = p.get(P::ReverbMix);
        for s in block.iter_mut() {
            let input = *s * INPUT_GAIN;
            let mut wet = 0.0;
            for c in &mut self.combs {
                wet += c.process(input, feedback, damp);
            }
            for a in &mut self.allpasses {
                wet = a.process(wet);
            }
            let m = self.mix.next(mix);
            *s = *s * (1.0 - 0.5 * m) + wet * WET_GAIN * m;
        }
    }

    pub fn reset(&mut self) {
        for c in &mut self.combs {
            c.buf.fill(0.0);
            c.store = 0.0;
        }
        for a in &mut self.allpasses {
            a.buf.fill(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impulse_gives_a_decaying_tail() {
        let mut p = Snapshot::defaults();
        p.set(P::ReverbSize, 1.0);
        p.set(P::ReverbMix, 1.0);
        let mut r = Reverb::new(48_000.0);
        let mut energy = Vec::new();
        for n in 0..(48_000 * 3 / BLOCK) {
            let mut b = [0.0; BLOCK];
            if n == 0 {
                b[0] = 1.0;
            }
            r.process(&mut b, &p);
            assert!(b.iter().all(|x| x.is_finite()));
            energy.push(b.iter().map(|x| x * x).sum::<f32>());
        }
        let early: f32 = energy[20..200].iter().sum();
        let late: f32 = energy[energy.len() - 200..].iter().sum();
        assert!(early > 1e-4, "has a tail");
        assert!(late < early * 0.05, "and it decays: {late} vs {early}");
    }
}
