//! Melody Voice: retunes the speaking voice, note by note, to a melody (or to a scale).
//!
//! ```text
//! voice ─▶ pitch detector ─▶ tracker ─▶ conductor (which note?) ─▶ shift = target − you
//!   │                                                                  │
//!   └─▶ [lookahead] ─┬─▶ pitch stage (shift; formants kept) ─────┐ voiced
//!                    └─▶ [delay = pitch stage latency] ──────────┴─▶ out
//!                                                             unvoiced (s, t, f, breath)
//! ```
//!
//! Unvoiced sounds and pauses take the dry path (delayed by the same amount, so the two paths
//! crossfade without a phase jump): consonants stay crisp and noise is never "tuned".
//!
//! Alignment: an estimate describes the audio of roughly half an analysis window ago, and the
//! pitch stage plays the audio of ~15 ms ago. If the analysis lags more than the shifter, the
//! audio waits (lookahead); if it lags less, the decisions wait (a short queue). Either way the
//! shift is decided from the same stretch of voice it is applied to, and the added delay is the
//! larger of the two, not their sum.

use super::Block;
use super::chain::PitchStage;
use super::conductor::{Conductor, Frame, Position, Settings};
use super::delay::DelayLine;
use super::params::{P, Snapshot};
use super::pitch::{PitchShifter, semitones_to_ratio};
use super::pitch_detect::{
    DetectorSettings, Estimate, PitchDetector, PitchTracker, analysis_delay_for, hz_to_midi,
};
use super::smooth::Smoother;
use crate::audio::block::BLOCK;
use crate::audio::shared::from_db;
use crate::melody::Note;
use crate::melody::store::{MAX_NOTES, MelodyStore};

/// Decision queue length, in blocks.
const CTRL: usize = 64;
/// When the lookahead changes (a new analysis window), the delay moves at most this many
/// samples per sample: a brief, tiny pitch bend instead of a click.
const DELAY_SLEW: f32 = 0.05;
/// Crossfade between the dry (unvoiced) and the retuned (voiced) path.
const GATE_S: f32 = 0.008;
/// The retune never shifts by more than this.
const MAX_SHIFT: f32 = 24.0;
/// Longest lookahead plus pitch-stage latency the input delay line holds.
const MAX_DELAY_S: f32 = 0.1;

/// What Melody Voice is doing, for the display.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MelodyStatus {
    /// Your pitch (MIDI note number), NaN while unvoiced.
    pub detected: f32,
    /// The note you are being tuned to (MIDI), NaN when the voice is left alone.
    pub target: f32,
    pub position: Position,
}

impl Default for MelodyStatus {
    fn default() -> Self {
        Self {
            detected: f32::NAN,
            target: f32::NAN,
            position: Position::default(),
        }
    }
}

/// Detector settings from the parameters. Sensitivity trades "catches quiet, breathy
/// syllables" against "ignores background noise".
pub fn detector_settings(p: &Snapshot) -> DetectorSettings {
    let sens = p.get(P::MelodySensitivity);
    DetectorSettings {
        min_hz: p.get(P::MelodyMinHz),
        max_hz: p.get(P::MelodyMaxHz),
        window_s: p.get(P::MelodyWindow) / 1000.0,
        threshold: 0.12 + 0.2 * sens,
        level_floor: from_db(-30.0 - 30.0 * sens),
    }
}

/// Delay Melody Voice adds with these settings, in samples: the larger of the analysis delay
/// and the pitch stage's (see the module docs). What [`MelodyVoice::latency`] reports.
pub fn latency_samples(p: &Snapshot, fs: f32) -> usize {
    let a = analysis_delay_for(fs, &detector_settings(p));
    a.max(PitchShifter::latency_for(fs))
}

/// [`latency_samples`] in ms at output rate `rate` (48 kHz if not known yet).
pub fn latency_ms(p: &Snapshot, rate: u32) -> f32 {
    let fs = if rate == 0 { 48_000.0 } else { rate as f32 };
    latency_samples(p, fs) as f32 * 1000.0 / fs
}

pub struct MelodyVoice {
    fs: f32,
    det: PitchDetector,
    det_cfg: DetectorSettings,
    tracker: PitchTracker,
    conductor: Conductor,
    /// The melody (copied from the shared store; never reallocated).
    notes: Vec<Note>,
    /// Store version the notes were copied from.
    seen: Option<u32>,
    stage: PitchStage,
    stage_latency: usize,
    input: DelayLine,
    /// Lookahead (samples) and where it is gliding to.
    pre: f32,
    pre_target: f32,
    /// Decisions (shift, voiced gate) per block, read `ctrl_delay` blocks later.
    ctrl: [(f32, f32); CTRL],
    ctrl_pos: usize,
    ctrl_delay: usize,
    want_shift: f32,
    want_gate: f32,
    /// Target pitch after the note-to-note glide.
    glided: Option<f32>,
    glide_k: f32,
    shift: Smoother,
    gate: Smoother,
    gate_prev: f32,
    mix: Smoother,
    status: MelodyStatus,
}

impl MelodyVoice {
    pub fn new(fs: f32) -> Self {
        let p = Snapshot::defaults();
        let det_cfg = detector_settings(&p);
        let det = PitchDetector::new(fs, det_cfg);
        let tracker_hop = det.hop_s();
        let tracker = PitchTracker::new(tracker_hop);
        let stage = PitchStage::new(fs);
        let stage_latency = stage.latency();
        let mut m = Self {
            fs,
            det,
            det_cfg,
            tracker,
            conductor: Conductor::new(tracker_hop),
            notes: Vec::with_capacity(MAX_NOTES),
            seen: None,
            stage,
            stage_latency,
            input: DelayLine::new((MAX_DELAY_S * fs) as usize),
            pre: 0.0,
            pre_target: 0.0,
            ctrl: [(0.0, 0.0); CTRL],
            ctrl_pos: 0,
            ctrl_delay: 0,
            want_shift: 0.0,
            want_gate: 0.0,
            glided: None,
            glide_k: 1.0,
            shift: Smoother::new(fs, 0.015, 0.0),
            gate: Smoother::new(fs, GATE_S, 0.0),
            gate_prev: 0.0,
            mix: Smoother::new(fs, 0.02, 1.0),
            status: MelodyStatus::default(),
        };
        m.align();
        m.pre = m.pre_target;
        m
    }

    /// Lookahead and decision delay for the current analysis window (see the module docs).
    fn align(&mut self) {
        let a = self.det.analysis_delay();
        let l = self.stage_latency;
        self.pre_target = a.saturating_sub(l) as f32;
        self.ctrl_delay = (l.saturating_sub(a) as f32 / BLOCK as f32).round() as usize;
        self.ctrl_delay = self.ctrl_delay.min(CTRL - 1);
    }

    /// Delay this effect adds, in samples.
    pub fn latency(&self) -> usize {
        self.pre_target as usize + self.stage_latency
    }

    pub fn status(&self) -> MelodyStatus {
        self.status
    }

    /// Picks up a newly published melody (cheap when nothing changed; never allocates).
    pub fn sync(&mut self, store: &MelodyStore) {
        if self.seen == Some(store.version()) {
            return;
        }
        if let Some(v) = store.read_into(&mut self.notes) {
            self.seen = Some(v);
            self.conductor.set_melody(&self.notes);
            self.status.position = self.conductor.position();
        }
    }

    /// Back to the first note.
    pub fn restart(&mut self) {
        self.conductor.restart();
        self.status.position = self.conductor.position();
    }

    fn configure(&mut self, p: &Snapshot) {
        let cfg = detector_settings(p);
        if cfg != self.det_cfg {
            self.det.configure(cfg);
            self.det_cfg = cfg;
            self.align();
        }
        let glide_s = p.get(P::MelodyGlide) / 1000.0;
        self.glide_k = if glide_s <= 0.0 {
            1.0
        } else {
            1.0 - (-self.det.hop_s() / glide_s).exp()
        };
        self.shift
            .set_time(self.fs, p.get(P::MelodyRetune) / 1000.0);
        self.stage
            .set_formant_warp(semitones_to_ratio(p.get(P::MelodyFormantShift)));
    }

    /// One analysis frame: where should the voice be?
    fn on_frame(&mut self, e: Estimate, s: &Settings) {
        let t = self.tracker.update(e);
        let detected = t.voiced.then(|| hz_to_midi(t.hz));
        let target = self.conductor.frame(
            Frame {
                detected,
                level: t.level,
            },
            s,
            &self.notes,
        );
        self.status = MelodyStatus {
            detected: detected.unwrap_or(f32::NAN),
            target: target.unwrap_or(f32::NAN),
            position: self.conductor.position(),
        };
        match (detected, target) {
            (Some(m), Some(target)) => {
                // Glide from note to note while the voice keeps going; a fresh syllable
                // starts right on its note.
                let g = match self.glided {
                    Some(g) if self.want_gate > 0.5 => g + (target - g) * self.glide_k,
                    _ => target,
                };
                self.glided = Some(g);
                self.want_shift = (g - m).clamp(-MAX_SHIFT, MAX_SHIFT);
                self.want_gate = 1.0;
            }
            _ => {
                self.glided = None;
                self.want_gate = 0.0;
            }
        }
    }

    pub fn process(&mut self, block: &mut Block, p: &Snapshot) {
        self.configure(p);
        let s = Settings::from_params(p);
        for &x in block.iter() {
            if let Some(e) = self.det.push(x) {
                self.on_frame(e, &s);
            }
        }

        self.ctrl[self.ctrl_pos] = (self.want_shift, self.want_gate);
        let (shift_t, gate_t) = self.ctrl[(self.ctrl_pos + CTRL - self.ctrl_delay) % CTRL];
        self.ctrl_pos = (self.ctrl_pos + 1) % CTRL;
        if gate_t > 0.5 && self.gate_prev <= 0.5 {
            // The retuned path was silent until now: start right on the new shift.
            self.shift.set_value(shift_t);
        }
        self.gate_prev = gate_t;

        let l = self.stage_latency as f32;
        let mut wet = [0.0; BLOCK];
        let mut dry = [0.0; BLOCK];
        let mut ratio = [1.0; BLOCK];
        for i in 0..BLOCK {
            self.input.push(block[i]);
            self.pre += (self.pre_target - self.pre).clamp(-DELAY_SLEW, DELAY_SLEW);
            wet[i] = self.input.read(1.0 + self.pre);
            dry[i] = self.input.read(1.0 + self.pre + l);
            ratio[i] = semitones_to_ratio(self.shift.next(shift_t));
        }
        let formant = if p.on(P::MelodyFormant) { 1.0 } else { 0.0 };
        self.stage.run(&mut wet, &ratio, formant);
        let mix = p.get(P::MelodyMix);
        for ((out, w), d) in block.iter_mut().zip(wet).zip(dry) {
            let g = self.gate.next(gate_t) * self.mix.next(mix);
            *out = d + (w - d) * g;
        }
    }

    /// Clears the audio state (the melody position is kept).
    pub fn reset(&mut self) {
        self.det.reset();
        self.tracker.reset();
        self.stage.reset();
        self.input.reset();
        self.ctrl = [(0.0, 0.0); CTRL];
        self.want_gate = 0.0;
        self.want_shift = 0.0;
        self.glided = None;
        self.gate.set_value(0.0);
        self.gate_prev = 0.0;
        self.shift.set_value(0.0);
        self.status = MelodyStatus {
            position: self.conductor.position(),
            ..MelodyStatus::default()
        };
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::dsp::pitch_detect::midi_to_hz;

    pub const FS: f32 = 48_000.0;

    /// Measures the pitch of `signal` (median of the voiced frames from `skip_s` on).
    pub fn measured_midi(signal: &[f32], skip_s: f32) -> Option<f32> {
        let mut d = PitchDetector::new(FS, DetectorSettings::default());
        let mut m: Vec<f32> = signal
            .iter()
            .enumerate()
            .filter_map(|(i, &x)| d.push(x).filter(|_| i as f32 >= skip_s * FS))
            .filter(|e| e.voiced)
            .map(|e| hz_to_midi(e.hz))
            .collect();
        if m.len() < 5 {
            return None;
        }
        m.sort_by(f32::total_cmp);
        Some(m[m.len() / 2])
    }

    /// A buzzy, vowel-like "voice": a sawtooth whose pitch follows `hz(t)`.
    pub fn voice(seconds: f32, mut hz: impl FnMut(f32) -> f32) -> Vec<f32> {
        let mut phase = 0.0f32;
        (0..(seconds * FS) as usize)
            .map(|i| {
                phase = (phase + hz(i as f32 / FS) / FS).fract();
                0.2 * (2.0 * phase - 1.0)
            })
            .collect()
    }

    pub fn run(m: &mut MelodyVoice, p: &Snapshot, input: &[f32]) -> Vec<f32> {
        let mut out = Vec::with_capacity(input.len());
        for chunk in input.as_chunks::<BLOCK>().0 {
            let mut b: Block = *chunk;
            m.process(&mut b, p);
            out.extend_from_slice(&b);
        }
        out
    }

    pub fn snap_params(key: f32, scale: f32) -> Snapshot {
        let mut p = Snapshot::defaults();
        p.set(P::MelodyOn, 1.0);
        p.set(P::MelodyMode, 3.0);
        p.set(P::MelodyKey, key);
        p.set(P::MelodyScale, scale);
        p.set(P::MelodyRetune, 0.0);
        p
    }

    #[test]
    fn scale_snap_pulls_an_off_key_voice_onto_the_scale() {
        // 150 Hz is D3 + 0.37 semitones (50.37). In C major the nearest note is D3 (50);
        // in C minor pentatonic (C Eb F G Bb) it is Eb3 (51).
        let input = voice(1.5, |_| 150.0);
        for (scale, want) in [(0.0, 50.0), (3.0, 51.0)] {
            for formant in [0.0, 1.0] {
                let mut p = snap_params(0.0, scale);
                p.set(P::MelodyFormant, formant);
                let mut m = MelodyVoice::new(FS);
                let out = run(&mut m, &p, &input);
                let got = measured_midi(&out, 0.5).expect("output is voiced");
                assert!(
                    (got - want).abs() < 0.15,
                    "scale {scale} formant {formant}: {got:.2}, want {want}"
                );
                assert!((m.status().detected - 50.37).abs() < 0.15);
                assert_eq!(m.status().target, want);
            }
        }
    }

    /// Pitch (MIDI) of every voiced analysis frame of `signal` from `skip_s` on.
    pub fn frame_pitches(signal: &[f32], skip_s: f32) -> Vec<f32> {
        let mut d = PitchDetector::new(FS, DetectorSettings::default());
        signal
            .iter()
            .enumerate()
            .filter_map(|(i, &x)| d.push(x).filter(|_| i as f32 >= skip_s * FS))
            .filter(|e| e.voiced)
            .map(|e| hz_to_midi(e.hz))
            .collect()
    }

    /// Spread between the 5th and 95th percentile.
    fn spread(mut v: Vec<f32>) -> f32 {
        v.sort_by(f32::total_cmp);
        v[v.len() * 95 / 100] - v[v.len() * 5 / 100]
    }

    #[test]
    fn hard_tune_flattens_a_gliding_voice_into_steps() {
        // The voice slides smoothly from 140 to 190 Hz (more than five semitones); hard-tuned
        // to chromatic, the output stays on whole notes except for brief note changes.
        let input = voice(2.0, |t| 140.0 + 25.0 * t);
        let mut m = MelodyVoice::new(FS);
        let out = run(&mut m, &snap_params(0.0, 4.0), &input);
        let pitches = frame_pitches(&out, 0.3);
        let on_note = pitches
            .iter()
            .filter(|m| (*m - m.round()).abs() < 0.15)
            .count() as f32
            / pitches.len() as f32;
        println!(
            "hard tune on a glide: {:.0}% of frames on a note",
            on_note * 100.0
        );
        assert!(pitches.len() > 500);
        // The rest are frames whose 30 ms window straddles a note change.
        assert!(on_note > 0.8, "only {:.0}% on a note", on_note * 100.0);
        // ...and it really moves through the notes instead of sticking to one.
        let first = pitches[..50].iter().sum::<f32>() / 50.0;
        let last = pitches[pitches.len() - 50..].iter().sum::<f32>() / 50.0;
        assert!(last - first > 4.0, "{first:.1} -> {last:.1}");
    }

    #[test]
    fn natural_retune_lets_vibrato_through_and_hard_tune_removes_it() {
        // 6 Hz vibrato of +-0.4 semitones around A3.
        let a3 = midi_to_hz(57.0);
        let input = voice(2.0, |t| {
            a3 * ((0.4 / 12.0) * (2.0 * 6.0 * std::f32::consts::PI * t).sin()).exp2()
        });
        let wobble = |retune: f32| {
            let mut p = snap_params(0.0, 4.0);
            p.set(P::MelodyRetune, retune);
            let mut m = MelodyVoice::new(FS);
            spread(frame_pitches(&run(&mut m, &p, &input), 0.5))
        };
        let input_wobble = spread(frame_pitches(&input, 0.5));
        let hard = wobble(0.0);
        let natural = wobble(250.0);
        println!("vibrato spread: input {input_wobble:.2} hard {hard:.2} natural {natural:.2}");
        assert!(
            input_wobble > 0.7,
            "the test vibrato is there: {input_wobble:.2}"
        );
        // A fast, wide vibrato (15 semitones/s at its steepest), measured every 2.5 ms: hard
        // tune must remove most of it, natural must keep nearly all of it.
        assert!(
            hard < 0.4 * input_wobble,
            "hard tune leaves {hard:.2} st of wobble"
        );
        assert!(
            natural > 0.75 * input_wobble,
            "natural keeps the vibrato: {natural:.2} st"
        );
    }

    #[test]
    fn unvoiced_sounds_pass_through_untouched() {
        // Noise (an "s") must come out exactly as it went in, only delayed.
        let mut n = crate::dsp::pitch_detect::tests::noise(5);
        let input: Vec<f32> = (0..(1.0 * FS) as usize).map(|_| 0.1 * n()).collect();
        let mut m = MelodyVoice::new(FS);
        let out = run(&mut m, &snap_params(0.0, 0.0), &input);
        let lat = m.latency();
        for i in (0.2 * FS) as usize..out.len() {
            assert!((out[i] - input[i - lat]).abs() < 1e-5, "sample {i}");
        }
    }

    #[test]
    fn latency_is_the_larger_of_analysis_and_shifter_not_the_sum() {
        let mut m = MelodyVoice::new(FS);
        let shifter = m.stage_latency;
        assert!(
            m.latency() < shifter + 200,
            "default window: {}",
            m.latency()
        );
        let mut p = snap_params(0.0, 0.0);
        p.set(P::MelodyWindow, 60.0);
        m.configure(&p);
        assert!(m.latency() > shifter + 400, "long window needs lookahead");
        assert!(m.latency() < (0.04 * FS) as usize);
        // The display's calculation agrees with the audio thread, at any rate.
        for fs in [44_100.0, 48_000.0, 96_000.0] {
            for window in [20.0, 30.0, 60.0] {
                p.set(P::MelodyWindow, window);
                let mut m = MelodyVoice::new(fs);
                m.configure(&p);
                assert_eq!(latency_samples(&p, fs), m.latency(), "{fs} {window}");
            }
        }
        assert_eq!(
            PitchShifter::latency_for(FS),
            PitchShifter::new(FS).latency()
        );
    }

    #[test]
    fn measured_delay_matches_the_reported_latency() {
        // A burst of noise after silence: find where it starts in the output.
        let mut n = crate::dsp::pitch_detect::tests::noise(9);
        let start = (0.3 * FS) as usize;
        let input: Vec<f32> = (0..FS as usize)
            .map(|i| if i >= start { 0.2 * n() } else { 0.0 })
            .collect();
        for window in [20.0, 30.0, 45.0, 60.0] {
            let mut p = snap_params(0.0, 0.0);
            p.set(P::MelodyWindow, window);
            let mut m = MelodyVoice::new(FS);
            m.configure(&p);
            m.pre = m.pre_target;
            let out = run(&mut m, &p, &input);
            let onset = out.iter().position(|x| x.abs() > 1e-4).unwrap() - start;
            println!(
                "Melody Voice, {window} ms window: measured added latency {:.1} ms",
                onset as f32 * 1000.0 / FS
            );
            assert!(
                onset.abs_diff(m.latency()) <= 2,
                "{onset} vs {}",
                m.latency()
            );
        }
    }

    /// Syllables: 150 ms of tone, 100 ms of quiet noise, each syllable a bit higher.
    pub fn syllables(seconds: f32, saw: bool) -> Vec<f32> {
        let mut n = crate::dsp::pitch_detect::tests::noise(11);
        let mut phase = 0.0f32;
        (0..(seconds * FS) as usize)
            .map(|i| {
                let t = i as f32 / FS;
                let k = (t / 0.25) as usize;
                let u = t % 0.25;
                if u < 0.15 {
                    phase = (phase + (120.0 + 15.0 * (k % 6) as f32) / FS).fract();
                    // Soft edges, like a real syllable.
                    let env = (u / 0.01).min((0.15 - u) / 0.01).min(1.0);
                    let w = if saw {
                        2.0 * phase - 1.0
                    } else {
                        (phase * std::f32::consts::TAU).sin()
                    };
                    0.2 * env * w
                } else {
                    0.02 * n()
                }
            })
            .collect()
    }

    fn max_step(x: &[f32]) -> f32 {
        x.windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max)
    }

    #[test]
    fn switching_between_voiced_and_unvoiced_never_clicks() {
        // A pure tone is the hardest case for the formant path (LPC sees one sharp peak).
        for saw in [false, true] {
            for formant in [0.0, 1.0] {
                let input = syllables(2.0, saw);
                let mut p = snap_params(0.0, 0.0);
                p.set(P::MelodyFormant, formant);
                let mut m = MelodyVoice::new(FS);
                let out = run(&mut m, &p, &input);
                let (step_in, step) = (max_step(&input), max_step(&out));
                assert!(
                    step < step_in * 1.2 + 0.01,
                    "saw={saw} formant={formant}: step {step} (input {step_in})"
                );
                assert!(out.iter().all(|x| x.is_finite() && x.abs() < 0.5));
            }
        }
    }

    /// A synthetic speaker: `count` syllables of buzz through two vowel formants, each one
    /// starting at its own pitch around 125 Hz and falling 3 semitones (like speech), with
    /// quiet breath noise in the pauses. Returns the audio and each syllable's (start, end).
    pub fn speech(count: usize, voiced_s: f32, pause_s: f32) -> (Vec<f32>, Vec<(f32, f32)>) {
        use crate::dsp::biquad::{Biquad, Coeffs};
        let mut f1 = Biquad::new(Coeffs::peaking(FS, 650.0, 4.0, 14.0));
        let mut f2 = Biquad::new(Coeffs::peaking(FS, 1100.0, 4.0, 10.0));
        let mut noise = crate::dsp::pitch_detect::tests::noise(21);
        let start_pitches = [126.0, 118.0, 133.0, 121.0, 129.0, 115.0, 124.0, 131.0];
        let (mut out, mut spans) = (Vec::new(), Vec::new());
        let mut phase = 0.0f32;
        for k in 0..count {
            let t0 = out.len() as f32 / FS;
            spans.push((t0, t0 + voiced_s));
            let n = (voiced_s * FS) as usize;
            for i in 0..n {
                let u = i as f32 / n as f32;
                let hz = start_pitches[k % start_pitches.len()] * (-3.0 * u / 12.0).exp2();
                phase = (phase + hz / FS).fract();
                let env = (i as f32 / (0.015 * FS))
                    .min((n - i) as f32 / (0.02 * FS))
                    .min(1.0);
                out.push(0.15 * env * f2.process(f1.process(2.0 * phase - 1.0)));
            }
            for _ in 0..(pause_s * FS) as usize {
                out.push(0.003 * noise());
            }
        }
        (out, spans)
    }

    /// Median output pitch of each syllable, measured in its steady middle part.
    pub fn sung_notes(out: &[f32], spans: &[(f32, f32)], latency: usize) -> Vec<Option<f32>> {
        let lat = latency as f32 / FS;
        spans
            .iter()
            .map(|&(a, b)| {
                let from = ((a + lat + 0.06) * FS) as usize;
                let to = (((b + lat - 0.02) * FS) as usize).min(out.len());
                let p = frame_pitches(&out[from.saturating_sub((0.03 * FS) as usize)..to], 0.03);
                if p.len() < 5 {
                    return None;
                }
                let mut p = p;
                p.sort_by(f32::total_cmp);
                Some(p[p.len() / 2])
            })
            .collect()
    }

    #[test]
    fn talking_through_the_full_chain_sings_twinkle_twinkle_one_syllable_per_note() {
        use crate::dsp::VoiceProcessor;
        use crate::dsp::chain::VoiceChain;
        use crate::melody::{library, store::MelodyStore};
        let store = MelodyStore::default();
        let twinkle = library::get("Twinkle Twinkle Little Star").unwrap();
        store.publish(&twinkle.notes);
        let mut p = Snapshot::defaults();
        p.set(P::MelodyOn, 1.0); // per syllable, octave fold on: the defaults

        let (input, spans) = speech(12, 0.22, 0.13);
        let mut chain = VoiceChain::new(FS);
        chain.melody().sync(&store);
        let mut out = Vec::with_capacity(input.len());
        for chunk in input.as_chunks::<BLOCK>().0 {
            let mut b = *chunk;
            chain.run(&mut b, &p, true);
            out.extend_from_slice(&b);
        }
        let latency = chain.latency_samples();
        // The voice sits around B2 (MIDI 47), the tune around E4 (64): sung an octave down.
        let want: Vec<f32> = twinkle.notes[..12]
            .iter()
            .map(|n| f32::from(n.key) - 12.0)
            .collect();
        let got = sung_notes(&out, &spans, latency);
        println!("wanted {want:?}");
        println!(
            "sung   {:?}",
            got.iter()
                .map(|g| g.map(|g| (g * 10.0).round() / 10.0))
                .collect::<Vec<_>>()
        );
        for (k, (g, w)) in got.iter().zip(&want).enumerate() {
            let g = g.unwrap_or_else(|| panic!("syllable {k} not voiced in the output"));
            assert!(
                (g - w).abs() < 0.25,
                "syllable {k}: sung {g:.2}, wanted {w}"
            );
        }
        assert_eq!(chain.melody().status().position.note, Some(11));
    }

    #[test]
    fn formant_shift_moves_the_timbre_not_the_pitch() {
        use crate::dsp::biquad::{Biquad, Coeffs};
        // A vowel-like buzz (150 Hz sawtooth through formants at 700 and 1200 Hz). Shifting
        // the formants up moves energy above 1.2 kHz; down moves it below. The pitch stays.
        let mut f1 = Biquad::new(Coeffs::peaking(FS, 700.0, 4.0, 15.0));
        let mut f2 = Biquad::new(Coeffs::peaking(FS, 1200.0, 4.0, 12.0));
        let input: Vec<f32> = voice(1.5, |_| 150.0)
            .into_iter()
            .map(|x| 0.4 * f2.process(f1.process(x)))
            .collect();
        let high_share = |shift: f32| {
            let mut p = snap_params(0.0, 0.0);
            p.set(P::MelodyFormantShift, shift);
            let mut m = MelodyVoice::new(FS);
            let out = run(&mut m, &p, &input);
            let got = measured_midi(&out, 0.5).unwrap();
            assert!((got - 50.0).abs() < 0.15, "shift {shift}: pitch {got:.2}");
            let tail = &out[(0.5 * FS) as usize..];
            let mut hp = Biquad::new(Coeffs::highpass(FS, 1200.0, 0.707));
            let e: f32 = tail.iter().map(|x| x * x).sum();
            let h: f32 = tail.iter().map(|&x| hp.process(x).powi(2)).sum();
            h / e
        };
        let (down, flat, up) = (high_share(-5.0), high_share(0.0), high_share(5.0));
        println!("energy above 1.2 kHz: down {down:.3} flat {flat:.3} up {up:.3}");
        assert!(down < flat * 0.8 && up > flat * 1.25, "{down} {flat} {up}");
    }
}
