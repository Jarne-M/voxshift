//! The voice chain: input gain -> effects -> output gain, plus the master bypass.

use super::params::{P, Snapshot};
use super::smooth::Smoother;
use super::{Block, VoiceProcessor};
use crate::audio::shared::from_db;

pub struct VoiceChain {
    input_gain: Smoother,
    output_gain: Smoother,
    /// 1 = processed voice, 0 = clean passthrough (master toggle off).
    engaged: Smoother,
}

impl VoiceChain {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            input_gain: Smoother::new(sample_rate, 0.02, 1.0),
            output_gain: Smoother::new(sample_rate, 0.02, 1.0),
            engaged: Smoother::new(sample_rate, 0.02, 1.0),
        }
    }

    /// Runs the chain; `voice_on == false` crossfades to the untouched input.
    pub fn run(&mut self, block: &mut Block, params: &Snapshot, voice_on: bool) {
        let dry = *block;
        self.process(block, params);
        let target = if voice_on { 1.0 } else { 0.0 };
        if voice_on && self.engaged.settled(1.0) {
            return;
        }
        for (s, d) in block.iter_mut().zip(dry.iter()) {
            let e = self.engaged.next(target);
            *s = *d + (*s - *d) * e;
        }
    }
}

impl VoiceProcessor for VoiceChain {
    fn process(&mut self, block: &mut Block, params: &Snapshot) {
        let gin = from_db(params.get(P::InputGain));
        let gout = from_db(params.get(P::OutputGain));
        for s in block.iter_mut() {
            *s *= self.input_gain.next(gin) * self.output_gain.next(gout);
        }
    }
}
