//! Low-latency pitch shifter: a delay line read by two "tape heads" moving at a different speed
//! than the write head.
//!
//! Reading a delay line whose delay shrinks by (ratio - 1) samples per sample plays the audio
//! `ratio` times faster, i.e. `ratio` times higher. The delay can't shrink forever, so each head
//! lives for one window and then restarts. Two heads half a life apart, each faded with a Hann
//! window that is zero exactly where it restarts, hide the jumps: sin^2 + cos^2 = 1.
//!
//! A naive restart jumps by exactly one window, which lands at a random point of the waveform
//! cycle and slightly detunes the result (the phase slips every grain). So when a head restarts
//! we search a few ms around the nominal position for the spot whose waveform best matches the
//! other head (normalised cross-correlation): the grains splice in phase. This is the idea
//! behind WSOLA and classic hardware harmonisers.

/// Head lifetime (one fade in + out). Two voice periods fit.
pub const WINDOW_S: f32 = 0.018;
/// How far around the nominal restart point to search for the best splice (one deep-voice period).
pub const SEARCH_S: f32 = 0.006;
/// Length of the waveform snippets compared when splicing.
const MATCH_S: f32 = 0.003;
/// Heads age at least this fast (as if |ratio - 1| were this big). At a ratio of exactly 1 they
/// would otherwise freeze at two fixed delays half a window apart: a static comb filter as
/// soon as the voice's pitch moves. Melody Voice and auto-tune spend a lot of time near 1.
const MIN_RATE: f32 = 0.02;

#[derive(Clone, Copy, Debug)]
struct Head {
    delay: f32,
    /// 0..1 through its life; the fade is sin^2(pi * age).
    age: f32,
}

pub struct PitchShifter {
    buf: Vec<f32>,
    /// A second signal shifted by the very same heads (see [`process_pair`](Self::process_pair)).
    aux: Vec<f32>,
    mask: usize,
    write: usize,
    window: f32,
    search: usize,
    match_len: usize,
    heads: [Head; 2],
}

impl PitchShifter {
    pub fn new(fs: f32) -> Self {
        let window = (WINDOW_S * fs).round().max(64.0);
        let search = (SEARCH_S * fs) as usize;
        let match_len = ((MATCH_S * fs) as usize).max(16);
        let len = (window as usize + 2 * search + match_len + 8).next_power_of_two();
        let mid = 1.0 + search as f32 + window / 2.0;
        Self {
            buf: vec![0.0; len],
            aux: Vec::new(),
            mask: len - 1,
            write: 0,
            window,
            search,
            match_len,
            heads: [
                Head {
                    delay: mid,
                    age: 0.0,
                },
                Head {
                    delay: mid,
                    age: 0.5,
                },
            ],
        }
    }

    /// A shifter that also carries a second signal (for [`process_pair`](Self::process_pair)).
    pub fn with_aux(fs: f32) -> Self {
        let mut s = Self::new(fs);
        s.aux = vec![0.0; s.buf.len()];
        s
    }

    /// Delay added on average, in samples.
    pub fn latency(&self) -> usize {
        1 + self.search + (self.window / 2.0) as usize
    }

    /// [`latency`](Self::latency) of a shifter at rate `fs`, without building one.
    pub fn latency_for(fs: f32) -> usize {
        let window = (WINDOW_S * fs).round().max(64.0);
        1 + (SEARCH_S * fs) as usize + (window / 2.0) as usize
    }

    fn max_delay(&self) -> f32 {
        (self.buf.len() - 2) as f32
    }

    /// Linear-interpolated read of `buf`, `delay` samples behind the write head (delay >= 0
    /// is "now").
    #[inline]
    fn read(&self, buf: &[f32], delay: f32) -> f32 {
        let delay = delay.clamp(0.0, self.max_delay());
        let d = delay.floor();
        let frac = delay - d;
        let i = self.write.wrapping_sub(d as usize) & self.mask;
        let j = i.wrapping_sub(1) & self.mask;
        buf[i] + (buf[j] - buf[i]) * frac
    }

    #[inline]
    fn at(&self, delay: usize) -> f32 {
        self.buf[self.write.wrapping_sub(delay) & self.mask]
    }

    /// Picks the restart delay near `nominal` whose upcoming waveform best matches what the
    /// other head (at `other`) is about to play.
    fn best_splice(&self, nominal: f32, other: f32) -> f32 {
        let n = self.match_len;
        let o = other.round() as usize;
        if o < n {
            return nominal;
        }
        let reference = |j: usize| self.at(o - j);
        let lo = (nominal as isize - self.search as isize).max(n as isize + 2) as usize;
        let hi = ((nominal as usize) + self.search).min(self.buf.len() - 3);
        let score = |d: usize| {
            let (mut dot, mut energy) = (0.0f32, 1e-9f32);
            for j in (0..n).step_by(2) {
                let c = self.at(d - j);
                dot += c * reference(j);
                energy += c * c;
            }
            dot / energy.sqrt()
        };
        // Coarse search in steps of 2, then the best spot's neighbours, then a parabola
        // through the peak: the splice lands within a fraction of a sample, which also suits
        // the spiky LPC residual that is shifted along with the voice (`process_pair`).
        let (mut best, mut best_score) = (lo, f32::MIN);
        for d in (lo..=hi).step_by(2) {
            let s = score(d);
            if s > best_score {
                (best, best_score) = (d, s);
            }
        }
        for d in [best.saturating_sub(1).max(lo), (best + 1).min(hi)] {
            let s = score(d);
            if s > best_score {
                (best, best_score) = (d, s);
            }
        }
        if best <= lo || best >= hi {
            return best as f32;
        }
        let (a, b, c) = (score(best - 1), best_score, score(best + 1));
        let den = a - 2.0 * b + c;
        let offset = if den < -1e-9 {
            (0.5 * (a - c) / den).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        best as f32 + offset
    }

    /// Moves both heads on by one sample (restarting one at the best splice if its life is
    /// over); returns each head's delay and fade gain. Call after writing the new sample.
    #[inline]
    fn step(&mut self, ratio: f32) -> [(f32, f32); 2] {
        let rate = 1.0 - ratio; // delay change per sample
        let speed = rate.abs().max(MIN_RATE);
        let age_step = speed / self.window;
        // Share of the window the delay sweeps in one life: -1 (up) .. 1 (down), smaller near 1.
        let sweep = rate / speed;
        let mut out = [(0.0, 0.0); 2];
        for (h, o) in out.iter_mut().enumerate() {
            let mut head = self.heads[h];
            head.delay += rate;
            head.age += age_step;
            if head.age >= 1.0 {
                head.age -= 1.0;
                // Pitch up: the delay shrinks, so start at the long end; pitch down: the short
                // end. In between, start so that the sweep stays centred on the average delay.
                let mid = 1.0 + self.search as f32 + self.window / 2.0;
                let nominal = mid - sweep * self.window / 2.0;
                head.delay = self.best_splice(nominal, self.heads[1 - h].delay);
            }
            let w = (std::f32::consts::PI * head.age).sin();
            *o = (head.delay, w * w);
            self.heads[h] = head;
        }
        out
    }

    /// Processes one sample. `ratio` = 2^(semitones / 12). (The effects use
    /// [`process_pair`](Self::process_pair); this is the plain shifter on its own.)
    #[cfg(test)]
    #[inline]
    pub fn process(&mut self, x: f32, ratio: f32) -> f32 {
        self.buf[self.write] = x;
        let heads = self.step(ratio);
        let y = heads
            .iter()
            .map(|&(d, w)| self.read(&self.buf, d) * w)
            .sum();
        self.write = (self.write + 1) & self.mask;
        y
    }

    /// Shifts `x` and, with exactly the same heads and splices, `aux` (needs
    /// [`with_aux`](Self::with_aux)). The splices are chosen on `x`: the formant path shifts
    /// the LPC residual this way, a spiky, noise-like signal whose own waveform is a poor guide
    /// for splicing, and it saves a second splice search.
    #[inline]
    pub fn process_pair(&mut self, x: f32, aux: f32, ratio: f32) -> (f32, f32) {
        self.buf[self.write] = x;
        self.aux[self.write] = aux;
        let heads = self.step(ratio);
        let (mut y, mut z) = (0.0, 0.0);
        for (d, w) in heads {
            y += self.read(&self.buf, d) * w;
            z += self.read(&self.aux, d) * w;
        }
        self.write = (self.write + 1) & self.mask;
        (y, z)
    }

    pub fn reset(&mut self) {
        self.buf.fill(0.0);
        self.aux.fill(0.0);
    }
}

pub fn semitones_to_ratio(st: f32) -> f32 {
    2f32.powf(st / 12.0)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::f32::consts::PI;

    /// Dominant frequency by counting rising zero crossings over the second half.
    pub fn frequency(signal: &[f32], fs: f32) -> f32 {
        let tail = &signal[signal.len() / 2..];
        let crossings = tail
            .windows(2)
            .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
            .count();
        crossings as f32 * fs / tail.len() as f32
    }

    fn shifted(freq: f32, semis: f32) -> Vec<f32> {
        let fs = 48_000.0;
        let mut p = PitchShifter::new(fs);
        let ratio = semitones_to_ratio(semis);
        (0..96_000)
            .map(|i| p.process((2.0 * PI * freq * i as f32 / fs).sin(), ratio))
            .collect()
    }

    #[test]
    fn shifts_by_the_requested_interval() {
        for freq in [110.0, 200.0, 330.0] {
            for semis in [12.0, -12.0, 7.0, -5.0, 3.0] {
                let want = freq * semitones_to_ratio(semis);
                let f = frequency(&shifted(freq, semis), 48_000.0);
                assert!(
                    (f - want).abs() / want < 0.015,
                    "{freq} Hz {semis} st: {f} Hz, want {want}"
                );
            }
        }
    }

    #[test]
    fn a_ratio_of_one_keeps_splicing_instead_of_freezing() {
        // Shift a little, then sit at exactly 1 while the input pitch glides. Heads frozen
        // half a window apart would comb-filter the glide: deep level dips at the notches.
        let fs = 48_000.0;
        let mut p = PitchShifter::new(fs);
        let mut phase = 0.0f32;
        let out: Vec<f32> = (0..96_000)
            .map(|i| {
                phase = (phase + (150.0 + 75.0 * i as f32 / fs) / fs).fract();
                let ratio = if i < 6_000 { 1.1 } else { 1.0 };
                p.process((phase * 2.0 * PI).sin(), ratio)
            })
            .collect();
        for chunk in out[12_000..].chunks(480) {
            let r = (chunk.iter().map(|x| x * x).sum::<f32>() / chunk.len() as f32).sqrt();
            assert!(r > 0.45, "dip: {r}");
        }
        // `frequency` looks at the last half: 1.5..2 s, where the glide is around 281 Hz.
        let f = frequency(&out[48_000..], fs);
        assert!((f - 281.0).abs() < 8.0, "unshifted glide: {f} Hz");
    }

    #[test]
    fn keeps_the_level_without_dropouts() {
        let out = shifted(200.0, -5.0);
        let tail = &out[48_000..];
        let rms = (tail.iter().map(|x| x * x).sum::<f32>() / tail.len() as f32).sqrt();
        assert!((rms - 0.707).abs() < 0.1, "rms {rms}");
        // In-phase splicing: no 5 ms stretch drops far below the average level.
        for chunk in tail.chunks(240) {
            let r = (chunk.iter().map(|x| x * x).sum::<f32>() / chunk.len() as f32).sqrt();
            assert!(r > 0.45, "dropout: {r}");
        }
    }
}
