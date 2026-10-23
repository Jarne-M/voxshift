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

    /// Linear-interpolated read `delay` samples behind the write head (delay >= 0 is "now").
    #[inline]
    fn read(&self, delay: f32) -> f32 {
        let delay = delay.clamp(0.0, self.max_delay());
        let d = delay.floor();
        let frac = delay - d;
        let i = self.write.wrapping_sub(d as usize) & self.mask;
        let j = i.wrapping_sub(1) & self.mask;
        self.buf[i] + (self.buf[j] - self.buf[i]) * frac
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
        let lo = (nominal as isize - self.search as isize).max(n as isize + 1) as usize;
        let hi = ((nominal as usize) + self.search).min(self.buf.len() - 2);
        let (mut best, mut best_score) = (nominal, f32::MIN);
        // Step 2 is plenty: the final position is still sub-sample interpolated.
        for d in (lo..=hi).step_by(2) {
            let (mut dot, mut energy) = (0.0f32, 1e-9f32);
            for j in (0..n).step_by(2) {
                let c = self.at(d - j);
                dot += c * reference(j);
                energy += c * c;
            }
            let score = dot / energy.sqrt();
            if score > best_score {
                best_score = score;
                best = d as f32;
            }
        }
        best
    }

    /// Processes one sample. `ratio` = 2^(semitones / 12).
    #[inline]
    pub fn process(&mut self, x: f32, ratio: f32) -> f32 {
        self.buf[self.write] = x;
        let rate = 1.0 - ratio; // delay change per sample
        let speed = rate.abs().max(MIN_RATE);
        let age_step = speed / self.window;
        // Share of the window the delay sweeps in one life: -1 (up) .. 1 (down), smaller near 1.
        let sweep = rate / speed;
        let mut y = 0.0;
        for h in 0..2 {
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
            y += self.read(head.delay) * w * w;
            self.heads[h] = head;
        }
        self.write = (self.write + 1) & self.mask;
        y
    }

    pub fn reset(&mut self) {
        self.buf.fill(0.0);
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
