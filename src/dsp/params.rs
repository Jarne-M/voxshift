//! Every effect parameter, described once: key, label, range and default.
//! The GUI builds its sliders from this table, presets store values by key (so adding a
//! parameter never breaks old presets), and the audio thread reads a snapshot of the atomics
//! once per block.

use crate::audio::shared::AtomicF32;
use std::collections::BTreeMap;

/// Parameter ids. The discriminant is the index into [`PARAMS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum P {
    InputGain,
    OutputGain,
    DryWet,
    GateOn,
    GateThreshold,
    PitchOn,
    PitchSemis,
    PitchFormant,
    EqOn,
    LowCut,
    HighCut,
    LowGain,
    MidGain,
    MidFreq,
    HighGain,
    DistOn,
    DistDrive,
    DistMix,
    RobotOn,
    RobotFreq,
    RobotMix,
    ChorusOn,
    ChorusRate,
    ChorusDepth,
    ChorusMix,
    EchoOn,
    EchoTime,
    EchoFeedback,
    EchoMix,
    ReverbOn,
    ReverbSize,
    ReverbDamping,
    ReverbMix,
    MelodyOn,
    MelodyMode,
    MelodyKey,
    MelodyScale,
    MelodyRetune,
    MelodyGlide,
    MelodyTranspose,
    MelodyFold,
    MelodyFormant,
    MelodyFormantShift,
    MelodyMix,
    MelodyBpm,
    MelodyLoop,
    MelodySensitivity,
    MelodyMinNote,
    MelodyResetAfter,
    MelodyWindow,
    MelodyMinHz,
    MelodyMaxHz,
}

pub const COUNT: usize = 52;

/// How a value is shown and edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// On/off (0 or 1).
    Toggle,
    Linear,
    /// Logarithmic slider (frequencies, times).
    Log,
    /// One of a few options, stored as its index (0, 1, 2, ...).
    Choice,
}

#[derive(Clone, Copy, Debug)]
pub struct ParamInfo {
    pub id: P,
    pub key: &'static str,
    pub label: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub unit: &'static str,
    pub kind: Kind,
}

const fn p(
    id: P,
    key: &'static str,
    label: &'static str,
    (min, max, default): (f32, f32, f32),
    unit: &'static str,
    kind: Kind,
) -> ParamInfo {
    ParamInfo {
        id,
        key,
        label,
        min,
        max,
        default,
        unit,
        kind,
    }
}

const fn toggle(id: P, key: &'static str, default: bool) -> ParamInfo {
    let d = if default { 1.0 } else { 0.0 };
    p(id, key, "Enabled", (0.0, 1.0, d), "", Kind::Toggle)
}

use Kind::{Choice, Linear, Log};

pub const PARAMS: [ParamInfo; COUNT] = [
    p(
        P::InputGain,
        "input_gain",
        "Input gain",
        (-24.0, 24.0, 0.0),
        "dB",
        Linear,
    ),
    p(
        P::OutputGain,
        "output_gain",
        "Output gain",
        (-24.0, 24.0, 0.0),
        "dB",
        Linear,
    ),
    p(
        P::DryWet,
        "dry_wet",
        "Dry / wet",
        (0.0, 1.0, 1.0),
        "",
        Linear,
    ),
    toggle(P::GateOn, "gate_on", false),
    p(
        P::GateThreshold,
        "gate_threshold",
        "Threshold",
        (-80.0, 0.0, -50.0),
        "dB",
        Linear,
    ),
    toggle(P::PitchOn, "pitch_on", false),
    p(
        P::PitchSemis,
        "pitch_semitones",
        "Pitch",
        (-12.0, 12.0, 0.0),
        "st",
        Linear,
    ),
    p(
        P::PitchFormant,
        "pitch_formant",
        "Preserve formants",
        (0.0, 1.0, 0.0),
        "",
        Kind::Toggle,
    ),
    toggle(P::EqOn, "eq_on", false),
    p(
        P::LowCut,
        "eq_low_cut",
        "Low cut",
        (20.0, 1000.0, 20.0),
        "Hz",
        Log,
    ),
    p(
        P::HighCut,
        "eq_high_cut",
        "High cut",
        (1000.0, 20000.0, 20000.0),
        "Hz",
        Log,
    ),
    p(
        P::LowGain,
        "eq_low",
        "Low (200 Hz)",
        (-15.0, 15.0, 0.0),
        "dB",
        Linear,
    ),
    p(
        P::MidGain,
        "eq_mid",
        "Mid",
        (-15.0, 15.0, 0.0),
        "dB",
        Linear,
    ),
    p(
        P::MidFreq,
        "eq_mid_freq",
        "Mid freq",
        (300.0, 5000.0, 1000.0),
        "Hz",
        Log,
    ),
    p(
        P::HighGain,
        "eq_high",
        "High (4 kHz)",
        (-15.0, 15.0, 0.0),
        "dB",
        Linear,
    ),
    toggle(P::DistOn, "dist_on", false),
    p(
        P::DistDrive,
        "dist_drive",
        "Drive",
        (0.0, 40.0, 12.0),
        "dB",
        Linear,
    ),
    p(P::DistMix, "dist_mix", "Mix", (0.0, 1.0, 1.0), "", Linear),
    toggle(P::RobotOn, "robot_on", false),
    p(
        P::RobotFreq,
        "robot_freq",
        "Carrier",
        (20.0, 500.0, 60.0),
        "Hz",
        Log,
    ),
    p(P::RobotMix, "robot_mix", "Mix", (0.0, 1.0, 1.0), "", Linear),
    toggle(P::ChorusOn, "chorus_on", false),
    p(
        P::ChorusRate,
        "chorus_rate",
        "Rate",
        (0.1, 5.0, 0.8),
        "Hz",
        Log,
    ),
    p(
        P::ChorusDepth,
        "chorus_depth",
        "Depth",
        (0.0, 10.0, 3.0),
        "ms",
        Linear,
    ),
    p(
        P::ChorusMix,
        "chorus_mix",
        "Mix",
        (0.0, 1.0, 0.5),
        "",
        Linear,
    ),
    toggle(P::EchoOn, "echo_on", false),
    p(
        P::EchoTime,
        "echo_time",
        "Time",
        (20.0, 1000.0, 250.0),
        "ms",
        Log,
    ),
    p(
        P::EchoFeedback,
        "echo_feedback",
        "Feedback",
        (0.0, 0.9, 0.35),
        "",
        Linear,
    ),
    p(P::EchoMix, "echo_mix", "Mix", (0.0, 1.0, 0.3), "", Linear),
    toggle(P::ReverbOn, "reverb_on", false),
    p(
        P::ReverbSize,
        "reverb_size",
        "Size",
        (0.0, 1.0, 0.6),
        "",
        Linear,
    ),
    p(
        P::ReverbDamping,
        "reverb_damping",
        "Damping",
        (0.0, 1.0, 0.5),
        "",
        Linear,
    ),
    p(
        P::ReverbMix,
        "reverb_mix",
        "Mix",
        (0.0, 1.0, 0.3),
        "",
        Linear,
    ),
    toggle(P::MelodyOn, "melody_on", false),
    p(
        P::MelodyMode,
        "melody_mode",
        "Mode",
        (0.0, 3.0, 0.0),
        "",
        Choice,
    ),
    p(
        P::MelodyKey,
        "melody_key",
        "Key",
        (0.0, 11.0, 0.0),
        "",
        Choice,
    ),
    p(
        P::MelodyScale,
        "melody_scale",
        "Scale",
        (0.0, 4.0, 0.0),
        "",
        Choice,
    ),
    p(
        P::MelodyRetune,
        "melody_retune",
        "Retune speed",
        (0.0, 400.0, 15.0),
        "ms",
        Linear,
    ),
    p(
        P::MelodyGlide,
        "melody_glide",
        "Glide",
        (0.0, 300.0, 40.0),
        "ms",
        Linear,
    ),
    p(
        P::MelodyTranspose,
        "melody_transpose",
        "Transpose",
        (-24.0, 24.0, 0.0),
        "st",
        Linear,
    ),
    p(
        P::MelodyFold,
        "melody_fold",
        "Octave-fold into my range",
        (0.0, 1.0, 1.0),
        "",
        Kind::Toggle,
    ),
    p(
        P::MelodyFormant,
        "melody_formant",
        "Preserve formants",
        (0.0, 1.0, 1.0),
        "",
        Kind::Toggle,
    ),
    p(
        P::MelodyFormantShift,
        "melody_formant_shift",
        "Formant shift",
        (-6.0, 6.0, 0.0),
        "st",
        Linear,
    ),
    p(
        P::MelodyMix,
        "melody_mix",
        "Dry / wet",
        (0.0, 1.0, 1.0),
        "",
        Linear,
    ),
    p(
        P::MelodyBpm,
        "melody_bpm",
        "Tempo",
        (30.0, 240.0, 100.0),
        "BPM",
        Linear,
    ),
    p(
        P::MelodyLoop,
        "melody_loop",
        "Loop",
        (0.0, 1.0, 1.0),
        "",
        Kind::Toggle,
    ),
    p(
        P::MelodySensitivity,
        "melody_sensitivity",
        "Sensitivity",
        (0.0, 1.0, 0.5),
        "",
        Linear,
    ),
    p(
        P::MelodyMinNote,
        "melody_min_note",
        "Min note length",
        (40.0, 600.0, 150.0),
        "ms",
        Log,
    ),
    p(
        P::MelodyResetAfter,
        "melody_reset_after",
        "Back to start after silence",
        (0.0, 30.0, 0.0),
        "s",
        Linear,
    ),
    p(
        P::MelodyWindow,
        "melody_window",
        "Analysis window",
        (20.0, 60.0, 30.0),
        "ms",
        Linear,
    ),
    p(
        P::MelodyMinHz,
        "melody_min_hz",
        "Lowest voice pitch",
        (50.0, 200.0, 70.0),
        "Hz",
        Log,
    ),
    p(
        P::MelodyMaxHz,
        "melody_max_hz",
        "Highest voice pitch",
        (200.0, 1000.0, 500.0),
        "Hz",
        Log,
    ),
];

impl P {
    pub fn info(self) -> &'static ParamInfo {
        &PARAMS[self as usize]
    }
}

impl ParamInfo {
    pub fn clamp(&self, v: f32) -> f32 {
        if v.is_finite() {
            v.clamp(self.min, self.max)
        } else {
            self.default
        }
    }
}

/// A plain copy of every value, read once per audio block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snapshot(pub [f32; COUNT]);

impl Snapshot {
    pub fn defaults() -> Self {
        Self(std::array::from_fn(|i| PARAMS[i].default))
    }

    pub fn get(&self, id: P) -> f32 {
        self.0[id as usize]
    }

    pub fn on(&self, id: P) -> bool {
        self.get(id) >= 0.5
    }

    /// A [`Kind::Choice`] value as its index.
    pub fn choice(&self, id: P) -> usize {
        self.get(id).round().max(0.0) as usize
    }

    pub fn set(&mut self, id: P, v: f32) {
        self.0[id as usize] = id.info().clamp(v);
    }
}

/// Serialisable form of all values: key -> value. Missing keys load as defaults and unknown
/// keys are ignored, so presets survive parameters being added or removed.
pub type EffectSettings = BTreeMap<String, f32>;

impl Snapshot {
    pub fn to_settings(self) -> EffectSettings {
        PARAMS
            .iter()
            .map(|info| (info.key.to_string(), self.get(info.id)))
            .collect()
    }

    pub fn from_settings(s: &EffectSettings) -> Self {
        let mut snap = Self::defaults();
        for info in &PARAMS {
            if let Some(&v) = s.get(info.key) {
                snap.set(info.id, v);
            }
        }
        snap
    }
}

/// The live values, written by the GUI and read by the audio thread.
pub struct ParamStore([AtomicF32; COUNT]);

impl Default for ParamStore {
    fn default() -> Self {
        Self(std::array::from_fn(|i| AtomicF32::new(PARAMS[i].default)))
    }
}

impl ParamStore {
    #[cfg(test)]
    pub fn get(&self, id: P) -> f32 {
        self.0[id as usize].get()
    }

    pub fn set(&self, id: P, v: f32) {
        self.0[id as usize].set(id.info().clamp(v));
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot(std::array::from_fn(|i| self.0[i].get()))
    }

    pub fn store(&self, snap: &Snapshot) {
        for (a, &v) in self.0.iter().zip(snap.0.iter()) {
            a.set(v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_order_matches_ids_and_keys_are_unique() {
        let mut keys = std::collections::HashSet::new();
        for (i, info) in PARAMS.iter().enumerate() {
            assert_eq!(info.id as usize, i, "{} is out of order", info.key);
            assert!(keys.insert(info.key), "duplicate key {}", info.key);
            assert!(info.min < info.max);
            assert!(
                (info.min..=info.max).contains(&info.default),
                "{}",
                info.key
            );
        }
    }

    #[test]
    fn settings_round_trip_and_tolerate_missing_or_bad_values() {
        let mut snap = Snapshot::defaults();
        snap.set(P::PitchSemis, 5.0);
        snap.set(P::EchoOn, 1.0);
        assert_eq!(Snapshot::from_settings(&snap.to_settings()), snap);

        let mut partial = EffectSettings::new();
        partial.insert("pitch_semitones".into(), 99.0); // clamped
        partial.insert("no_such_param".into(), 1.0); // ignored
        let loaded = Snapshot::from_settings(&partial);
        assert_eq!(loaded.get(P::PitchSemis), 12.0);
        assert_eq!(loaded.get(P::EchoTime), P::EchoTime.info().default);
    }

    #[test]
    fn store_clamps_and_ignores_nan() {
        let store = ParamStore::default();
        store.set(P::DryWet, 7.0);
        assert_eq!(store.get(P::DryWet), 1.0);
        store.set(P::DryWet, f32::NAN);
        assert_eq!(store.get(P::DryWet), P::DryWet.info().default);
    }
}
