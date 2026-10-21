//! Echo: a feedback delay. Each repeat is darkened a little, like a real room or tape echo.

use super::Block;
use super::delay::DelayLine;
use super::params::{P, Snapshot};
use super::smooth::Smoother;

const MAX_S: f32 = 1.05;

pub struct Echo {
    fs: f32,
    line: DelayLine,
    time: Smoother,
    feedback: Smoother,
    mix: Smoother,
    damp: f32,
}

impl Echo {
    pub fn new(fs: f32) -> Self {
        Self {
            fs,
            line: DelayLine::new((MAX_S * fs) as usize),
            // Slow glide: changing the time gives a short pitch bend instead of a click.
            time: Smoother::new(fs, 0.15, P::EchoTime.info().default),
            feedback: Smoother::new(fs, 0.03, P::EchoFeedback.info().default),
            mix: Smoother::new(fs, 0.02, P::EchoMix.info().default),
            damp: 0.0,
        }
    }

    pub fn process(&mut self, block: &mut Block, p: &Snapshot) {
        let (time, fb, mix) = (
            p.get(P::EchoTime),
            p.get(P::EchoFeedback),
            p.get(P::EchoMix),
        );
        for s in block.iter_mut() {
            let delay = self.time.next(time) * self.fs / 1000.0;
            let wet = self.line.read(delay);
            // One-pole low-pass in the loop.
            self.damp += (wet - self.damp) * 0.35;
            self.line.push(*s + self.damp * self.feedback.next(fb));
            *s += wet * self.mix.next(mix);
        }
    }

    pub fn reset(&mut self) {
        self.line.reset();
        self.damp = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::block::BLOCK;

    #[test]
    fn impulse_repeats_at_the_delay_time_and_decays() {
        // Default time (250 ms), so the glide does not start from elsewhere.
        let mut p = Snapshot::defaults();
        p.set(P::EchoFeedback, 0.5);
        p.set(P::EchoMix, 1.0);
        let mut e = Echo::new(48_000.0);
        let mut out = Vec::new();
        for n in 0..(48_000 / BLOCK) {
            let mut b = [0.0; BLOCK];
            if n == 0 {
                b[0] = 1.0;
            }
            e.process(&mut b, &p);
            out.extend_from_slice(&b);
        }
        let peak_near = |center: usize| {
            out[center - 50..center + 50]
                .iter()
                .fold(0.0f32, |m, x| m.max(x.abs()))
        };
        let (first, second) = (peak_near(12_000), peak_near(24_000));
        assert!(first > 0.3, "first echo {first}");
        assert!(second < first && second > 0.05, "second echo {second}");
        assert!(
            out[6000..6500].iter().all(|x| x.abs() < 1e-6),
            "silence between"
        );
    }
}
