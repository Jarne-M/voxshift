//! "Play melody": the melody as a simple synth tone (a sine with two soft overtones, like a
//! flute or an organ), for your headphones only, so you can hear the tune you are aiming for.
//! It plays at the tempo slider's speed and with the transpose applied, as written (Melody
//! Voice itself may move it by octaves to suit your voice).

use crate::melody::Note;
use crate::melody::store::{MAX_NOTES, MelodyStore};

const ATTACK_S: f32 = 0.01;
const RELEASE_S: f32 = 0.03;
const LEVEL: f32 = 0.3;

pub struct Preview {
    fs: f32,
    notes: Vec<Note>,
    seen: Option<u32>,
    playing: bool,
    beat: f32,
    /// First note that has not ended yet.
    cursor: usize,
    /// Index of the note sounding (it keeps its pitch while it fades out).
    sounding: Option<usize>,
    phase: f32,
    freq: f32,
    env: f32,
    attack: f32,
    release: f32,
}

impl Preview {
    pub fn new(fs: f32) -> Self {
        Self {
            fs,
            notes: Vec::with_capacity(MAX_NOTES),
            seen: None,
            playing: false,
            beat: 0.0,
            cursor: 0,
            sounding: None,
            phase: 0.0,
            freq: 0.0,
            env: 0.0,
            attack: 1.0 / (ATTACK_S * fs),
            release: (-1.0 / (RELEASE_S * fs)).exp(),
        }
    }

    /// Picks up a newly published melody (stops playing the old one).
    pub fn sync(&mut self, store: &MelodyStore) {
        if self.seen == Some(store.version()) {
            return;
        }
        if let Some(v) = store.read_into(&mut self.notes) {
            self.seen = Some(v);
            self.playing = false;
        }
    }

    pub fn start(&mut self) {
        self.playing = !self.notes.is_empty();
        self.beat = self.notes.first().map_or(0.0, |n| n.start);
        self.cursor = 0;
        self.sounding = None;
    }

    /// Stops (the last note fades out, no click).
    pub fn stop(&mut self) {
        self.playing = false;
    }

    pub fn playing(&self) -> bool {
        self.playing
    }

    /// True while anything is still audible (playing, or the last note fading).
    pub fn active(&self) -> bool {
        self.playing || self.env > 1e-4
    }

    /// Playhead in beats.
    pub fn beat(&self) -> f32 {
        self.beat
    }

    /// Adds one block of the tune (mono) to `out`.
    pub fn render(&mut self, out: &mut [f32], bpm: f32, transpose: f32) {
        let beats_per_sample = bpm / 60.0 / self.fs;
        for o in out.iter_mut() {
            let mut gate = false;
            if self.playing {
                self.beat += beats_per_sample;
                while self
                    .notes
                    .get(self.cursor)
                    .is_some_and(|n| n.start + n.len <= self.beat)
                {
                    self.cursor += 1;
                }
                match self.notes.get(self.cursor) {
                    None => self.playing = false, // the end
                    Some(n) if n.start <= self.beat => {
                        // Let go a little before the end so repeated notes are heard as two.
                        let gap = (RELEASE_S * bpm / 60.0).min(n.len * 0.25);
                        gate = self.beat < n.start + n.len - gap;
                        if gate && self.sounding != Some(self.cursor) {
                            self.sounding = Some(self.cursor);
                            let key = f32::from(n.key) + transpose;
                            self.freq = 440.0 * ((key - 69.0) / 12.0).exp2();
                        }
                    }
                    Some(_) => {} // a rest
                }
            }
            if gate {
                self.env = (self.env + self.attack).min(1.0);
            } else {
                self.env *= self.release;
            }
            if self.env < 1e-5 {
                self.env = 0.0;
                continue;
            }
            self.phase = (self.phase + self.freq / self.fs).fract();
            let w = std::f32::consts::TAU * self.phase;
            let tone = w.sin() + 0.35 * (2.0 * w).sin() + 0.12 * (3.0 * w).sin();
            *o += LEVEL * self.env * tone;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn rig(keys: &[(u8, f32, f32)]) -> (Preview, MelodyStore) {
        let store = MelodyStore::default();
        let notes: Vec<Note> = keys
            .iter()
            .map(|&(key, start, len)| Note { start, len, key })
            .collect();
        store.publish(&notes);
        let mut p = Preview::new(FS);
        p.sync(&store);
        (p, store)
    }

    fn play(p: &mut Preview, seconds: f32, bpm: f32, transpose: f32) -> Vec<f32> {
        let mut out = vec![0.0; (seconds * FS) as usize];
        for chunk in out.chunks_mut(64) {
            p.render(chunk, bpm, transpose);
        }
        out
    }

    #[test]
    fn plays_each_note_in_time_and_stops_at_the_end() {
        // C4 then G4, one beat each at 120 BPM: half a second each.
        let (mut p, _store) = rig(&[(60, 0.0, 1.0), (67, 1.0, 1.0)]);
        assert!(!p.active());
        p.start();
        let out = play(&mut p, 1.3, 120.0, 0.0);
        let pitch = |from: f32| {
            let s = &out[(from * FS) as usize..((from + 0.2) * FS) as usize];
            crate::dsp::melody::tests::measured_midi(s, 0.05).unwrap()
        };
        assert!((pitch(0.1) - 60.0).abs() < 0.1, "{}", pitch(0.1));
        assert!((pitch(0.6) - 67.0).abs() < 0.1, "{}", pitch(0.6));
        assert!(!p.playing(), "finished after the last note");
        assert!(
            out[(1.2 * FS) as usize..].iter().all(|x| x.abs() < 1e-3),
            "silent after"
        );
        assert!(out.iter().all(|x| x.abs() < 0.6));
    }

    #[test]
    fn transpose_and_no_clicks_between_repeated_notes() {
        let (mut p, _store) = rig(&[(60, 0.0, 1.0), (60, 1.0, 1.0), (60, 2.0, 1.0)]);
        p.start();
        let out = play(&mut p, 1.6, 120.0, 7.0);
        let got = crate::dsp::melody::tests::measured_midi(&out[..(0.4 * FS) as usize], 0.05);
        assert!((got.unwrap() - 67.0).abs() < 0.1, "{got:?}");
        let step = out
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        // A 392 Hz tone at 0.3 with two overtones moves at most ~0.04 per sample.
        assert!(step < 0.05, "step {step}");
        // Repeated notes are separated by a short dip: the loudness (peak over 3 ms, a bit
        // more than one period) drops well below the steady level around the note change.
        let peak_at = |t: f32| {
            let s = &out[(t * FS) as usize..((t + 0.003) * FS) as usize];
            s.iter().fold(0.0f32, |m, x| m.max(x.abs()))
        };
        let dip = (0..10)
            .map(|i| peak_at(0.49 + 0.002 * i as f32))
            .fold(f32::MAX, f32::min);
        assert!(dip < 0.5 * peak_at(0.25), "the gap between notes: {dip}");
    }

    #[test]
    fn stop_fades_and_a_new_melody_stops_the_old_one() {
        let (mut p, store) = rig(&[(69, 0.0, 8.0)]);
        p.start();
        play(&mut p, 0.2, 100.0, 0.0);
        p.stop();
        let out = play(&mut p, 0.4, 100.0, 0.0);
        assert!(out[(0.25 * FS) as usize..].iter().all(|x| x.abs() < 1e-3));
        assert!(!p.active());
        let step = out
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        assert!(step < 0.05);

        p.start();
        store.publish(&[]);
        p.sync(&store);
        assert!(!p.playing());
        // An empty melody never starts.
        p.start();
        assert!(!p.playing());
    }
}
