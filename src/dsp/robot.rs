//! Robot voice: ring modulation. Multiplying the voice by a sine at a few dozen Hz smears every
//! harmonic into a sum and a difference frequency, which sounds metallic and "Dalek"-like.

use super::Block;
use super::params::{P, Snapshot};
use super::smooth::Smoother;
use std::f32::consts::TAU;

pub struct Robot {
    fs: f32,
    phase: f32,
    freq: Smoother,
    mix: Smoother,
}

impl Robot {
    pub fn new(fs: f32) -> Self {
        Self {
            fs,
            phase: 0.0,
            freq: Smoother::new(fs, 0.05, P::RobotFreq.info().default),
            mix: Smoother::new(fs, 0.02, P::RobotMix.info().default),
        }
    }

    pub fn process(&mut self, block: &mut Block, p: &Snapshot) {
        let (freq, mix) = (p.get(P::RobotFreq), p.get(P::RobotMix));
        for s in block.iter_mut() {
            self.phase += self.freq.next(freq) / self.fs;
            self.phase -= self.phase.floor();
            // x2 makes up for the average |sin| loss so the level stays similar.
            let wet = *s * (TAU * self.phase).sin() * std::f32::consts::SQRT_2;
            *s += (wet - *s) * self.mix.next(mix);
        }
    }

    pub fn reset(&mut self) {
        self.phase = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::block::BLOCK;
    use crate::dsp::pitch::tests::frequency;

    #[test]
    fn dc_becomes_the_carrier() {
        let mut p = Snapshot::defaults();
        p.set(P::RobotFreq, 100.0);
        let mut r = Robot::new(48_000.0);
        let mut out = Vec::new();
        for _ in 0..1500 {
            let mut b = [0.5; BLOCK];
            r.process(&mut b, &p);
            out.extend_from_slice(&b);
        }
        let f = frequency(&out, 48_000.0);
        assert!((f - 100.0).abs() < 1.0, "{f}");
    }
}
