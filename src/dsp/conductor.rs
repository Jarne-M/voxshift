//! Melody Voice's note logic: which note your voice should be on right now.
//!
//! Pure bookkeeping, no audio: it is fed one pitch-tracker frame at a time (every ~2.5 ms) and
//! answers with the target pitch (a MIDI note number), or `None` to leave the voice alone.

use super::params::{P, Snapshot};

/// How the target note is chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Every new syllable moves to the next note of the melody.
    PerSyllable,
    /// The melody plays at its own tempo; you sing whatever note is playing.
    Tempo,
    /// A hotkey moves to the next note.
    Manual,
    /// No melody: the nearest note of a key/scale (auto-tune).
    ScaleSnap,
}

impl Mode {
    pub const ALL: [Mode; 4] = [
        Mode::PerSyllable,
        Mode::Tempo,
        Mode::Manual,
        Mode::ScaleSnap,
    ];

    pub fn from_index(i: usize) -> Self {
        Self::ALL.get(i).copied().unwrap_or(Mode::PerSyllable)
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::PerSyllable => "Per syllable",
            Mode::Tempo => "Tempo-synced",
            Mode::Manual => "Manual step (hotkey)",
            Mode::ScaleSnap => "Scale snap (auto-tune)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scale {
    Major,
    Minor,
    MajorPentatonic,
    MinorPentatonic,
    Chromatic,
}

impl Scale {
    pub const ALL: [Scale; 5] = [
        Scale::Major,
        Scale::Minor,
        Scale::MajorPentatonic,
        Scale::MinorPentatonic,
        Scale::Chromatic,
    ];

    pub fn from_index(i: usize) -> Self {
        Self::ALL.get(i).copied().unwrap_or(Scale::Major)
    }

    pub fn label(self) -> &'static str {
        match self {
            Scale::Major => "Major",
            Scale::Minor => "Minor",
            Scale::MajorPentatonic => "Major pentatonic",
            Scale::MinorPentatonic => "Minor pentatonic",
            Scale::Chromatic => "Chromatic",
        }
    }

    /// Semitones above the root that belong to the scale.
    fn steps(self) -> &'static [i32] {
        match self {
            Scale::Major => &[0, 2, 4, 5, 7, 9, 11],
            Scale::Minor => &[0, 2, 3, 5, 7, 8, 10],
            Scale::MajorPentatonic => &[0, 2, 4, 7, 9],
            Scale::MinorPentatonic => &[0, 3, 5, 7, 10],
            Scale::Chromatic => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        }
    }

    pub fn contains(self, key: i32, note: i32) -> bool {
        self.steps().contains(&(note - key).rem_euclid(12))
    }
}

pub const KEY_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// A pitch may wander this far past the half-way point between two notes before the snap
/// lets go of the note it is on (no flicker between two notes on a wobbly voice).
const SNAP_HYSTERESIS: f32 = 0.2;

/// The note of the scale nearest to `midi`, preferring `current` while it is still close.
pub fn snap(midi: f32, key: i32, scale: Scale, current: Option<i32>) -> i32 {
    if let Some(c) = current
        && scale.contains(key, c)
        && (midi - c as f32).abs() < 0.5 + SNAP_HYSTERESIS
    {
        return c;
    }
    let centre = midi.round() as i32;
    (centre - 3..=centre + 3)
        .filter(|&n| scale.contains(key, n))
        .min_by(|&a, &b| (midi - a as f32).abs().total_cmp(&(midi - b as f32).abs()))
        .unwrap_or(centre)
}

/// The settings the conductor needs, read from the parameters once per block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub mode: Mode,
    pub key: i32,
    pub scale: Scale,
    pub transpose: f32,
}

impl Settings {
    pub fn from_params(p: &Snapshot) -> Self {
        Self {
            mode: Mode::from_index(p.choice(P::MelodyMode)),
            key: p.choice(P::MelodyKey) as i32,
            scale: Scale::from_index(p.choice(P::MelodyScale)),
            transpose: p.get(P::MelodyTranspose).round(),
        }
    }
}

/// One pitch-tracker frame as the conductor sees it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Frame {
    /// Detected pitch (MIDI note number) if the voice is voiced.
    pub detected: Option<f32>,
    /// Frame level (linear RMS).
    pub level: f32,
}

#[derive(Default)]
pub struct Conductor {
    snapped: Option<i32>,
}

impl Conductor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Advances by one frame; returns the target pitch.
    pub fn frame(&mut self, f: Frame, s: &Settings) -> Option<f32> {
        match s.mode {
            Mode::ScaleSnap => {
                let Some(m) = f.detected else {
                    self.snapped = None;
                    return None;
                };
                let n = snap(m, s.key, s.scale, self.snapped);
                self.snapped = Some(n);
                Some(n as f32 + s.transpose)
            }
            // The melody modes arrive in the next stages; until then the voice passes.
            Mode::PerSyllable | Mode::Tempo | Mode::Manual => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapping_picks_the_nearest_scale_note() {
        // C major: C# (61) is not in the scale; 61.2 is 1.2 from C and 0.8 from D.
        assert_eq!(snap(61.2, 0, Scale::Major, None), 62);
        assert_eq!(snap(60.4, 0, Scale::Major, None), 60);
        assert_eq!(snap(63.6, 0, Scale::Major, None), 64); // D# -> E
        // A minor pentatonic (A C D E G): B (71) lies between A (69) and C (72) -> C is nearer.
        assert_eq!(snap(71.0, 9, Scale::MinorPentatonic, None), 72);
        assert_eq!(snap(70.0, 9, Scale::MinorPentatonic, None), 69);
        // Chromatic: just rounding.
        assert_eq!(snap(66.4, 5, Scale::Chromatic, None), 66);
        // Other keys: D major has F# (66), not F (65).
        assert_eq!(snap(65.2, 2, Scale::Major, None), 66);
        assert!(Scale::Major.contains(2, 66) && !Scale::Major.contains(2, 65));
    }

    #[test]
    fn snapping_holds_a_note_against_wobble() {
        // Between E (64) and F (65) in C major: the note it is on wins near the middle.
        assert_eq!(snap(64.6, 0, Scale::Major, Some(64)), 64);
        assert_eq!(snap(64.6, 0, Scale::Major, None), 65);
        assert_eq!(snap(64.8, 0, Scale::Major, Some(64)), 65, "clearly moved");
        // A held note that is not in the (new) scale is dropped (a tie goes to the lower one).
        assert_eq!(snap(61.0, 0, Scale::Major, Some(61)), 60);
    }

    #[test]
    fn scale_snap_mode_follows_the_voice_and_transposes() {
        let mut c = Conductor::new();
        let mut s = Settings {
            mode: Mode::ScaleSnap,
            key: 0,
            scale: Scale::Major,
            transpose: 0.0,
        };
        let frame = |m: Option<f32>| Frame {
            detected: m,
            level: 0.1,
        };
        assert_eq!(c.frame(frame(Some(57.3)), &s), Some(57.0));
        assert_eq!(c.frame(frame(Some(58.6)), &s), Some(59.0)); // A# -> B
        assert_eq!(c.frame(frame(None), &s), None, "unvoiced: leave it alone");
        s.transpose = 12.0;
        assert_eq!(c.frame(frame(Some(60.1)), &s), Some(72.0));
    }

    #[test]
    fn settings_come_from_the_parameters() {
        let mut p = Snapshot::defaults();
        p.set(P::MelodyMode, 3.0);
        p.set(P::MelodyKey, 9.0);
        p.set(P::MelodyScale, 1.0);
        p.set(P::MelodyTranspose, -7.2);
        let s = Settings::from_params(&p);
        assert_eq!(s.mode, Mode::ScaleSnap);
        assert_eq!((s.key, s.scale, s.transpose), (9, Scale::Minor, -7.0));
        assert_eq!(Mode::from_index(99), Mode::PerSyllable);
    }
}
