//! Melody Voice's note logic: which note your voice should be on right now.
//!
//! Pure bookkeeping, no audio: it is fed one pitch-tracker frame at a time (every ~2.5 ms) and
//! answers with the target pitch (a MIDI note number), or `None` to leave the voice alone.

use super::params::{P, Snapshot};
use crate::melody::{Note, median_key};

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
    /// Start over at the end of the melody (otherwise: stop retuning).
    pub looped: bool,
    /// Move the melody by whole octaves to sit around your voice.
    pub fold: bool,
    /// 0..1: how readily a new syllable is heard.
    pub sensitivity: f32,
    /// A note lasts at least this long before the next syllable may move on (seconds).
    pub min_note_s: f32,
    /// Back to the first note after this much silence (seconds; 0 = never).
    pub reset_after_s: f32,
    pub bpm: f32,
}

impl Settings {
    pub fn from_params(p: &Snapshot) -> Self {
        Self {
            mode: Mode::from_index(p.choice(P::MelodyMode)),
            key: p.choice(P::MelodyKey) as i32,
            scale: Scale::from_index(p.choice(P::MelodyScale)),
            transpose: p.get(P::MelodyTranspose).round(),
            looped: p.on(P::MelodyLoop),
            fold: p.on(P::MelodyFold),
            sensitivity: p.get(P::MelodySensitivity),
            min_note_s: p.get(P::MelodyMinNote) / 1000.0,
            reset_after_s: p.get(P::MelodyResetAfter),
            bpm: p.get(P::MelodyBpm),
        }
    }

    /// Unvoiced gaps shorter than this are glitches inside a syllable, not a new one.
    fn gap_s(&self) -> f32 {
        0.06 - 0.04 * self.sensitivity
    }

    /// A syllable boundary inside voiced speech: the level dips by this much and comes back.
    fn dip_db(&self) -> f32 {
        9.0 - 6.0 * self.sensitivity
    }

    /// A sudden pitch jump this big (semitones) also starts a new syllable.
    fn break_st(&self) -> f32 {
        4.0 - 2.0 * self.sensitivity
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

/// Where in the melody we are, for the display.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Position {
    /// Index of the current note.
    pub note: Option<usize>,
    /// Playhead in beats.
    pub beat: f32,
    /// Reached the end (without loop): the voice is left alone until a restart.
    pub finished: bool,
}

/// The peak level slowly falls this fast, so the natural fade of a long vowel is not taken
/// for a syllable boundary.
const PEAK_DECAY_DB_S: f32 = 6.0;
/// Frames a pitch jump must last to count as a break (not a detector slip).
const BREAK_FRAMES: u32 = 2;
/// Time constant of "where your voice sits" for octave folding.
const CENTRE_S: f32 = 3.0;
/// The melody is moved by an octave once its middle is this far from your voice.
const FOLD_SLACK: f32 = 8.0;

pub struct Conductor {
    snapped: Option<i32>,
    /// Seconds per frame.
    dt: f32,
    pos: Position,
    note_age: f32,
    silence: f32,
    voiced_before: bool,
    level_db: f32,
    peak_db: f32,
    valley_db: f32,
    dipped: bool,
    pitch_ref: f32,
    break_run: u32,
    centre: Option<f32>,
    fold: f32,
    melody_middle: f32,
}

impl Conductor {
    /// `dt`: time between frames.
    pub fn new(dt: f32) -> Self {
        Self {
            snapped: None,
            dt,
            pos: Position::default(),
            note_age: 0.0,
            silence: f32::INFINITY,
            voiced_before: false,
            level_db: -120.0,
            peak_db: -120.0,
            valley_db: -120.0,
            dipped: false,
            pitch_ref: 0.0,
            break_run: 0,
            centre: None,
            fold: 0.0,
            melody_middle: 60.0,
        }
    }

    pub fn position(&self) -> Position {
        self.pos
    }

    /// A new melody: start from its beginning.
    pub fn set_melody(&mut self, notes: &[Note]) {
        self.melody_middle = median_key(notes);
        self.restart();
    }

    /// Back to the beginning (the restart button / hotkey).
    pub fn restart(&mut self) {
        self.pos = Position::default();
        self.note_age = 0.0;
    }

    /// Advances by one frame; returns the target pitch.
    pub fn frame(&mut self, f: Frame, s: &Settings, notes: &[Note]) -> Option<f32> {
        if let Some(m) = f.detected {
            let k = (self.dt / CENTRE_S).min(1.0);
            self.centre = Some(self.centre.map_or(m, |c| c + (m - c) * k));
        }
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
            Mode::PerSyllable => self.per_syllable(f, s, notes),
            // Tempo-synced and manual stepping arrive in the next stage.
            Mode::Tempo | Mode::Manual => None,
        }
    }

    /// One syllable, one note.
    fn per_syllable(&mut self, f: Frame, s: &Settings, notes: &[Note]) -> Option<f32> {
        let Some(m) = f.detected else {
            self.silence += self.dt;
            self.voiced_before = false;
            if s.reset_after_s > 0.0 && self.silence >= s.reset_after_s {
                self.restart();
            }
            return None;
        };
        let fresh = !self.voiced_before;
        let onset = if fresh {
            // Voice is back. After a real pause that is a new syllable; after a glitch of a
            // few ms it is the same one going on.
            self.start_segment(f.level, m);
            self.silence >= s.gap_s()
        } else {
            // Within voiced speech ("hello", "my love"): a dip in loudness or a jump in pitch.
            let dip = self.level_dip(f.level, s);
            let jump = self.pitch_break(m, s);
            dip || jump
        };
        self.silence = 0.0;
        self.voiced_before = true;
        self.note_age += self.dt;
        if onset && (self.pos.note.is_none() || self.note_age >= s.min_note_s) {
            self.advance(notes, s);
        }
        self.target(notes, s)
    }

    fn start_segment(&mut self, level: f32, m: f32) {
        self.level_db = to_db(level);
        self.peak_db = self.level_db;
        self.dipped = false;
        self.pitch_ref = m;
        self.break_run = 0;
    }

    /// True when the level has dipped and come back up: a syllable boundary.
    fn level_dip(&mut self, level: f32, s: &Settings) -> bool {
        self.level_db += (to_db(level) - self.level_db) * 0.5;
        if self.level_db > self.peak_db {
            self.peak_db = self.level_db;
        } else {
            self.peak_db -= PEAK_DECAY_DB_S * self.dt;
        }
        let dip = s.dip_db();
        if !self.dipped {
            if self.level_db < self.peak_db - dip {
                self.dipped = true;
                self.valley_db = self.level_db;
            }
            return false;
        }
        self.valley_db = self.valley_db.min(self.level_db);
        if self.level_db > self.valley_db + 0.7 * dip {
            self.dipped = false;
            self.peak_db = self.level_db;
            return true;
        }
        false
    }

    /// True when the pitch jumped and stayed there.
    fn pitch_break(&mut self, m: f32, s: &Settings) -> bool {
        if (m - self.pitch_ref).abs() > s.break_st() {
            self.break_run += 1;
            if self.break_run >= BREAK_FRAMES {
                self.pitch_ref = m;
                self.break_run = 0;
                return true;
            }
        } else {
            self.break_run = 0;
            self.pitch_ref = m;
        }
        false
    }

    /// On to the next note (or around / to the end).
    fn advance(&mut self, notes: &[Note], s: &Settings) {
        if notes.is_empty() || self.pos.finished {
            return;
        }
        let next = self.pos.note.map_or(0, |i| i + 1);
        let next = if next < notes.len() {
            next
        } else if s.looped {
            0
        } else {
            self.pos.finished = true;
            return;
        };
        self.pos.note = Some(next);
        self.pos.beat = notes[next].start;
        self.note_age = 0.0;
        self.update_fold(s);
    }

    /// Picks the octave shift that keeps the melody around your voice. It only changes when
    /// your voice is clearly elsewhere, and only between notes, so it never jumps mid-note.
    fn update_fold(&mut self, s: &Settings) {
        if !s.fold {
            self.fold = 0.0;
            return;
        }
        let Some(centre) = self.centre else { return };
        let middle = self.melody_middle + s.transpose;
        if (centre - (middle + self.fold)).abs() > FOLD_SLACK {
            self.fold = 12.0 * ((centre - middle) / 12.0).round();
        }
    }

    fn target(&self, notes: &[Note], s: &Settings) -> Option<f32> {
        if self.pos.finished {
            return None;
        }
        let note = notes.get(self.pos.note?)?;
        let fold = if s.fold { self.fold } else { 0.0 };
        Some(f32::from(note.key) + s.transpose + fold)
    }
}

fn to_db(level: f32) -> f32 {
    20.0 * level.max(1e-6).log10()
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

    const DT: f32 = 0.0025;

    fn settings(mode: Mode) -> Settings {
        Settings::from_params(&{
            let mut p = Snapshot::defaults();
            p.set(P::MelodyMode, mode as usize as f32);
            p.set(P::MelodyFold, 0.0);
            p
        })
    }

    fn notes(keys: &[u8]) -> Vec<Note> {
        keys.iter()
            .enumerate()
            .map(|(i, &key)| Note {
                start: i as f32,
                len: 1.0,
                key,
            })
            .collect()
    }

    /// Feeds `seconds` of frames; returns the last target.
    fn feed(
        c: &mut Conductor,
        s: &Settings,
        n: &[Note],
        seconds: f32,
        pitch: Option<f32>,
        level: f32,
    ) -> Option<f32> {
        let mut out = None;
        for _ in 0..(seconds / DT).round() as usize {
            out = c.frame(
                Frame {
                    detected: pitch,
                    level,
                },
                s,
                n,
            );
        }
        out
    }

    /// One spoken syllable: `seconds` of voice at `pitch`, then a pause.
    fn syllable(
        c: &mut Conductor,
        s: &Settings,
        n: &[Note],
        pitch: f32,
        pause: f32,
    ) -> Option<f32> {
        let t = feed(c, s, n, 0.2, Some(pitch), 0.1);
        feed(c, s, n, pause, None, 0.001);
        t
    }

    #[test]
    fn scale_snap_mode_follows_the_voice_and_transposes() {
        let mut c = Conductor::new(DT);
        let mut s = settings(Mode::ScaleSnap);
        let f = |m: Option<f32>| Frame {
            detected: m,
            level: 0.1,
        };
        assert_eq!(c.frame(f(Some(57.3)), &s, &[]), Some(57.0));
        assert_eq!(c.frame(f(Some(58.6)), &s, &[]), Some(59.0)); // A# -> B
        assert_eq!(c.frame(f(None), &s, &[]), None, "unvoiced: leave it alone");
        s.transpose = 12.0;
        assert_eq!(c.frame(f(Some(60.1)), &s, &[]), Some(72.0));
    }

    #[test]
    fn each_syllable_sings_the_next_note() {
        let mut c = Conductor::new(DT);
        let s = settings(Mode::PerSyllable);
        let n = notes(&[60, 62, 64, 65]);
        assert_eq!(c.position().note, None, "nothing before the first syllable");
        // Whatever pitch you speak at, each syllable gets the next note.
        for (i, pitch) in [50.0, 52.0, 49.0, 51.0].into_iter().enumerate() {
            let target = syllable(&mut c, &s, &n, pitch, 0.1);
            assert_eq!(target, Some(f32::from(n[i].key)), "syllable {i}");
            assert_eq!(c.position().note, Some(i));
            assert_eq!(c.position().beat, i as f32);
        }
    }

    #[test]
    fn short_glitches_and_too_quick_syllables_do_not_skip_notes() {
        let mut c = Conductor::new(DT);
        let s = settings(Mode::PerSyllable);
        let n = notes(&[60, 62, 64, 65]);
        feed(&mut c, &s, &n, 0.1, Some(50.0), 0.1);
        // A 10 ms dropout inside the vowel: same syllable.
        feed(&mut c, &s, &n, 0.01, None, 0.001);
        feed(&mut c, &s, &n, 0.1, Some(50.0), 0.1);
        assert_eq!(c.position().note, Some(0));
        feed(&mut c, &s, &n, 0.1, None, 0.001);
        // A real pause, then a short syllable: the next note.
        feed(&mut c, &s, &n, 0.03, Some(50.0), 0.1);
        assert_eq!(c.position().note, Some(1));
        feed(&mut c, &s, &n, 0.05, None, 0.001);
        // Another syllable only 80 ms after that note started is under the 150 ms minimum
        // note length: still note 1.
        feed(&mut c, &s, &n, 0.05, Some(50.0), 0.1);
        assert_eq!(c.position().note, Some(1), "debounced");
    }

    #[test]
    fn a_dip_in_loudness_inside_voiced_speech_is_a_new_syllable() {
        // "hel-lo" without any unvoiced sound in between: loud, 12 dB quieter, loud again.
        let mut c = Conductor::new(DT);
        let s = settings(Mode::PerSyllable);
        let n = notes(&[60, 67]);
        feed(&mut c, &s, &n, 0.2, Some(50.0), 0.1);
        feed(&mut c, &s, &n, 0.05, Some(50.0), 0.025);
        assert_eq!(c.position().note, Some(0));
        let t = feed(&mut c, &s, &n, 0.2, Some(50.0), 0.1);
        assert_eq!(c.position().note, Some(1));
        assert_eq!(t, Some(67.0));
        // A long vowel slowly fading (3 dB per second) is not a boundary.
        let mut c = Conductor::new(DT);
        let mut level = 0.1f32;
        for _ in 0..800 {
            level *= 10f32.powf(-3.0 / 20.0 * DT);
            c.frame(
                Frame {
                    detected: Some(50.0),
                    level,
                },
                &s,
                &n,
            );
        }
        assert_eq!(c.position().note, Some(0));
    }

    #[test]
    fn a_jump_in_pitch_is_a_new_syllable_but_a_glide_is_not() {
        let mut c = Conductor::new(DT);
        let s = settings(Mode::PerSyllable);
        let n = notes(&[60, 62, 64]);
        feed(&mut c, &s, &n, 0.2, Some(50.0), 0.1);
        // Gliding up 6 semitones over 300 ms (speech intonation): no new note.
        for i in 0..120 {
            c.frame(
                Frame {
                    detected: Some(50.0 + 6.0 * i as f32 / 120.0),
                    level: 0.1,
                },
                &s,
                &n,
            );
        }
        assert_eq!(c.position().note, Some(0));
        // A sudden 5 semitone jump that stays: next note.
        feed(&mut c, &s, &n, 0.1, Some(61.0), 0.1);
        assert_eq!(c.position().note, Some(1));
    }

    #[test]
    fn loop_end_restart_and_reset_after_silence() {
        let n = notes(&[60, 62]);
        let mut s = settings(Mode::PerSyllable);
        let mut c = Conductor::new(DT);
        for _ in 0..2 {
            syllable(&mut c, &s, &n, 50.0, 0.1);
        }
        assert_eq!(
            syllable(&mut c, &s, &n, 50.0, 0.1),
            Some(60.0),
            "loops around"
        );

        s.looped = false;
        let mut c = Conductor::new(DT);
        syllable(&mut c, &s, &n, 50.0, 0.1);
        syllable(&mut c, &s, &n, 50.0, 0.1);
        assert_eq!(syllable(&mut c, &s, &n, 50.0, 0.1), None, "song over");
        assert!(c.position().finished);
        c.restart();
        assert_eq!(syllable(&mut c, &s, &n, 50.0, 0.1), Some(60.0));

        s.reset_after_s = 2.0;
        syllable(&mut c, &s, &n, 50.0, 1.0); // note 1, then 1 s of silence: keeps going
        assert_eq!(c.position().note, Some(1));
        let mut c2 = Conductor::new(DT);
        syllable(&mut c2, &s, &n, 50.0, 2.5);
        assert_eq!(
            c2.position().note,
            None,
            "2.5 s of silence: back to the top"
        );
        assert_eq!(syllable(&mut c2, &s, &n, 50.0, 0.1), Some(60.0));
    }

    #[test]
    fn a_new_melody_starts_from_its_first_note() {
        let mut c = Conductor::new(DT);
        let s = settings(Mode::PerSyllable);
        let a = notes(&[60, 62, 64]);
        syllable(&mut c, &s, &a, 50.0, 0.1);
        syllable(&mut c, &s, &a, 50.0, 0.1);
        let b = notes(&[70, 71]);
        c.set_melody(&b);
        assert_eq!(syllable(&mut c, &s, &b, 50.0, 0.1), Some(70.0));
        // An empty melody leaves the voice alone.
        let mut c = Conductor::new(DT);
        assert_eq!(syllable(&mut c, &s, &[], 50.0, 0.1), None);
    }

    #[test]
    fn octave_fold_moves_the_melody_to_your_voice_and_transpose_shifts_it() {
        // A melody around C5 (72) and a low voice around D3 (50): folded down two octaves.
        let n = notes(&[72, 74, 76]);
        let mut s = settings(Mode::PerSyllable);
        s.fold = true;
        let mut c = Conductor::new(DT);
        c.set_melody(&n);
        assert_eq!(syllable(&mut c, &s, &n, 50.0, 0.1), Some(48.0));
        assert_eq!(syllable(&mut c, &s, &n, 51.0, 0.1), Some(50.0));
        // Without folding the written notes are used; transpose adds on top.
        s.fold = false;
        s.transpose = -5.0;
        assert_eq!(syllable(&mut c, &s, &n, 50.0, 0.1), Some(71.0));
        // A high voice (around A4) keeps the melody where it is.
        s.fold = true;
        s.transpose = 0.0;
        let mut c = Conductor::new(DT);
        c.set_melody(&n);
        assert_eq!(syllable(&mut c, &s, &n, 69.0, 0.1), Some(72.0));
    }

    #[test]
    fn settings_come_from_the_parameters() {
        let mut p = Snapshot::defaults();
        p.set(P::MelodyMode, 3.0);
        p.set(P::MelodyKey, 9.0);
        p.set(P::MelodyScale, 1.0);
        p.set(P::MelodyTranspose, -7.2);
        p.set(P::MelodyMinNote, 200.0);
        p.set(P::MelodyLoop, 0.0);
        let s = Settings::from_params(&p);
        assert_eq!(s.mode, Mode::ScaleSnap);
        assert_eq!((s.key, s.scale, s.transpose), (9, Scale::Minor, -7.0));
        assert!((s.min_note_s - 0.2).abs() < 1e-6 && !s.looped && s.fold);
        assert_eq!(Mode::from_index(99), Mode::PerSyllable);
    }
}
