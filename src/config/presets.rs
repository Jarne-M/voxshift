//! Named sets of effect values. Built-in presets are defined here as changes from the
//! defaults; user presets live in the config file.

use crate::dsp::params::{EffectSettings, P, Snapshot};
use crate::melody::MelodySource;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub effects: EffectSettings,
    /// Melody Voice's melody. `None` keeps whatever melody is picked (built-in presets, and
    /// presets saved without Melody Voice or before it existed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub melody: Option<MelodySource>,
    /// One of your favourites: shown first in the preset bar (and offered in the tray menu).
    /// Yours alone: a shared `.voxpack` does not carry it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub favorite: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn make(name: &str, changes: &[(P, f32)]) -> Preset {
    let mut s = Snapshot::defaults();
    for &(id, v) in changes {
        s.set(id, v);
    }
    Preset {
        name: name.to_string(),
        effects: s.to_settings(),
        melody: None,
        favorite: false,
    }
}

/// The built-in presets (a copy; see [`builtin_presets`] for the list itself).
pub fn builtin() -> Vec<Preset> {
    builtin_presets().to_vec()
}

/// The built-in presets, built once.
pub fn builtin_presets() -> &'static [Preset] {
    static PRESETS: std::sync::OnceLock<Vec<Preset>> = std::sync::OnceLock::new();
    PRESETS.get_or_init(make_builtin)
}

/// Your own presets in the order they are shown: favourites first, otherwise as saved.
pub fn favorites_first(own: &[Preset]) -> Vec<&Preset> {
    let (mut favorites, rest): (Vec<&Preset>, Vec<&Preset>) = own.iter().partition(|p| p.favorite);
    favorites.extend(rest);
    favorites
}

/// Whether `name` can be the name of one of your presets: not empty, not a built-in's name
/// and not taken by another preset of yours (`except` = the one being renamed).
pub fn name_problem(name: &str, own: &[Preset], except: Option<&str>) -> Option<&'static str> {
    let name = name.trim();
    if name.is_empty() {
        return Some("Type a name");
    }
    if is_builtin(name) {
        return Some("That is the name of a built-in preset");
    }
    let taken = own
        .iter()
        .any(|p| Some(p.name.as_str()) != except && p.name.eq_ignore_ascii_case(name));
    (taken && except.is_some()).then_some("Another preset of yours has that name")
}

fn make_builtin() -> Vec<Preset> {
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
        // Talk, and every syllable is the next note of the melody you picked.
        make(
            "Sing the Song (per-syllable)",
            &[
                (MelodyOn, 1.0),
                (MelodyMode, 0.0),
                (MelodyRetune, 5.0),
                (MelodyGlide, 40.0),
                (MelodyFold, 1.0),
                (MelodyLoop, 1.0),
                (MelodyFormant, 1.0),
                (ReverbOn, 1.0),
                (ReverbSize, 0.5),
                (ReverbMix, 0.15),
            ],
        ),
        // The robotic, instant auto-tune: snaps to C major with no glide.
        make(
            "Hard Tune",
            &[
                (MelodyOn, 1.0),
                (MelodyMode, 3.0),
                (MelodyKey, 0.0),
                (MelodyScale, 0.0),
                (MelodyRetune, 0.0),
                (MelodyGlide, 0.0),
                (MelodyFormant, 1.0),
            ],
        ),
    ]
}

pub fn is_builtin(name: &str) -> bool {
    builtin_presets()
        .iter()
        .any(|p| p.name.eq_ignore_ascii_case(name))
}

/// The values a preset produces when applied on top of `current`: everything comes from the
/// preset except the input gain, which belongs to the mic, not to the voice.
pub fn apply(preset: &Preset, current: &Snapshot) -> Snapshot {
    let mut s = Snapshot::from_settings(&preset.effects);
    s.set(P::InputGain, current.get(P::InputGain));
    s
}

/// True if `current` (and the melody picked) is exactly what applying `preset` would give.
pub fn matches(preset: &Preset, current: &Snapshot, melody: &MelodySource) -> bool {
    apply(preset, current) == *current && preset.melody.as_ref().is_none_or(|m| m == melody)
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
        let any = MelodySource::default();
        assert!(matches(&demon, &applied, &any));
        let mut tweaked = applied;
        tweaked.set(P::ReverbMix, 0.9);
        assert!(!matches(&demon, &tweaked, &any));
        assert!(is_builtin("deep voice"));
    }

    #[test]
    fn melody_presets_exist_and_a_preset_melody_counts_for_modified() {
        let all = builtin();
        let sing = all
            .iter()
            .find(|p| p.name == "Sing the Song (per-syllable)")
            .unwrap();
        let hard = all.iter().find(|p| p.name == "Hard Tune").unwrap();
        let s = Snapshot::from_settings(&sing.effects);
        assert!(s.on(P::MelodyOn) && s.choice(P::MelodyMode) == 0);
        let h = Snapshot::from_settings(&hard.effects);
        assert!(h.on(P::MelodyOn) && h.choice(P::MelodyMode) == 3);
        assert_eq!(h.get(P::MelodyRetune), 0.0);
        assert!(
            sing.melody.is_none(),
            "built-ins keep the melody you picked"
        );

        let mine = Preset {
            name: "Mine".into(),
            effects: sing.effects.clone(),
            melody: Some(MelodySource::BuiltIn("Ode to Joy".into())),
            favorite: false,
        };
        let fx = apply(&mine, &Snapshot::defaults());
        assert!(matches(
            &mine,
            &fx,
            &MelodySource::BuiltIn("Ode to Joy".into())
        ));
        assert!(
            !matches(&mine, &fx, &MelodySource::default()),
            "other melody"
        );
    }

    #[test]
    fn presets_round_trip_with_and_without_a_melody() {
        let p = Preset {
            name: "Song".into(),
            effects: builtin()[0].effects.clone(),
            melody: Some(MelodySource::File {
                path: "C:/music/song.mid".into(),
                part: Some(2),
            }),
            favorite: true,
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"favorite\":true"));
        assert_eq!(serde_json::from_str::<Preset>(&json).unwrap(), p);
        // A preset saved before Melody Voice and favourites existed still loads.
        let old: Preset = serde_json::from_str(r#"{"name":"Old","effects":{}}"#).unwrap();
        assert_eq!(old.melody, None);
        assert!(!old.favorite);
        let plain = serde_json::to_string(&builtin()[0]).unwrap();
        assert!(
            !plain.contains("melody\":") && !plain.contains("favorite"),
            "no melody or favourite field when there is none"
        );
    }

    #[test]
    fn favourites_come_first_and_names_are_checked() {
        let own: Vec<Preset> = [("Bee", false), ("Ant", true), ("Cat", false), ("Dog", true)]
            .into_iter()
            .map(|(name, favorite)| Preset {
                favorite,
                ..make(name, &[])
            })
            .collect();
        let order: Vec<&str> = favorites_first(&own)
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(
            order,
            ["Ant", "Dog", "Bee", "Cat"],
            "favourites, then as saved"
        );

        assert_eq!(name_problem("  ", &own, None), Some("Type a name"));
        assert!(
            name_problem("deep voice", &own, None).is_some(),
            "a built-in"
        );
        assert_eq!(name_problem("New one", &own, None), None);
        // Saving under a name of your own replaces that preset; renaming onto one is refused.
        assert_eq!(name_problem("Bee", &own, None), None);
        assert!(name_problem("bee", &own, Some("Cat")).is_some());
        assert_eq!(name_problem("Cat", &own, Some("Cat")), None, "unchanged");
        assert_eq!(builtin(), builtin_presets(), "one list");
    }
}
