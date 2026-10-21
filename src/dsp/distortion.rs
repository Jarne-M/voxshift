//! Soft-clipping distortion: drive the signal into tanh, then tame the fizz with a low-pass.

use super::Block;
use super::biquad::{Biquad, Coeffs};
use super::params::{P, Snapshot};
use super::smooth::Smoother;
use crate::audio::shared::from_db;

pub struct Distortion {
    drive: Smoother,
    mix: Smoother,
    tone: Biquad,
}

impl Distortion {
    pub fn new(fs: f32) -> Self {
        let mut tone = Biquad::default();
        tone.c = Coeffs::lowpass(fs, 6500.0, 0.707);
        Self {
            drive: Smoother::new(fs, 0.03, from_db(P::DistDrive.info().default)),
            mix: Smoother::new(fs, 0.02, P::DistMix.info().default),
            tone,
        }
    }

    pub fn process(&mut self, block: &mut Block, p: &Snapshot) {
        let drive = from_db(p.get(P::DistDrive));
        let mix = p.get(P::DistMix);
        for s in block.iter_mut() {
            let g = self.drive.next(drive);
            // Loudness grows with drive, but only as its square root.
            let wet = self.tone.process((g * *s).tanh() / g.sqrt());
            let m = self.mix.next(mix);
            *s += (wet - *s) * m;
        }
    }

    pub fn reset(&mut self) {
        self.tone.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::block::BLOCK;

    #[test]
    fn output_is_bounded_even_at_full_drive() {
        let mut p = Snapshot::defaults();
        p.set(P::DistDrive, 40.0);
        let mut d = Distortion::new(48_000.0);
        for n in 0..100 {
            let mut b = [0.0; BLOCK];
            for (i, s) in b.iter_mut().enumerate() {
                *s = ((n * BLOCK + i) as f32 * 0.03).sin();
            }
            d.process(&mut b, &p);
            assert!(b.iter().all(|x| x.abs() <= 1.2));
        }
    }
}
