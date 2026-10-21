//! Noise gate: mutes the mic between words so keyboard and room noise do not get through.
//! Envelope follower -> open/closed with hysteresis and hold time -> smoothed gain.

use super::Block;
use super::params::{P, Snapshot};
use crate::audio::shared::from_db;

const HYSTERESIS_DB: f32 = 4.0;
const HOLD_S: f32 = 0.08;
const ENV_RELEASE_S: f32 = 0.03;
const ATTACK_S: f32 = 0.001;
const RELEASE_S: f32 = 0.06;

pub struct Gate {
    env: f32,
    gain: f32,
    open: bool,
    hold_left: usize,
    hold: usize,
    env_release: f32,
    attack: f32,
    release: f32,
}

fn coeff(fs: f32, t: f32) -> f32 {
    (-1.0 / (t * fs)).exp()
}

impl Gate {
    pub fn new(fs: f32) -> Self {
        Self {
            env: 0.0,
            gain: 0.0,
            open: false,
            hold_left: 0,
            hold: (HOLD_S * fs) as usize,
            env_release: coeff(fs, ENV_RELEASE_S),
            attack: coeff(fs, ATTACK_S),
            release: coeff(fs, RELEASE_S),
        }
    }

    pub fn process(&mut self, block: &mut Block, p: &Snapshot) {
        let open_at = from_db(p.get(P::GateThreshold));
        let close_at = open_at * from_db(-HYSTERESIS_DB);
        for s in block.iter_mut() {
            let a = s.abs();
            self.env = a.max(self.env * self.env_release);
            if self.env >= open_at {
                self.open = true;
                self.hold_left = self.hold;
            } else if self.env < close_at {
                if self.hold_left > 0 {
                    self.hold_left -= 1;
                } else {
                    self.open = false;
                }
            }
            let target = if self.open { 1.0 } else { 0.0 };
            let k = if target > self.gain {
                self.attack
            } else {
                self.release
            };
            self.gain = target + (self.gain - target) * k;
            *s *= self.gain;
        }
    }

    pub fn reset(&mut self) {
        self.env = 0.0;
        self.gain = 0.0;
        self.open = false;
        self.hold_left = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::block::BLOCK;

    fn run(gate: &mut Gate, p: &Snapshot, amp: f32, blocks: usize) -> f32 {
        let mut peak = 0.0f32;
        for n in 0..blocks {
            let mut b = [0.0; BLOCK];
            for (i, s) in b.iter_mut().enumerate() {
                *s = amp * ((n * BLOCK + i) as f32 * 0.05).sin();
            }
            gate.process(&mut b, p);
            if n > blocks * 3 / 4 {
                peak = b.iter().fold(peak, |m, x| m.max(x.abs()));
            }
        }
        peak
    }

    #[test]
    fn mutes_quiet_noise_and_passes_speech() {
        let mut p = Snapshot::defaults();
        p.set(P::GateThreshold, -40.0);
        let mut g = Gate::new(48_000.0);
        assert!(run(&mut g, &p, 0.003, 200) < 1e-4, "-50 dB noise is muted");
        assert!(
            (run(&mut g, &p, 0.2, 200) - 0.2).abs() < 0.01,
            "speech passes"
        );
        assert!(
            run(&mut g, &p, 0.003, 400) < 1e-4,
            "closes again after the hold"
        );
    }
}
