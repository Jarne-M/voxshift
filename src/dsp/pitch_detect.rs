//! Real-time pitch detection: YIN (de Cheveigné & Kawahara, 2002), written for a live voice.
//!
//! The voice is high-passed (rumble), low-passed and decimated to ~12 kHz: a voice's pitch is
//! below 1 kHz, and analysing a quarter of the samples makes the analysis 16 times cheaper.
//! Every ~2.5 ms the last `window` of samples is analysed:
//!
//! 1. Difference function `d(tau) = sum (x[j] - x[j + tau])^2`: zero at the period of a
//!    perfectly periodic signal.
//! 2. Cumulative mean normalisation `d'(tau) = d(tau) * tau / sum_{k<=tau} d(k)`: removes the
//!    bias toward tiny lags and makes the value comparable between frames (0 = perfectly
//!    periodic, ~1 = noise). `1 - d'` is the confidence.
//! 3. The *first* dip below the threshold wins (not the deepest): a deeper dip at twice the
//!    period is the classic octave-too-low error.
//! 4. Parabolic interpolation around the dip gives sub-sample precision.
//!
//! [`PitchTracker`] then smooths the raw estimates: a median over the last few frames, an
//! octave-jump guard (a sudden jump of exactly an octave must persist before it is believed)
//! and hysteresis on the voiced/unvoiced decision.

use super::biquad::{Biquad, Coeffs};

/// Analysis sample rate the input is decimated to (approximately).
const ANALYSIS_RATE: f32 = 12_000.0;
/// Time between analyses.
pub const HOP_S: f32 = 0.0025;
/// Longest analysis window the buffers are sized for.
pub const MAX_WINDOW_S: f32 = 0.08;

/// One analysis result.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Estimate {
    /// Fundamental frequency; meaningful only if `voiced`.
    pub hz: f32,
    pub voiced: bool,
    /// 0..1, how periodic the frame is (1 - YIN's normalised difference at the period).
    pub confidence: f32,
    /// RMS level of the analysed frame (linear).
    pub level: f32,
}

/// What the detector looks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DetectorSettings {
    pub min_hz: f32,
    pub max_hz: f32,
    /// Analysis window in seconds (longer = steadier and lower pitches, but later).
    pub window_s: f32,
    /// YIN threshold: a frame is voiced if `d'` dips below this (0.1 strict .. 0.35 lenient).
    pub threshold: f32,
    /// Frames quieter than this RMS (linear) are unvoiced whatever their shape.
    pub level_floor: f32,
}

impl Default for DetectorSettings {
    fn default() -> Self {
        Self {
            min_hz: 70.0,
            max_hz: 500.0,
            window_s: 0.03,
            threshold: 0.2,
            level_floor: 0.003, // about -50 dBFS
        }
    }
}

pub struct PitchDetector {
    decim: usize,
    fs_a: f32,
    hp: Biquad,
    lp: [Biquad; 2],
    phase: usize,
    ring: Vec<f32>,
    pos: usize,
    frame: Vec<f32>,
    diff: Vec<f32>,
    hop: usize,
    until: usize,
    /// Analysis-rate samples per frame, longest lag and shortest lag.
    n: usize,
    tau_max: usize,
    tau_min: usize,
    settings: DetectorSettings,
    last: Estimate,
}

impl PitchDetector {
    pub fn new(fs: f32, settings: DetectorSettings) -> Self {
        let decim = ((fs / ANALYSIS_RATE).round() as usize).max(1);
        let fs_a = fs / decim as f32;
        let cap = (MAX_WINDOW_S * fs_a).ceil() as usize + 2;
        // 4th-order Butterworth low-pass at a quarter of the analysis rate (two biquads with
        // the Butterworth Qs): keeps the first harmonics, stops what would alias.
        let cut = fs_a * 0.25;
        let mut d = Self {
            decim,
            fs_a,
            hp: Biquad::new(Coeffs::highpass(fs, 50.0, 0.707)),
            lp: [
                Biquad::new(Coeffs::lowpass(fs, cut, 0.541)),
                Biquad::new(Coeffs::lowpass(fs, cut, 1.307)),
            ],
            phase: 0,
            ring: vec![0.0; cap],
            pos: 0,
            frame: vec![0.0; cap],
            diff: vec![0.0; cap],
            hop: ((HOP_S * fs_a).round() as usize).max(1),
            until: 1,
            n: 0,
            tau_max: 0,
            tau_min: 0,
            settings,
            last: Estimate::default(),
        };
        d.configure(settings);
        d
    }

    /// Changes range, window, threshold or level floor. Never allocates (real-time safe).
    pub fn configure(&mut self, s: DetectorSettings) {
        let cap = self.ring.len() - 2;
        let n = ((s.window_s.clamp(0.01, MAX_WINDOW_S) * self.fs_a) as usize).clamp(16, cap);
        let max_hz = s.max_hz.max(s.min_hz * 1.5);
        // Lags up to one period of the lowest pitch, but leave at least a third of the frame
        // to integrate over.
        let tau_max = ((self.fs_a / s.min_hz.max(20.0)).ceil() as usize + 1).min(n * 2 / 3);
        let tau_min = ((self.fs_a / max_hz).floor() as usize).clamp(2, tau_max - 2);
        self.n = n;
        self.tau_max = tau_max;
        self.tau_min = tau_min;
        self.settings = s;
    }

    /// Lowest pitch the current window can see (may be above `min_hz` for short windows).
    pub fn lowest_hz(&self) -> f32 {
        self.fs_a / (self.tau_max - 1) as f32
    }

    /// The analysis looks at this much past audio: its estimate describes the signal about
    /// half of it ago.
    pub fn window_samples(&self) -> usize {
        self.n * self.decim
    }

    pub fn last(&self) -> Estimate {
        self.last
    }

    /// Feeds one input sample. Returns a new estimate when an analysis ran.
    #[inline]
    pub fn push(&mut self, x: f32) -> Option<Estimate> {
        let y = self.lp[0].process(self.hp.process(x));
        let y = self.lp[1].process(y);
        self.phase += 1;
        if self.phase < self.decim {
            return None;
        }
        self.phase = 0;
        self.ring[self.pos] = y;
        self.pos = (self.pos + 1) % self.ring.len();
        self.until -= 1;
        if self.until > 0 {
            return None;
        }
        self.until = self.hop;
        self.last = self.analyze();
        Some(self.last)
    }

    fn analyze(&mut self) -> Estimate {
        let (n, cap) = (self.n, self.ring.len());
        let start = self.pos + cap - n;
        let mut energy = 0.0f32;
        for i in 0..n {
            let v = self.ring[(start + i) % cap];
            self.frame[i] = v;
            energy += v * v;
        }
        let level = (energy / n as f32).sqrt();
        let (tau_min, tau_max) = (self.tau_min, self.tau_max);
        let w = n - tau_max;
        let x = &self.frame[..n];

        // Difference function, then cumulative mean normalisation in place.
        let d = &mut self.diff;
        d[0] = 1.0;
        let mut running = 0.0f32;
        for tau in 1..=tau_max {
            let mut acc = 0.0f32;
            for j in 0..w {
                let e = x[j] - x[j + tau];
                acc += e * e;
            }
            running += acc;
            d[tau] = if running > 1e-12 {
                acc * tau as f32 / running
            } else {
                1.0
            };
        }

        // First dip under the threshold, followed down to its minimum.
        let thr = self.settings.threshold;
        let mut found = None;
        let mut tau = tau_min;
        while tau < tau_max {
            if d[tau] < thr {
                while tau + 1 < tau_max && d[tau + 1] < d[tau] {
                    tau += 1;
                }
                found = Some(tau);
                break;
            }
            tau += 1;
        }
        let tau = found.unwrap_or_else(|| {
            (tau_min..tau_max)
                .min_by(|&a, &b| d[a].total_cmp(&d[b]))
                .unwrap_or(tau_min)
        });
        let confidence = (1.0 - d[tau]).clamp(0.0, 1.0);

        // Parabola through the dip and its neighbours.
        let mut t = tau as f32;
        if tau > 1 && tau < tau_max {
            let (a, b, c) = (d[tau - 1], d[tau], d[tau + 1]);
            let den = a - 2.0 * b + c;
            if den.abs() > 1e-9 {
                t += (0.5 * (a - c) / den).clamp(-0.5, 0.5);
            }
        }
        let hz = self.fs_a / t;
        let s = &self.settings;
        let voiced = found.is_some()
            && level >= s.level_floor
            && hz >= s.min_hz * 0.97
            && hz <= s.max_hz * 1.03;
        Estimate {
            hz,
            voiced,
            confidence,
            level,
        }
    }

    pub fn reset(&mut self) {
        self.hp.reset();
        for f in &mut self.lp {
            f.reset();
        }
        self.ring.fill(0.0);
        self.phase = 0;
        self.until = self.hop;
        self.last = Estimate::default();
    }
}

/// How many recent voiced estimates the median looks at.
const MEDIAN: usize = 5;
/// An exact octave jump must persist this long before it is believed.
const OCTAVE_HOLD_S: f32 = 0.04;
/// Voiced must persist this long to switch on, unvoiced this long to switch off.
const VOICED_ON_S: f32 = 0.005;
const VOICED_OFF_S: f32 = 0.012;
/// After this much unvoiced time the pitch history is forgotten (a new phrase may start
/// anywhere).
const FORGET_S: f32 = 0.12;

/// The smoothed result of [`PitchTracker`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tracked {
    pub hz: f32,
    pub voiced: bool,
    pub confidence: f32,
    pub level: f32,
}

impl Tracked {
    /// Pitch as a (fractional) MIDI note number.
    pub fn midi(&self) -> f32 {
        hz_to_midi(self.hz)
    }
}

pub struct PitchTracker {
    hist: [f32; MEDIAN],
    count: usize,
    next: usize,
    /// Smoothed pitch, log2(Hz); `None` when there is no recent voiced history.
    tracked: Option<f32>,
    octave_run: u32,
    voiced_run: u32,
    unvoiced_run: u32,
    voiced: bool,
    on_frames: u32,
    off_frames: u32,
    octave_frames: u32,
    forget_frames: u32,
    out: Tracked,
}

impl PitchTracker {
    /// `hop_s`: time between estimates.
    pub fn new(hop_s: f32) -> Self {
        let frames = |s: f32| ((s / hop_s).round() as u32).max(1);
        Self {
            hist: [0.0; MEDIAN],
            count: 0,
            next: 0,
            tracked: None,
            octave_run: 0,
            voiced_run: 0,
            unvoiced_run: 0,
            voiced: false,
            on_frames: frames(VOICED_ON_S),
            off_frames: frames(VOICED_OFF_S),
            octave_frames: frames(OCTAVE_HOLD_S),
            forget_frames: frames(FORGET_S),
            out: Tracked::default(),
        }
    }

    pub fn update(&mut self, e: Estimate) -> Tracked {
        if e.voiced {
            let mut l = e.hz.log2();
            if let Some(t) = self.tracked {
                let jump = l - t;
                let octaves = jump.round();
                if octaves != 0.0 && (jump - octaves).abs() < 0.08 {
                    self.octave_run += 1;
                    if self.octave_run < self.octave_frames {
                        l -= octaves; // probably an octave error: fold it back
                    } else {
                        // It persisted: a real jump. Start the history over at the new pitch.
                        self.count = 0;
                        self.octave_run = 0;
                    }
                } else {
                    self.octave_run = 0;
                }
            }
            self.hist[self.next] = l;
            self.next = (self.next + 1) % MEDIAN;
            self.count = (self.count + 1).min(MEDIAN);
            self.tracked = Some(self.median());
            self.voiced_run += 1;
            self.unvoiced_run = 0;
            if self.voiced_run >= self.on_frames {
                self.voiced = true;
            }
        } else {
            self.unvoiced_run += 1;
            self.voiced_run = 0;
            if self.unvoiced_run >= self.off_frames {
                self.voiced = false;
            }
            if self.unvoiced_run >= self.forget_frames {
                self.tracked = None;
                self.count = 0;
                self.octave_run = 0;
            }
        }
        self.out = Tracked {
            hz: self.tracked.map_or(0.0, f32::exp2),
            voiced: self.voiced && self.tracked.is_some(),
            confidence: e.confidence,
            level: e.level,
        };
        self.out
    }

    pub fn last(&self) -> Tracked {
        self.out
    }

    fn median(&self) -> f32 {
        // The history holds the newest `count` values (in ring order; order does not matter).
        let mut v = [0.0f32; MEDIAN];
        let (start, n) = if self.count < MEDIAN {
            ((self.next + MEDIAN - self.count) % MEDIAN, self.count)
        } else {
            (0, MEDIAN)
        };
        for (i, slot) in v.iter_mut().take(n).enumerate() {
            *slot = self.hist[(start + i) % MEDIAN];
        }
        let v = &mut v[..n];
        v.sort_unstable_by(f32::total_cmp);
        v[n / 2]
    }

    pub fn reset(&mut self) {
        *self = Self {
            on_frames: self.on_frames,
            off_frames: self.off_frames,
            octave_frames: self.octave_frames,
            forget_frames: self.forget_frames,
            ..Self::new(1.0)
        };
    }
}

pub fn hz_to_midi(hz: f32) -> f32 {
    69.0 + 12.0 * (hz.max(1e-3) / 440.0).log2()
}

pub fn midi_to_hz(m: f32) -> f32 {
    440.0 * ((m - 69.0) / 12.0).exp2()
}

/// "C4", "F#3", ... for the nearest note.
pub fn note_name(midi: f32) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let m = midi.round() as i32;
    format!("{}{}", NAMES[m.rem_euclid(12) as usize], m.div_euclid(12) - 1)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::f32::consts::PI;

    /// Deterministic white noise in -1..1.
    pub fn noise(seed: u32) -> impl FnMut() -> f32 {
        let mut s = seed;
        move || {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            (s >> 8) as f32 / (1u32 << 23) as f32 - 1.0
        }
    }

    /// Runs `seconds` of `signal(i)` through a detector and returns the estimates.
    fn run(
        fs: f32,
        settings: DetectorSettings,
        seconds: f32,
        mut signal: impl FnMut(usize) -> f32,
    ) -> Vec<Estimate> {
        let mut d = PitchDetector::new(fs, settings);
        (0..(seconds * fs) as usize)
            .filter_map(|i| d.push(signal(i)))
            .collect()
    }

    /// Median pitch of the voiced estimates in the second half, and the voiced fraction there.
    fn summarize(est: &[Estimate]) -> (f32, f32) {
        let tail = &est[est.len() / 2..];
        let mut hz: Vec<f32> = tail.iter().filter(|e| e.voiced).map(|e| e.hz).collect();
        let voiced = hz.len() as f32 / tail.len() as f32;
        hz.sort_by(f32::total_cmp);
        (hz.get(hz.len() / 2).copied().unwrap_or(0.0), voiced)
    }

    fn saw(freq: f32, fs: f32) -> impl FnMut(usize) -> f32 {
        let mut phase = 0.0f32;
        move |_| {
            phase = (phase + freq / fs).fract();
            0.3 * (2.0 * phase - 1.0)
        }
    }

    #[test]
    fn sines_and_sawtooths_are_measured_within_one_percent() {
        for fs in [48_000.0, 44_100.0] {
            for freq in [75.0, 98.0, 131.0, 196.0, 262.0, 349.0, 440.0, 490.0] {
                let sine = run(fs, DetectorSettings::default(), 0.5, |i| {
                    0.3 * (2.0 * PI * freq * i as f32 / fs).sin()
                });
                let sawtooth = run(fs, DetectorSettings::default(), 0.5, saw(freq, fs));
                for (kind, est) in [("sine", sine), ("saw", sawtooth)] {
                    let (hz, voiced) = summarize(&est);
                    assert!(voiced > 0.95, "{fs} {kind} {freq}: voiced only {voiced}");
                    let err = (hz - freq).abs() / freq;
                    assert!(err < 0.01, "{fs} {kind} {freq} Hz: measured {hz} ({err:.4})");
                }
            }
        }
    }

    #[test]
    fn confidence_is_high_for_tones_and_low_for_noise() {
        let fs = 48_000.0;
        let tone = run(fs, DetectorSettings::default(), 0.3, saw(150.0, fs));
        let mut n = noise(7);
        let hiss = run(fs, DetectorSettings::default(), 0.3, |_| 0.3 * n());
        let mean = |e: &[Estimate]| e.iter().map(|e| e.confidence).sum::<f32>() / e.len() as f32;
        assert!(mean(&tone[10..]) > 0.9, "tone {}", mean(&tone[10..]));
        assert!(mean(&hiss[10..]) < 0.6, "noise {}", mean(&hiss[10..]));
    }

    #[test]
    fn noise_silence_and_whispers_are_unvoiced() {
        let fs = 48_000.0;
        let s = DetectorSettings::default();
        let mut n = noise(1);
        let (_, white) = summarize(&run(fs, s, 0.5, |_| 0.3 * n()));
        assert!(white < 0.02, "white noise voiced {white}");
        let (_, silence) = summarize(&run(fs, s, 0.5, |_| 0.0));
        assert_eq!(silence, 0.0);
        // A "whisper": noise through a vocal-tract-like resonance at 600 Hz. Breathy and
        // coloured, but without a period.
        let mut f = Biquad::new(Coeffs::peaking(fs, 600.0, 3.0, 18.0));
        let mut n = noise(2);
        let (_, whisper) = summarize(&run(fs, s, 0.5, |_| 0.05 * f.process(n())));
        assert!(whisper < 0.05, "whisper voiced {whisper}");
        // A pitched tone far below the level floor (-80 dBFS) is not a voice either.
        let (_, faint) = summarize(&run(fs, s, 0.5, |i| {
            1e-4 * (2.0 * PI * 200.0 * i as f32 / fs).sin()
        }));
        assert_eq!(faint, 0.0);
    }

    #[test]
    fn a_tone_in_light_noise_is_still_found() {
        let fs = 48_000.0;
        let mut n = noise(3);
        let mut s = saw(180.0, fs);
        let est = run(fs, DetectorSettings::default(), 0.5, |i| s(i) + 0.02 * n());
        let (hz, voiced) = summarize(&est);
        assert!(voiced > 0.9, "voiced {voiced}");
        assert!((hz - 180.0).abs() / 180.0 < 0.01, "{hz}");
    }

    #[test]
    fn range_limits_are_respected() {
        let fs = 48_000.0;
        let narrow = DetectorSettings {
            min_hz: 150.0,
            max_hz: 400.0,
            ..Default::default()
        };
        let (_, voiced) = summarize(&run(fs, narrow, 0.5, saw(100.0, fs)));
        assert!(voiced < 0.05, "100 Hz is out of a 150-400 Hz range: {voiced}");
        let (hz, voiced) = summarize(&run(fs, narrow, 0.5, saw(300.0, fs)));
        assert!(voiced > 0.95 && (hz - 300.0).abs() < 3.0);
    }

    #[test]
    fn short_windows_raise_the_lowest_pitch_and_cost_less_delay() {
        let mut d = PitchDetector::new(48_000.0, DetectorSettings::default());
        assert!(d.lowest_hz() <= 70.0, "default window covers 70 Hz");
        d.configure(DetectorSettings {
            window_s: 0.02,
            ..Default::default()
        });
        assert!(d.lowest_hz() > 70.0);
        assert!(d.window_samples() < 1000);
    }

    #[test]
    fn detection_follows_a_change_quickly() {
        // 200 Hz then 300 Hz: the estimate settles on the new pitch within the window length.
        let fs = 48_000.0;
        let mut a = saw(200.0, fs);
        let mut b = saw(300.0, fs);
        let switch = (0.3 * fs) as usize;
        let mut d = PitchDetector::new(fs, DetectorSettings::default());
        let mut settled_at = None;
        for i in 0..(0.6 * fs) as usize {
            let x = if i < switch { a(i) } else { b(i) };
            if let Some(e) = d.push(x)
                && i > switch
                && settled_at.is_none()
                && (e.hz - 300.0).abs() < 3.0
            {
                settled_at = Some(i - switch);
            }
        }
        let ms = settled_at.expect("never settled") as f32 / fs * 1000.0;
        assert!(ms < 30.0, "took {ms} ms");
    }

    #[test]
    fn tracker_rejects_brief_octave_errors_and_accepts_real_jumps() {
        let mut t = PitchTracker::new(HOP_S);
        let est = |hz| Estimate {
            hz,
            voiced: true,
            confidence: 0.9,
            level: 0.1,
        };
        for _ in 0..20 {
            t.update(est(150.0));
        }
        // Two frames an octave up (a classic detector slip): the output stays at 150 Hz.
        for _ in 0..2 {
            let out = t.update(est(300.0));
            assert!((out.hz - 150.0).abs() < 1.0, "{}", out.hz);
        }
        let out = t.update(est(75.0));
        assert!((out.hz - 150.0).abs() < 1.0, "octave down slip: {}", out.hz);
        // A real jump of an octave that stays: accepted after the hold time.
        let mut last = 0.0;
        for _ in 0..40 {
            last = t.update(est(300.0)).hz;
        }
        assert!((last - 300.0).abs() < 1.0, "{last}");
        // A non-octave move is followed directly (through the median).
        let mut last = 0.0;
        for _ in 0..4 {
            last = t.update(est(250.0)).hz;
        }
        assert!((last - 250.0).abs() < 1.0, "{last}");
    }

    #[test]
    fn tracker_median_removes_single_outliers_and_voicing_has_hysteresis() {
        let mut t = PitchTracker::new(HOP_S);
        let e = |hz, voiced| Estimate {
            hz,
            voiced,
            confidence: 0.9,
            level: 0.1,
        };
        assert!(!t.update(e(200.0, true)).voiced, "one frame is not enough");
        for _ in 0..5 {
            t.update(e(200.0, true));
        }
        assert!(t.last().voiced);
        let out = t.update(e(260.0, true)); // a single wild frame
        assert!((out.hz - 200.0).abs() < 1.0);
        // A single unvoiced frame (a glitch) keeps it voiced at the same pitch.
        let out = t.update(e(0.0, false));
        assert!(out.voiced && (out.hz - 200.0).abs() < 1.0);
        for _ in 0..10 {
            t.update(e(0.0, false));
        }
        assert!(!t.last().voiced);
    }

    #[test]
    fn note_names_and_conversions() {
        assert_eq!(note_name(60.0), "C4");
        assert_eq!(note_name(57.2), "A3");
        assert_eq!(note_name(70.0), "A#4");
        assert!((midi_to_hz(69.0) - 440.0).abs() < 1e-3);
        assert!((hz_to_midi(261.63) - 60.0).abs() < 0.01);
    }

    #[test]
    fn analysis_is_cheap() {
        let fs = 48_000.0;
        let seconds = 5.0;
        let mut d = PitchDetector::new(fs, DetectorSettings::default());
        let mut s = saw(140.0, fs);
        let start = std::time::Instant::now();
        let mut voiced = 0;
        for i in 0..(seconds * fs) as usize {
            if d.push(s(i)).is_some_and(|e| e.voiced) {
                voiced += 1;
            }
        }
        let load = start.elapsed().as_secs_f32() / seconds;
        println!("pitch detector CPU load: {:.2}% of one core", load * 100.0);
        assert!(voiced > 1000);
        assert!(load < 0.05, "detector uses {:.1}% of real time", load * 100.0);
    }
}
