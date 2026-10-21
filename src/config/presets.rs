//! Named sets of effect values. Built-in presets are defined here as changes from the
//! defaults; user presets live in the config file.

use crate::dsp::params::{EffectSettings, P, Snapshot};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub effects: EffectSettings,
}

fn make(name: &str, changes: &[(P, f32)]) -> Preset {
    let mut s = Snapshot::defaults();
    for &(id, v) in changes {
        s.set(id, v);
    }
    Preset {
        name: name.to_string(),
        effects: s.to_settings(),
    }
}

pub fn builtin() -> Vec<Preset> {
    use P::*;
    vec![
        make("Clean", &[]),
        make(
            "Deep Voice",
            &[
                (PitchOn, 1.0),
                (PitchSemis, -5.0),
                (PitchFormant, 1.0),
                (EqOn, 1.0),
                (LowGain, 4.0),
                (HighGain, -2.0),
            ],
        ),
        make(
            "Chipmunk",
            &[
                (PitchOn, 1.0),
                (PitchSemis, 8.0),
                (EqOn, 1.0),
                (LowCut, 150.0),
                (HighGain, 2.0),
            ],
        ),
        make(
            "Robot",
            &[
                (RobotOn, 1.0),
                (RobotFreq, 50.0),
                (RobotMix, 0.85),
                (DistOn, 1.0),
                (DistDrive, 6.0),
                (DistMix, 0.3),
                (EqOn, 1.0),
                (MidGain, 4.0),
                (MidFreq, 1500.0),
                (EchoOn, 1.0),
                (EchoTime, 35.0),
                (EchoFeedback, 0.3),
                (EchoMix, 0.2),
            ],
        ),
        make(
            "Cave",
            &[
                (EchoOn, 1.0),
                (EchoTime, 320.0),
                (EchoFeedback, 0.45),
                (EchoMix, 0.35),
                (ReverbOn, 1.0),
                (ReverbSize, 0.95),
                (ReverbDamping, 0.3),
                (ReverbMix, 0.5),
                (EqOn, 1.0),
                (LowCut, 120.0),
            ],
        ),
        make(
            "Radio",
            &[
                (EqOn, 1.0),
                (LowCut, 400.0),
                (HighCut, 4500.0),
                (MidGain, 5.0),
                (MidFreq, 1800.0),
                (DistOn, 1.0),
                (DistDrive, 14.0),
                (DistMix, 0.5),
                (GateOn, 1.0),
                (GateThreshold, -45.0),
            ],
        ),
        make(
            "Telephone",
            &[
                (EqOn, 1.0),
                (LowCut, 300.0),
                (HighCut, 3400.0),
                (MidGain, 6.0),
                (MidFreq, 1500.0),
                (DistOn, 1.0),
                (DistDrive, 8.0),
                (DistMix, 0.35),
            ],
        ),
        make(
            "Demon",
            &[
                (PitchOn, 1.0),
                (PitchSemis, -8.0),
                (DistOn, 1.0),
                (DistDrive, 10.0),
                (DistMix, 0.4),
                (RobotOn, 1.0),
                (RobotFreq, 30.0),
                (RobotMix, 0.2),
                (ReverbOn, 1.0),
                (ReverbSize, 0.7),
                (ReverbMix, 0.3),
                (EqOn, 1.0),
                (LowGain, 6.0),
            ],
        ),
    ]
}

pub fn is_builtin(name: &str) -> bool {
    builtin().iter().any(|p| p.name.eq_ignore_ascii_case(name))
}

/// The values a preset produces when applied on top of `current`: everything comes from the
/// preset except the input gain, which belongs to the mic, not to the voice.
pub fn apply(preset: &Preset, current: &Snapshot) -> Snapshot {
    let mut s = Snapshot::from_settings(&preset.effects);
    s.set(P::InputGain, current.get(P::InputGain));
    s
}

/// True if `current` is exactly what applying `preset` would give.
pub fn matches(preset: &Preset, current: &Snapshot) -> bool {
    apply(preset, current) == *current
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::block::BLOCK;
    use crate::dsp::chain::VoiceChain;

    #[test]
    fn the_seven_requested_presets_exist_with_unique_names() {
        let all = builtin();
        for name in [
            "Deep Voice",
            "Chipmunk",
            "Robot",
            "Cave",
            "Radio",
            "Telephone",
            "Demon",
        ] {
            assert!(all.iter().any(|p| p.name == name), "{name} missing");
        }
        let mut names: Vec<_> = all.iter().map(|p| p.name.to_lowercase()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), all.len());
    }

    #[test]
    fn every_preset_sounds_sane() {
        for preset in builtin() {
            let p = Snapshot::from_settings(&preset.effects);
            let mut chain = VoiceChain::new(48_000.0);
            let mut peak = 0.0f32;
            for n in 0..1500 {
                let mut b = [0.0; BLOCK];
                for (i, s) in b.iter_mut().enumerate() {
                    let t = (n * BLOCK + i) as f32 / 48_000.0;
                    *s = 0.3 * (2.0 * std::f32::consts::PI * 180.0 * t).sin();
                }
                chain.run(&mut b, &p, true);
                assert!(b.iter().all(|x| x.is_finite()), "{}", preset.name);
                peak = b.iter().fold(peak, |m, x| m.max(x.abs()));
            }
            assert!(peak > 0.05 && peak < 2.0, "{}: peak {peak}", preset.name);
        }
    }

    #[test]
    fn applying_keeps_the_input_gain_and_modified_detection_works() {
        let mut current = Snapshot::defaults();
        current.set(P::InputGain, 6.0);
        let demon = builtin().into_iter().find(|p| p.name == "Demon").unwrap();
        let applied = apply(&demon, &current);
        assert_eq!(applied.get(P::InputGain), 6.0);
        assert_eq!(applied.get(P::PitchSemis), -8.0);
        assert!(matches(&demon, &applied));
        let mut tweaked = applied;
        tweaked.set(P::ReverbMix, 0.9);
        assert!(!matches(&demon, &tweaked));
        assert!(is_builtin("deep voice"));
    }
}
