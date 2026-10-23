//! Melodies for Melody Voice: the note type, the built-in library, MIDI import and the
//! lock-free hand-off to the audio thread.

pub mod library;
pub mod midi;
pub mod store;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Which melody is picked (saved in the settings and in presets).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MelodySource {
    BuiltIn(String),
    /// A MIDI file and the part to sing (`None` = the one VoxShift guesses is the melody).
    File {
        path: PathBuf,
        part: Option<usize>,
    },
}

impl Default for MelodySource {
    fn default() -> Self {
        Self::BuiltIn("Twinkle Twinkle Little Star".into())
    }
}

impl MelodySource {
    /// Short name for lists: the tune's name or the file name.
    pub fn label(&self) -> String {
        match self {
            Self::BuiltIn(name) => name.clone(),
            Self::File { path, .. } => file_label(path),
        }
    }
}

/// "song.mid" -> "song".
pub fn file_label(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// True for .mid / .midi files.
pub fn is_midi(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("mid") || e.eq_ignore_ascii_case("midi"))
}

/// A loaded melody, plus the file's parts when it came from a MIDI file (for the part picker).
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
    pub melody: Melody,
    pub song: Option<midi::MidiSong>,
    /// The part used (MIDI files only).
    pub part: Option<usize>,
}

pub fn load(source: &MelodySource) -> Result<Loaded, String> {
    match source {
        MelodySource::BuiltIn(name) => {
            let melody =
                library::get(name).ok_or_else(|| format!("no built-in melody \"{name}\""))?;
            Ok(Loaded {
                melody,
                song: None,
                part: None,
            })
        }
        MelodySource::File { path, part } => {
            let bytes = std::fs::read(path)
                .map_err(|e| format!("could not read {}: {e}", path.display()))?;
            let song = midi::parse(&bytes)?;
            let part = part
                .filter(|&p| p < song.parts.len())
                .unwrap_or(song.default_part);
            let melody = song
                .melody(part, &file_label(path))
                .ok_or("that part does not exist")?;
            if melody.notes.is_empty() {
                return Err("that part has no notes".into());
            }
            Ok(Loaded {
                melody,
                song: Some(song),
                part: Some(part),
            })
        }
    }
}

/// One note of a melody. Times are in beats (quarter notes) so the tempo can change freely.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    pub start: f32,
    pub len: f32,
    /// MIDI note number (60 = middle C).
    pub key: u8,
}

impl Note {
    pub fn end(&self) -> f32 {
        self.start + self.len
    }
}

/// A melody ready to sing.
#[derive(Clone, Debug, PartialEq)]
pub struct Melody {
    pub name: String,
    pub notes: Vec<Note>,
    /// The tempo it was written at.
    pub bpm: f32,
}

impl Melody {
    /// Length in beats (to the end of the last note).
    pub fn beats(&self) -> f32 {
        self.notes.iter().map(Note::end).fold(0.0, f32::max)
    }

    pub fn lowest(&self) -> u8 {
        self.notes.iter().map(|n| n.key).min().unwrap_or(60)
    }

    pub fn highest(&self) -> u8 {
        self.notes.iter().map(|n| n.key).max().unwrap_or(60)
    }
}

/// The middle key of a set of notes (by count), for octave folding.
pub fn median_key(notes: &[Note]) -> f32 {
    if notes.is_empty() {
        return 60.0;
    }
    let mut keys = [0u32; 128];
    for n in notes {
        keys[usize::from(n.key.min(127))] += 1;
    }
    let half = notes.len().div_ceil(2) as u32;
    let mut seen = 0;
    for (k, c) in keys.iter().enumerate() {
        seen += c;
        if seen >= half {
            return k as f32;
        }
    }
    60.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_key_and_extent() {
        let n = |start, key| Note {
            start,
            len: 1.0,
            key,
        };
        let m = Melody {
            name: "x".into(),
            notes: vec![n(0.0, 60), n(1.0, 64), n(2.0, 72), n(3.5, 62)],
            bpm: 100.0,
        };
        assert_eq!(m.beats(), 4.5);
        assert_eq!((m.lowest(), m.highest()), (60, 72));
        assert_eq!(median_key(&m.notes), 62.0);
        assert_eq!(median_key(&[]), 60.0);
    }

    #[test]
    fn loading_built_ins_and_files() {
        use crate::soundboard::decode::tests::temp_dir;
        let twinkle = load(&MelodySource::default()).unwrap();
        assert_eq!(twinkle.melody.notes[0].key, 60);
        assert!(twinkle.song.is_none());
        assert!(load(&MelodySource::BuiltIn("Nope".into())).is_err());

        let dir = temp_dir("melody-load");
        let path = dir.join("tune.MID");
        assert!(is_midi(&path) && !is_midi(&dir.join("a.wav")));
        let lead: Vec<midi::tests::TestNote> =
            (0..9).map(|i| (0, 70, i * 480, i * 480 + 400)).collect();
        let bass: Vec<midi::tests::TestNote> =
            (0..9).map(|i| (1, 40, i * 480, i * 480 + 400)).collect();
        let bytes =
            midi::tests::build(midly::Format::Parallel, &[("Lead", &lead), ("Bass", &bass)]);
        std::fs::write(&path, bytes).unwrap();
        let guessed = load(&MelodySource::File {
            path: path.clone(),
            part: None,
        })
        .unwrap();
        assert_eq!(guessed.part, Some(0));
        assert_eq!(guessed.melody.name, "tune");
        assert_eq!(guessed.song.as_ref().unwrap().parts.len(), 2);
        let bass = load(&MelodySource::File {
            path: path.clone(),
            part: Some(1),
        })
        .unwrap();
        assert_eq!(bass.melody.notes[0].key, 40);
        // A part number the file does not have falls back to the guess.
        let bad = load(&MelodySource::File {
            path: path.clone(),
            part: Some(9),
        })
        .unwrap();
        assert_eq!(bad.part, Some(0));
        let missing = load(&MelodySource::File {
            path: dir.join("gone.mid"),
            part: None,
        });
        assert!(missing.unwrap_err().contains("could not read"));
        std::fs::remove_dir_all(dir).ok();
    }
}
