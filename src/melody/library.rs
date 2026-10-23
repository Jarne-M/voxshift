//! Built-in melodies: short public-domain tunes and a few scales, written out by hand below
//! (no audio or MIDI data is bundled).
//!
//! Notation: `NAME[:beats]`, e.g. `E4` (a quarter note), `D4:2` (a half note), `C#5:0.5`
//! (an eighth). `r:1` is a quarter rest. Beats are quarter notes.

use super::{Melody, Note};

pub struct BuiltIn {
    pub name: &'static str,
    pub bpm: f32,
    spec: &'static str,
}

const fn tune(name: &'static str, bpm: f32, spec: &'static str) -> BuiltIn {
    BuiltIn { name, bpm, spec }
}

/// Beethoven, Symphony No. 9 (1824): the famous theme, in C.
const ODE_TO_JOY: &str = "\
    E4 E4 F4 G4 G4 F4 E4 D4 C4 C4 D4 E4 E4:1.5 D4:0.5 D4:2 \
    E4 E4 F4 G4 G4 F4 E4 D4 C4 C4 D4 E4 D4:1.5 C4:0.5 C4:2 \
    D4 D4 E4 C4 D4 E4:0.5 F4:0.5 E4 C4 D4 E4:0.5 F4:0.5 E4 D4 C4 D4 G3:2 \
    E4 E4 F4 G4 G4 F4 E4 D4 C4 C4 D4 E4 D4:1.5 C4:0.5 C4:2";

/// Traditional (French, 18th century).
const TWINKLE: &str = "\
    C4 C4 G4 G4 A4 A4 G4:2 F4 F4 E4 E4 D4 D4 C4:2 \
    G4 G4 F4 F4 E4 E4 D4:2 G4 G4 F4 F4 E4 E4 D4:2 \
    C4 C4 G4 G4 A4 A4 G4:2 F4 F4 E4 E4 D4 D4 C4:2";

/// Hill sisters (1893); public domain. In 3/4 with a pickup.
const HAPPY_BIRTHDAY: &str = "\
    G4:0.75 G4:0.25 A4 G4 C5 B4:2 \
    G4:0.75 G4:0.25 A4 G4 D5 C5:2 \
    G4:0.75 G4:0.25 G5 E5 C5 B4 A4 \
    F5:0.75 F5:0.25 E5 C5 D5 C5:3";

/// Traditional English (16th century), in A minor, 6/8.
const GREENSLEEVES: &str = "\
    A4:0.5 C5 D5:0.5 E5:0.75 F5:0.25 E5:0.5 D5 B4:0.5 G4:0.75 A4:0.25 B4:0.5 \
    C5 A4:0.5 A4:0.75 G#4:0.25 A4:0.5 B4 G#4:0.5 E4 A4:0.5 \
    C5 D5:0.5 E5:0.75 F5:0.25 E5:0.5 D5 B4:0.5 G4:0.75 A4:0.25 B4:0.5 \
    C5:0.75 B4:0.25 A4:0.5 G#4:0.75 F#4:0.25 G#4:0.5 A4:2.5";

/// Beethoven, Bagatelle in A minor (1810), the opening phrase.
const FUR_ELISE: &str = "\
    E5:0.25 D#5:0.25 E5:0.25 D#5:0.25 E5:0.25 B4:0.25 D5:0.25 C5:0.25 A4:0.5 \
    r:0.25 C4:0.25 E4:0.25 A4:0.25 B4:0.5 r:0.25 E4:0.25 G#4:0.25 B4:0.25 C5:0.5 \
    r:0.25 E4:0.25 E5:0.25 D#5:0.25 \
    E5:0.25 D#5:0.25 E5:0.25 B4:0.25 D5:0.25 C5:0.25 A4:0.5 \
    r:0.25 C4:0.25 E4:0.25 A4:0.25 B4:0.5 r:0.25 E4:0.25 C5:0.25 B4:0.25 A4:1";

/// Traditional French round.
const FRERE_JACQUES: &str = "\
    C4 D4 E4 C4 C4 D4 E4 C4 E4 F4 G4:2 E4 F4 G4:2 \
    G4:0.5 A4:0.5 G4:0.5 F4:0.5 E4 C4 G4:0.5 A4:0.5 G4:0.5 F4:0.5 E4 C4 \
    C4 G3 C4:2 C4 G3 C4:2";

pub const BUILT_IN: [BuiltIn; 10] = [
    tune("Ode to Joy", 110.0, ODE_TO_JOY),
    tune("Twinkle Twinkle Little Star", 100.0, TWINKLE),
    tune("Happy Birthday", 100.0, HAPPY_BIRTHDAY),
    tune("Greensleeves", 90.0, GREENSLEEVES),
    tune("F\u{fc}r Elise (opening)", 70.0, FUR_ELISE),
    tune("Fr\u{e8}re Jacques", 110.0, FRERE_JACQUES),
    tune(
        "Major scale (C)",
        100.0,
        "C4 D4 E4 F4 G4 A4 B4 C5 B4 A4 G4 F4 E4 D4 C4:2",
    ),
    tune(
        "Minor scale (A)",
        100.0,
        "A3 B3 C4 D4 E4 F4 G4 A4 G4 F4 E4 D4 C4 B3 A3:2",
    ),
    tune(
        "Pentatonic run (C)",
        100.0,
        "C4 D4 E4 G4 A4 C5 A4 G4 E4 D4 C4:2",
    ),
    tune("Major arpeggio (C)", 100.0, "C4 E4 G4 C5 G4 E4 C4:2"),
];

/// The built-in melody with this name.
pub fn get(name: &str) -> Option<Melody> {
    let b = BUILT_IN.iter().find(|b| b.name == name)?;
    Some(Melody {
        name: b.name.to_string(),
        notes: parse(b.spec).expect("built-in melodies are valid"),
        bpm: b.bpm,
    })
}

/// "C#4" -> 61.
fn key(name: &str) -> Option<u8> {
    let mut chars = name.chars();
    let base: i32 = match chars.next()?.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest = chars.as_str();
    let (accidental, octave) = match rest.strip_prefix('#') {
        Some(o) => (1, o),
        None => match rest.strip_prefix('b') {
            Some(o) => (-1, o),
            None => (0, rest),
        },
    };
    let octave: i32 = octave.parse().ok()?;
    u8::try_from(12 * (octave + 1) + base + accidental)
        .ok()
        .filter(|k| *k < 128)
}

/// Parses the notation described at the top of this file.
pub fn parse(spec: &str) -> Result<Vec<Note>, String> {
    let mut notes = Vec::new();
    let mut t = 0.0f32;
    for token in spec.split_whitespace() {
        let (name, beats) = match token.split_once(':') {
            Some((n, b)) => (
                n,
                b.parse::<f32>()
                    .map_err(|_| format!("bad length in {token}"))?,
            ),
            None => (token, 1.0),
        };
        if beats.is_nan() || beats <= 0.0 {
            return Err(format!("bad length in {token}"));
        }
        if name != "r" {
            let key = key(name).ok_or_else(|| format!("bad note {token}"))?;
            notes.push(Note {
                start: t,
                len: beats,
                key,
            });
        }
        t += beats;
    }
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_names_parse() {
        assert_eq!(key("C4"), Some(60));
        assert_eq!(key("A4"), Some(69));
        assert_eq!(key("C#5"), Some(73));
        assert_eq!(key("Bb3"), Some(58));
        assert_eq!(key("G#4"), Some(68));
        assert_eq!(key("H4"), None);
        assert_eq!(key("C"), None);
        let notes = parse("E4 r:0.5 D4:1.5 C4:0.25").unwrap();
        assert_eq!(
            notes,
            vec![
                Note {
                    start: 0.0,
                    len: 1.0,
                    key: 64
                },
                Note {
                    start: 1.5,
                    len: 1.5,
                    key: 62
                },
                Note {
                    start: 3.0,
                    len: 0.25,
                    key: 60
                },
            ]
        );
        assert!(parse("E4:0").is_err());
        assert!(parse("X9").is_err());
    }

    #[test]
    fn every_built_in_melody_is_valid_and_singable() {
        for b in &BUILT_IN {
            let m = get(b.name).unwrap();
            assert!(m.notes.len() >= 7, "{}", b.name);
            // Within a comfortable two-octave range around middle C.
            assert!(m.lowest() >= 55 && m.highest() <= 81, "{}", b.name);
            // Notes are in order and never overlap (a melody, one note at a time).
            for w in m.notes.windows(2) {
                assert!(w[1].start >= w[0].end() - 1e-4, "{}", b.name);
            }
        }
        assert!(get("No such song").is_none());
    }

    #[test]
    fn the_tunes_are_the_right_tunes() {
        // Spot checks: first notes and lengths, which a typo would break.
        let ode = get("Ode to Joy").unwrap();
        assert_eq!(ode.notes.len(), 62);
        assert_eq!(ode.beats(), 64.0, "16 bars of 4/4");
        let first: Vec<u8> = ode.notes.iter().take(4).map(|n| n.key).collect();
        assert_eq!(first, [64, 64, 65, 67]);
        let twinkle = get("Twinkle Twinkle Little Star").unwrap();
        assert_eq!(twinkle.notes.len(), 42);
        assert_eq!(twinkle.beats(), 48.0);
        let birthday = get("Happy Birthday").unwrap();
        assert_eq!(birthday.notes.len(), 25, "6 + 6 + 7 + 6 syllables");
        assert_eq!(
            birthday.beats(),
            25.0,
            "a one-beat pickup and 8 bars of 3/4"
        );
        let jacques = get("Fr\u{e8}re Jacques").unwrap();
        assert_eq!(jacques.beats(), 32.0);
        let elise = get("F\u{fc}r Elise (opening)").unwrap();
        assert_eq!(elise.notes[0].key, 76);
        assert_eq!(elise.notes[1].key, 75);
        let green = get("Greensleeves").unwrap();
        assert_eq!(green.notes[0].key, 69);
        assert_eq!(
            green.beats(),
            24.0,
            "a pickup, then 8 bars of 6/8 ending on A"
        );
        assert_eq!(elise.notes.len(), 35);
    }
}
