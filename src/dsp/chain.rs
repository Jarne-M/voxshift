//! The voice chain:
//! input gain -> gate -> melody voice -> pitch -> tone -> distortion -> robot -> chorus -> echo
//! -> reverb
//! -> master dry/wet -> output gain, plus the master bypass.
//!
//! Every section fades in and out (20 ms) instead of switching, and is skipped entirely (and
//! cleared, so no stale echo tail replays later) once it has faded out.

use super::chorus::Chorus;
use super::distortion::Distortion;
use super::echo::Echo;
use super::eq::Tone;
use super::gate::Gate;
use super::lpc::{Colorer, Lpc, Whitener};
use super::melody::MelodyVoice;
use super::params::{P, Snapshot};
use super::pitch::{PitchShifter, semitones_to_ratio};
use super::reverb::Reverb;
use super::robot::Robot;
use super::smooth::{Ramp, Smoother};
use super::{Block, VoiceProcessor};
use crate::audio::block::BLOCK;
use crate::audio::shared::from_db;

const FADE_S: f32 = 0.02;

/// Pitch shifting, optionally formant-preserving (see `lpc`). Both paths always run so the
/// "preserve formants" switch can crossfade without a click.
pub struct PitchStage {
    plain: PitchShifter,
    residual: PitchShifter,
    lpc: Lpc,
    whiten: Whitener,
    color: Colorer,
    semis: Smoother,
    formant: Smoother,
    /// Short-term power of the plain and the formant path, for level matching.
    power: [f32; 2],
    power_coeff: f32,
    match_gain: f32,
    gain_rise: f32,
    /// Peak envelope of the plain path (safety limit for the formant path).
    env: f32,
    env_release: f32,
    /// Gain of the formant path's peak limiter (drops instantly, recovers in ~50 ms).
    limit_gain: f32,
    limit_rise: f32,
    pre_x: f32,
    de_y: f32,
}

/// The formant path may differ in level from the plain one by at most this factor.
const MAX_MATCH: f32 = 4.0;
/// Pre-emphasis flattens the voice's natural treble roll-off before LPC analysis, which keeps
/// the analysis well-conditioned (standard in speech coding); de-emphasis undoes it after.
const PRE_EMPHASIS: f32 = 0.9;
/// The formant path never peaks above this multiple of the plain path's envelope.
const PEAK_LIMIT: f32 = 2.0;

impl PitchStage {
    pub fn new(fs: f32) -> Self {
        Self {
            plain: PitchShifter::new(fs),
            residual: PitchShifter::new(fs),
            lpc: Lpc::new(fs),
            whiten: Whitener::default(),
            color: Colorer::default(),
            semis: Smoother::new(fs, 0.03, 0.0),
            formant: Smoother::new(fs, FADE_S, 0.0),
            power: [0.0; 2],
            power_coeff: 1.0 - (-1.0 / (0.03 * fs)).exp(),
            match_gain: 1.0,
            gain_rise: 1.0 - (-1.0 / (0.03 * fs)).exp(),
            env: 0.0,
            env_release: (-1.0 / (0.05 * fs)).exp(),
            limit_gain: 1.0,
            limit_rise: 1.0 - (-1.0 / (0.05 * fs)).exp(),
            pre_x: 0.0,
            de_y: 0.0,
        }
    }

    pub fn process(&mut self, block: &mut Block, p: &Snapshot) {
        let ratio = semitones_to_ratio(self.semis.next_n(p.get(P::PitchSemis), BLOCK));
        let formant = if p.on(P::PitchFormant) { 1.0 } else { 0.0 };
        self.run(block, &[ratio; BLOCK], formant);
    }

    /// Moves the formants by `warp` (1 = untouched; only in formant-preserving mode).
    pub fn set_formant_warp(&mut self, warp: f32) {
        self.lpc.set_warp(warp);
    }

    /// Shifts by a per-sample `ratio`. `formant`: 1 = formant-preserving, 0 = plain; changes
    /// are crossfaded.
    pub fn run(&mut self, block: &mut Block, ratio: &[f32; BLOCK], formant: f32) {
        for (s, &ratio) in block.iter_mut().zip(ratio.iter()) {
            let x = *s;
            let a = self.plain.process(x, ratio);

            let xp = x - PRE_EMPHASIS * self.pre_x;
            self.pre_x = x;
            self.lpc.push(xp);
            let (k, kc) = self.lpc.coeffs();
            let e = self.whiten.process(xp, k);
            let yp = self.color.process(self.residual.process(e, ratio), kc);
            let b = yp + PRE_EMPHASIS * self.de_y;
            self.de_y = b;

            // Re-colouring a shifted residual can land on a sharp resonance: keep the formant
            // path at the plain path's loudness (gain drops instantly, recovers in ~30 ms)...
            self.power[0] += (a * a - self.power[0]) * self.power_coeff;
            self.power[1] += (b * b - self.power[1]) * self.power_coeff;
            let wanted = ((self.power[0] + 1e-9) / (self.power[1] + 1e-9))
                .sqrt()
                .clamp(1.0 / MAX_MATCH, MAX_MATCH);
            self.match_gain = if wanted < self.match_gain {
                wanted
            } else {
                self.match_gain + (wanted - self.match_gain) * self.gain_rise
            };
            // ...and never let a transient peak above twice the plain path's envelope. A
            // limiter, not a clipper: the gain drops exactly enough at the peak and recovers
            // slowly, so a resonance (a hummed, near-pure tone) gets quieter instead of being
            // squared off into a buzz.
            self.env = a.abs().max(self.env * self.env_release);
            let b = b * self.match_gain;
            let limit = PEAK_LIMIT * self.env + 1e-6;
            let needed = if b.abs() > limit {
                limit / b.abs()
            } else {
                1.0
            };
            self.limit_gain = if needed < self.limit_gain {
                needed
            } else {
                self.limit_gain + (1.0 - self.limit_gain) * self.limit_rise
            };
            let b = b * self.limit_gain;
            *s = a + (b - a) * self.formant.next(formant);
        }
    }

    pub fn latency(&self) -> usize {
        self.plain.latency()
    }

    pub fn reset(&mut self) {
        self.plain.reset();
        self.residual.reset();
        self.lpc.reset();
        self.whiten = Whitener::default();
        self.color = Colorer::default();
        self.power = [0.0; 2];
        self.match_gain = 1.0;
        self.env = 0.0;
        self.limit_gain = 1.0;
        self.pre_x = 0.0;
        self.de_y = 0.0;
    }
}

#[derive(Clone, Copy)]
enum Section {
    Gate,
    Melody,
    Pitch,
    Tone,
    Dist,
    Robot,
    Chorus,
    Echo,
    Reverb,
}

const SECTIONS: usize = 9;

pub struct VoiceChain {
    input_gain: Smoother,
    output_gain: Smoother,
    dry_wet: Smoother,
    fs: f32,
    /// Input to the effects: 1 = effects on, 0 = clean (master toggle off).
    engaged: Ramp,
    /// Input gain for the clean path.
    clean_gain: Smoother,
    /// Switched to clean and the tail has died away: the effects are not run at all.
    resting: bool,
    quiet_blocks: u32,
    faders: [Smoother; SECTIONS],
    idle: [bool; SECTIONS],
    gate: Gate,
    melody: MelodyVoice,
    pitch: PitchStage,
    tone: Tone,
    dist: Distortion,
    robot: Robot,
    chorus: Chorus,
    echo: Echo,
    reverb: Reverb,
}

/// Runs one section with its fade. A macro because each section is a different type and a
/// closure could not borrow the effect and the fader at the same time.
macro_rules! section {
    ($self:ident, $sec:expr, $on:expr, $block:ident, $p:ident, $fx:ident) => {{
        let i = $sec as usize;
        let on: bool = $on;
        if !on && $self.faders[i].settled(0.0) {
            if !$self.idle[i] {
                $self.$fx.reset();
                $self.idle[i] = true;
            }
        } else {
            $self.idle[i] = false;
            let dry = *$block;
            $self.$fx.process($block, $p);
            let fader = &mut $self.faders[i];
            if !(on && fader.settled(1.0)) {
                let target = if on { 1.0 } else { 0.0 };
                for (s, d) in $block.iter_mut().zip(dry.iter()) {
                    *s = *d + (*s - *d) * fader.next(target);
                }
            }
        }
    }};
}

impl VoiceChain {
    pub fn new(fs: f32) -> Self {
        Self {
            input_gain: Smoother::new(fs, 0.02, 1.0),
            output_gain: Smoother::new(fs, 0.02, 1.0),
            dry_wet: Smoother::new(fs, 0.02, 1.0),
            fs,
            engaged: Ramp::new(fs, SWITCH_S, 1.0),
            clean_gain: Smoother::new(fs, 0.02, 1.0),
            resting: false,
            quiet_blocks: 0,
            faders: [Smoother::new(fs, FADE_S, 0.0); SECTIONS],
            idle: [true; SECTIONS],
            gate: Gate::new(fs),
            melody: MelodyVoice::new(fs),
            pitch: PitchStage::new(fs),
            tone: Tone::new(fs),
            dist: Distortion::new(fs),
            robot: Robot::new(fs),
            chorus: Chorus::new(fs),
            echo: Echo::new(fs),
            reverb: Reverb::new(fs),
        }
    }

    /// Runs the chain; `voice_on == false` switches to the clean mic.
    ///
    /// The switch fades the chain's *input* over 25 ms, not its output, while the clean mic
    /// fades in: your voice crossfades without a click, and whatever the effects still have
    /// in flight (an echo, a reverb tail) rings out naturally instead of being cut. Once that
    /// tail has died away the chain rests and the output is exactly the clean mic (with the
    /// input gain, which is mic calibration, so the level does not jump).
    pub fn run(&mut self, block: &mut Block, params: &Snapshot, voice_on: bool) {
        let gin = from_db(params.get(P::InputGain));
        if self.resting {
            if !voice_on {
                for s in block.iter_mut() {
                    *s *= self.clean_gain.next(gin);
                }
                return;
            }
            self.resting = false;
        }
        let target = if voice_on { 1.0 } else { 0.0 };
        let clean = *block;
        let mut fade = [0.0; BLOCK];
        for (s, f) in block.iter_mut().zip(fade.iter_mut()) {
            *f = self.engaged.next(target);
            *s *= *f;
        }
        self.process(block, params);
        let tail_peak = block.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        for ((s, c), f) in block.iter_mut().zip(clean.iter()).zip(fade.iter()) {
            *s += c * self.clean_gain.next(gin) * (1.0 - f);
        }

        if !voice_on && self.engaged.value() == 0.0 {
            // An echo is silent between repeats: only call the tail over once it has been
            // quiet for longer than the echo time.
            let gap_s = if params.on(P::EchoOn) {
                params.get(P::EchoTime) / 1000.0
            } else {
                0.0
            };
            let needed = ((TAIL_QUIET_S + gap_s) * self.fs / BLOCK as f32) as u32;
            if tail_peak < TAIL_SILENT {
                self.quiet_blocks += 1;
                if self.quiet_blocks >= needed {
                    self.resting = true;
                    self.quiet_blocks = 0;
                    self.reset_effects();
                }
            } else {
                self.quiet_blocks = 0;
            }
        } else {
            self.quiet_blocks = 0;
        }
    }

    /// Clears every effect's memory (used when the chain goes to rest).
    fn reset_effects(&mut self) {
        self.gate.reset();
        self.melody.reset();
        self.pitch.reset();
        self.tone.reset();
        self.dist.reset();
        self.robot.reset();
        self.chorus.reset();
        self.echo.reset();
        self.reverb.reset();
    }
}

/// Crossfade time between effects and clean.
const SWITCH_S: f32 = 0.025;
/// After switching to clean, the effect tail plays until it stays below this level...
const TAIL_SILENT: f32 = 1e-5;
/// ...for this long (plus the echo time if the echo is on); then the chain rests.
const TAIL_QUIET_S: f32 = 0.05;

impl VoiceProcessor for VoiceChain {
    fn process(&mut self, block: &mut Block, p: &Snapshot) {
        let gin = from_db(p.get(P::InputGain));
        for s in block.iter_mut() {
            *s *= self.input_gain.next(gin);
        }
        section!(self, Section::Gate, p.on(P::GateOn), block, p, gate);
        section!(self, Section::Melody, p.on(P::MelodyOn), block, p, melody);

        let dry = *block;
        let pitch_on = p.on(P::PitchOn) && p.get(P::PitchSemis).abs() > 0.05;
        section!(self, Section::Pitch, pitch_on, block, p, pitch);
        section!(self, Section::Tone, p.on(P::EqOn), block, p, tone);
        section!(self, Section::Dist, p.on(P::DistOn), block, p, dist);
        section!(self, Section::Robot, p.on(P::RobotOn), block, p, robot);
        section!(self, Section::Chorus, p.on(P::ChorusOn), block, p, chorus);
        section!(self, Section::Echo, p.on(P::EchoOn), block, p, echo);
        section!(self, Section::Reverb, p.on(P::ReverbOn), block, p, reverb);

        let wet = p.get(P::DryWet);
        let gout = from_db(p.get(P::OutputGain));
        for (s, d) in block.iter_mut().zip(dry.iter()) {
            let w = self.dry_wet.next(wet);
            *s = (*d + (*s - *d) * w) * self.output_gain.next(gout);
        }
    }

    fn latency_samples(&self) -> usize {
        let mut total = 0;
        if !self.idle[Section::Melody as usize] {
            total += self.melody.latency();
        }
        if !self.idle[Section::Pitch as usize] {
            total += self.pitch.latency();
        }
        total
    }
}

impl VoiceChain {
    /// Melody Voice, for the engine to feed it the melody and read its status.
    pub fn melody(&mut self) -> &mut MelodyVoice {
        &mut self.melody
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::pitch::tests::frequency;
    use std::f32::consts::PI;

    const FS: f32 = 48_000.0;

    /// Runs `blocks` blocks of a 220 Hz sine through the chain, calling `at(block_index, &mut
    /// params)` before each block. Returns the output.
    fn run(blocks: usize, mut at: impl FnMut(usize, &mut Snapshot)) -> Vec<f32> {
        let mut chain = VoiceChain::new(FS);
        let mut p = Snapshot::defaults();
        let mut out = Vec::with_capacity(blocks * BLOCK);
        for n in 0..blocks {
            at(n, &mut p);
            let mut b = [0.0; BLOCK];
            for (i, s) in b.iter_mut().enumerate() {
                *s = 0.3 * (2.0 * PI * 220.0 * (n * BLOCK + i) as f32 / FS).sin();
            }
            chain.run(&mut b, &p, true);
            out.extend_from_slice(&b);
        }
        out
    }

    fn max_step(x: &[f32]) -> f32 {
        x.windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max)
    }

    #[test]
    fn defaults_are_transparent() {
        let out = run(200, |_, _| {});
        for (i, y) in out.iter().enumerate().skip(10) {
            let x = 0.3 * (2.0 * PI * 220.0 * i as f32 / FS).sin();
            assert!((x - y).abs() < 1e-4);
        }
    }

    #[test]
    fn toggling_every_effect_never_clicks() {
        // A 220 Hz sine at 0.3 changes by at most ~0.009 per sample. Turning effects on and off
        // must stay in that league (pitch/robot change the waveform, so allow some headroom).
        let toggles = [
            P::GateOn,
            P::MelodyOn,
            P::PitchOn,
            P::EqOn,
            P::DistOn,
            P::RobotOn,
            P::ChorusOn,
            P::EchoOn,
            P::ReverbOn,
        ];
        let out = run(3000, |n, p| {
            p.set(P::PitchSemis, -5.0);
            p.set(P::GateThreshold, -60.0);
            p.set(P::LowCut, 300.0);
            let which = (n / 150) % toggles.len();
            let on = (n / 75) % 2 == 0;
            p.set(toggles[which], if on { 1.0 } else { 0.0 });
        });
        let step = max_step(&out);
        assert!(step < 0.05, "max step {step}");
        assert!(out.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn jumping_sliders_never_clicks() {
        let out = run(2000, |n, p| {
            p.set(P::EchoOn, 1.0);
            p.set(P::ReverbOn, 1.0);
            let big = (n / 100) % 2 == 0;
            p.set(P::InputGain, if big { 6.0 } else { -12.0 });
            p.set(P::OutputGain, if big { -6.0 } else { 3.0 });
            p.set(P::EchoTime, if big { 80.0 } else { 600.0 });
            p.set(P::DryWet, if big { 1.0 } else { 0.2 });
        });
        assert!(max_step(&out) < 0.05, "max step {}", max_step(&out));
    }

    #[test]
    fn pitch_section_shifts_and_reports_latency() {
        let mut chain = VoiceChain::new(FS);
        let mut p = Snapshot::defaults();
        p.set(P::PitchOn, 1.0);
        p.set(P::PitchSemis, 12.0);
        for formant in [0.0, 1.0] {
            p.set(P::PitchFormant, formant);
            let mut out = Vec::new();
            for n in 0..1500 {
                let mut b = [0.0; BLOCK];
                for (i, s) in b.iter_mut().enumerate() {
                    *s = 0.3 * (2.0 * PI * 200.0 * (n * BLOCK + i) as f32 / FS).sin();
                }
                chain.run(&mut b, &p, true);
                out.extend_from_slice(&b);
            }
            let f = frequency(&out, FS);
            assert!((f - 400.0).abs() < 20.0, "formant={formant}: {f} Hz");
        }
        assert!(chain.latency_samples() > 400);
    }

    #[test]
    fn formant_path_has_no_startup_burst() {
        // Regression: the first LPC frames see a half-empty window while the plain path is
        // still silent; the formant path used to burst to 50x the input there.
        let mut st = PitchStage::new(FS);
        let mut p = Snapshot::defaults();
        p.set(P::PitchSemis, -5.0);
        p.set(P::PitchFormant, 1.0);
        for n in 0..1500 {
            let mut b = [0.0; BLOCK];
            for (i, s) in b.iter_mut().enumerate() {
                *s = 0.3 * (2.0 * PI * 180.0 * (n * BLOCK + i) as f32 / FS).sin();
            }
            st.process(&mut b, &p);
            assert!(
                b.iter().all(|x| x.abs() <= 0.3 * PEAK_LIMIT + 1e-3),
                "block {n}"
            );
        }
    }

    #[test]
    fn formant_preserving_pitch_keeps_the_level() {
        use crate::dsp::biquad::{Biquad, Coeffs};
        // A pure tone and a vowel-like buzz (120 Hz sawtooth through two formant peaks).
        let mut f1 = Biquad::new(Coeffs::peaking(FS, 700.0, 4.0, 15.0));
        let mut f2 = Biquad::new(Coeffs::peaking(FS, 1200.0, 4.0, 12.0));
        let mut saw = 0.0f32;
        let mut vowel = move |_: usize| {
            saw += 120.0 / FS;
            saw -= saw.floor();
            0.1 * f2.process(f1.process(saw - 0.5))
        };
        let tone = |i: usize| 0.3 * (2.0 * PI * 180.0 * i as f32 / FS).sin();
        for (name, semis) in [("down", -5.0), ("up", 7.0)] {
            for (kind, input) in [
                (
                    "tone",
                    &mut (Box::new(tone) as Box<dyn FnMut(usize) -> f32>),
                ),
                (
                    "vowel",
                    &mut (Box::new(&mut vowel) as Box<dyn FnMut(usize) -> f32>),
                ),
            ] {
                let mut chain = VoiceChain::new(FS);
                let mut p = Snapshot::defaults();
                p.set(P::PitchOn, 1.0);
                p.set(P::PitchSemis, semis);
                p.set(P::PitchFormant, 1.0);
                let (mut e_in, mut e_out) = (0.0f64, 0.0f64);
                for n in 0..1500 {
                    let mut b = [0.0; BLOCK];
                    for (i, s) in b.iter_mut().enumerate() {
                        *s = input(n * BLOCK + i);
                    }
                    let x = b;
                    chain.run(&mut b, &p, true);
                    if n > 300 {
                        e_in += x.iter().map(|v| f64::from(v * v)).sum::<f64>();
                        e_out += b.iter().map(|v| f64::from(v * v)).sum::<f64>();
                    }
                }
                let db = 10.0 * (e_out / e_in).log10();
                assert!(db.abs() < 6.0, "{kind} {name}: level changed by {db:.1} dB");
            }
        }
    }

    #[test]
    fn everything_on_at_extremes_stays_finite_and_bounded() {
        let out = run(4000, |_, p| {
            for id in [
                P::GateOn,
                P::MelodyOn,
                P::PitchOn,
                P::PitchFormant,
                P::EqOn,
                P::DistOn,
                P::RobotOn,
                P::ChorusOn,
                P::EchoOn,
                P::ReverbOn,
            ] {
                p.set(id, 1.0);
            }
            p.set(P::PitchSemis, -12.0);
            p.set(P::MelodyMode, 3.0); // scale snap: retunes without a melody
            p.set(P::MelodyTranspose, 24.0);
            p.set(P::MelodyFormantShift, 6.0);
            p.set(P::GateThreshold, -80.0);
            p.set(P::LowGain, 15.0);
            p.set(P::MidGain, 15.0);
            p.set(P::HighGain, 15.0);
            p.set(P::DistDrive, 40.0);
            p.set(P::EchoFeedback, 0.9);
            p.set(P::EchoMix, 1.0);
            p.set(P::ReverbSize, 1.0);
            p.set(P::ReverbMix, 1.0);
            p.set(P::InputGain, 24.0);
        });
        assert!(out.iter().all(|x| x.is_finite() && x.abs() < 50.0));
    }

    #[test]
    fn full_chain_runs_far_faster_than_real_time() {
        let seconds = 5.0;
        let blocks = (seconds * FS) as usize / BLOCK;
        let start = std::time::Instant::now();
        run(blocks, |_, p| {
            for id in [
                P::GateOn,
                P::MelodyOn,
                P::PitchOn,
                P::PitchFormant,
                P::EqOn,
                P::DistOn,
                P::RobotOn,
                P::ChorusOn,
                P::EchoOn,
                P::ReverbOn,
            ] {
                p.set(id, 1.0);
            }
            p.set(P::PitchSemis, -7.0);
            p.set(P::MelodyMode, 3.0);
            p.set(P::MelodyFormantShift, 3.0);
        });
        let load = start.elapsed().as_secs_f32() / seconds;
        println!("full chain CPU load: {:.1}% of one core", load * 100.0);
        assert!(load < 0.25, "chain uses {:.0}% of real time", load * 100.0);
    }

    #[test]
    fn clean_is_the_mic_with_its_input_gain_once_the_tail_is_gone() {
        let mut chain = VoiceChain::new(FS);
        let mut p = Snapshot::defaults();
        p.set(P::RobotOn, 1.0);
        p.set(P::InputGain, 6.0);
        let gain = from_db(6.0);
        for n in 0..200 {
            let mut b = [0.0; BLOCK];
            for (i, s) in b.iter_mut().enumerate() {
                *s = 0.3 * (2.0 * PI * 220.0 * (n * BLOCK + i) as f32 / FS).sin();
            }
            let input = b;
            chain.run(&mut b, &p, false);
            if n > 150 {
                assert!(chain.resting, "effects stopped running");
                for (x, y) in input.iter().zip(b.iter()) {
                    assert!((x * gain - y).abs() < 1e-4, "clean: {x} -> {y}");
                }
            }
        }
    }

    /// Effects on for a burst, then switch to clean: the echo of the burst keeps sounding
    /// after the switch instead of being cut off.
    #[test]
    fn switching_to_clean_lets_the_echo_ring_out() {
        let mut chain = VoiceChain::new(FS);
        let mut p = Snapshot::defaults();
        p.set(P::EchoOn, 1.0);
        p.set(P::EchoFeedback, 0.5);
        p.set(P::EchoMix, 0.8);
        let mut out = Vec::new();
        let blocks_per_s = FS as usize / BLOCK;
        // 6 s: with feedback 0.5 the repeats take ~4 s to fall below the silence threshold.
        for n in 0..blocks_per_s * 6 {
            let mut b = [0.0; BLOCK];
            if n < blocks_per_s / 10 {
                // 100 ms burst
                for (i, s) in b.iter_mut().enumerate() {
                    *s = 0.3 * (2.0 * PI * 300.0 * (n * BLOCK + i) as f32 / FS).sin();
                }
            }
            let voice_on = n < blocks_per_s / 10 + 5; // switch right after the burst
            chain.run(&mut b, &p, voice_on);
            out.extend_from_slice(&b);
        }
        // The first echo arrives 250 ms after the burst, long after the switch.
        let echo = &out[(0.26 * FS) as usize..(0.34 * FS) as usize];
        let peak = echo.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(peak > 0.05, "echo tail after switching to clean: {peak}");
        assert!(chain.resting, "and once it has died away the chain rests");
    }

    #[test]
    fn toggling_while_talking_never_clicks_and_comes_back_exactly() {
        let mut chain = VoiceChain::new(FS);
        let mut p = Snapshot::defaults();
        p.set(P::RobotOn, 1.0);
        p.set(P::ReverbOn, 1.0);
        p.set(P::InputGain, 3.0);
        let mut out = Vec::new();
        for n in 0..3000 {
            let mut b = [0.0; BLOCK];
            for (i, s) in b.iter_mut().enumerate() {
                *s = 0.3 * (2.0 * PI * 220.0 * (n * BLOCK + i) as f32 / FS).sin();
            }
            let voice_on = (n / 200) % 2 == 0; // switch every ~270 ms
            chain.run(&mut b, &p, voice_on);
            out.extend_from_slice(&b);
        }
        let step = max_step(&out);
        // The sine itself (at +3 dB) moves up to ~0.012 per sample; ring modulation and the
        // reverb add some more, but nothing like a click (a jump of 0.3+).
        assert!(step < 0.06, "max step {step}");
        assert!(out.iter().all(|x| x.is_finite()));
    }
}
