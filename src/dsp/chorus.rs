//! Chorus: two copies of the voice with slowly wobbling short delays, which sound like several
//! slightly detuned voices at once.

use super::Block;
use super::delay::DelayLine;
use super::params::{P, Snapshot};
use super::smooth::Smoother;
use std::f32::consts::TAU;

const BASE_MS: f32 = 12.0;

pub struct Chorus {
    fs: f32,
    line: DelayLine,
    lfo: f32,
    rate: Smoother,
    depth: Smoother,
    mix: Smoother,
}

impl Chorus {
    pub fn new(fs: f32) -> Self {
        Self {
            fs,
            line: DelayLine::new((0.04 * fs) as usize),
            lfo: 0.0,
            rate: Smoother::new(fs, 0.05, P::ChorusRate.info().default),
            depth: Smoother::new(fs, 0.05, P::ChorusDepth.info().default),
            mix: Smoother::new(fs, 0.02, P::ChorusMix.info().default),
        }
    }

    pub fn process(&mut self, block: &mut Block, p: &Snapshot) {
        let (rate, depth, mix) = (p.get(P::ChorusRate), p.get(P::ChorusDepth), p.get(P::ChorusMix));
        let ms = self.fs / 1000.0;
        for s in block.iter_mut() {
            self.line.push(*s);
            self.lfo += self.rate.next(rate) / self.fs;
            self.lfo -= self.lfo.floor();
            let d = self.depth.next(depth) * ms * 0.5;
            let base = BASE_MS * ms;
            let t1 = self.line.read(base + d * (1.0 + (TAU * self.lfo).sin()));
            let t2 = self.line.read(base + d * (1.0 + (TAU * (self.lfo + 0.5)).sin()));
            let wet = 0.5 * (t1 + t2);
            *s += (wet - *s) * self.mix.next(mix) * 0.7;
        }
    }

    pub fn reset(&mut self) {
        self.line.reset();
    }
}
