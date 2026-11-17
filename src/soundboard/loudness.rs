//! How loud a clip is, so "Even out loudness" can play every clip at about the same level.
//!
//! The measure is a gated RMS in the spirit of EBU R128 (without its K-weighting filter):
//! the clip is cut into 400 ms windows, silent ones are ignored, and so are windows far
//! quieter than the rest, so a short shout after a long pause counts as loud. Measured once
//! on the loader thread when a clip loads; nothing here runs on the audio thread.

/// Where every clip ends up (dBFS, gated RMS). Leaves room for a voice on top.
pub const TARGET_DB: f32 = -20.0;
/// How far a clip is moved at most (a near-silent clip is not blown up to full level).
pub const MAX_BOOST_DB: f32 = 12.0;
pub const MAX_CUT_DB: f32 = 24.0;
/// The loudest sample may not go above this after the gain.
const PEAK_CEILING: f32 = 0.95;
const WINDOW_S: f32 = 0.4;
const ABSOLUTE_GATE_DB: f32 = -60.0;
const RELATIVE_GATE_DB: f32 = -10.0;

/// A clip's measured level.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Level {
    /// Gated RMS in dBFS.
    pub loudness_db: f32,
    /// Highest absolute sample (linear).
    pub peak: f32,
}

impl Level {
    /// The gain (linear) that brings the clip to [`TARGET_DB`], limited so it never clips.
    pub fn gain(&self) -> f32 {
        let db = (TARGET_DB - self.loudness_db).clamp(-MAX_CUT_DB, MAX_BOOST_DB);
        let gain = 10f32.powf(db / 20.0);
        if self.peak > 0.0 {
            gain.min(PEAK_CEILING / self.peak)
        } else {
            gain
        }
    }

    /// The gain in dB, for showing.
    pub fn gain_db(&self) -> f32 {
        20.0 * self.gain().max(1e-6).log10()
    }
}

fn db(power: f64) -> f32 {
    (10.0 * power.max(1e-12).log10()) as f32
}

/// Measures interleaved stereo at `rate`. `None` for a silent (or empty) clip.
pub fn measure(frames: &[f32], rate: u32) -> Option<Level> {
    let pairs = frames.as_chunks::<2>().0;
    if pairs.is_empty() || rate == 0 {
        return None;
    }
    let peak = frames.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    let window = ((rate as f32 * WINDOW_S) as usize).max(1);
    // Mean square (both channels averaged) per window; a short clip is one window.
    let powers: Vec<f64> = pairs
        .chunks(window)
        .map(|w| {
            let sum: f64 = w
                .iter()
                .map(|[l, r]| (f64::from(*l).powi(2) + f64::from(*r).powi(2)) / 2.0)
                .sum();
            sum / w.len() as f64
        })
        .collect();
    let mean = |ps: &mut dyn Iterator<Item = f64>| {
        let (sum, n) = ps.fold((0.0, 0usize), |(s, n), p| (s + p, n + 1));
        (n > 0).then(|| sum / n as f64)
    };
    let loud = |p: &f64| db(*p) > ABSOLUTE_GATE_DB;
    let ungated = mean(&mut powers.iter().copied().filter(loud))?;
    let gate = db(ungated) + RELATIVE_GATE_DB;
    let gated = mean(&mut powers.iter().copied().filter(|p| loud(p) && db(*p) > gate))?;
    Some(Level {
        loudness_db: db(gated),
        peak,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(amplitude: f32, seconds: f32) -> Vec<f32> {
        let n = (48_000.0 * seconds) as usize;
        (0..n)
            .flat_map(|i| {
                let x = amplitude * (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin();
                [x, x]
            })
            .collect()
    }

    #[test]
    fn a_sine_measures_as_its_rms_and_silence_as_nothing() {
        // A sine's RMS is its amplitude / sqrt(2): -3 dB below the peak.
        let l = measure(&tone(0.5, 2.0), 48_000).unwrap();
        assert!((l.loudness_db - (-9.03)).abs() < 0.1, "{l:?}");
        assert!((l.peak - 0.5).abs() < 1e-3);
        assert!(measure(&vec![0.0; 48_000], 48_000).is_none());
        assert!(measure(&[], 48_000).is_none());
    }

    #[test]
    fn pauses_do_not_make_a_clip_count_as_quiet() {
        let mut clip = vec![0.0; 48_000 * 2 * 5]; // five seconds of silence...
        clip.extend(tone(0.5, 1.0)); // ...then a shout
        let l = measure(&clip, 48_000).unwrap();
        // (One window straddles the start of the shout: within a dB, not 8 dB lower.)
        assert!((l.loudness_db - (-9.03)).abs() < 1.0, "{l:?}");
    }

    #[test]
    fn the_gain_brings_clips_to_the_target_without_clipping() {
        let quiet = measure(&tone(0.05, 2.0), 48_000).unwrap(); // about -29 dB
        let loud = measure(&tone(0.9, 2.0), 48_000).unwrap(); // about -4 dB
        assert!((quiet.loudness_db + 20.0 * quiet.gain().log10() - TARGET_DB).abs() < 0.1);
        assert!((loud.loudness_db + loud.gain_db() - TARGET_DB).abs() < 0.1);
        assert!(loud.gain() < 1.0 && quiet.gain() > 1.0);
        // A very quiet clip is not boosted more than the limit...
        let whisper = measure(&tone(0.002, 2.0), 48_000).unwrap();
        assert!((whisper.gain_db() - MAX_BOOST_DB).abs() < 0.01);
        // ...and a spiky one (quiet on average, one loud click) stops below full scale.
        let mut spiky = tone(0.02, 2.0);
        spiky[1000] = 0.9;
        let l = measure(&spiky, 48_000).unwrap();
        assert!(l.peak * l.gain() <= PEAK_CEILING + 1e-6, "{l:?}");
    }
}
