//! Standard MIDI file import (with `midly`): split into parts, guess the melody part, and
//! reduce it to one note at a time.
//!
//! A "part" is one channel of one track, so a format-0 file (everything in one track) still
//! offers its instruments separately. Drums (channel 10) are skipped.
//!
//! The default part is the one with the highest average pitch among the parts that are
//! mostly one note at a time: tunes usually sit on top of the accompaniment.
//!
//! Skyline: at each moment notes start, the highest one wins. A lower note that starts while
//! the chosen note is still held is accompaniment and is ignored; a higher one cuts the held
//! note short.

use super::{Melody, Note};
use midly::{MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};

/// One note with times in ticks, before reduction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RawNote {
    pub start: u64,
    pub end: u64,
    pub key: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub track: usize,
    /// 1-based, as musicians count.
    pub channel: u8,
    pub name: String,
    pub notes: Vec<RawNote>,
    pub average_key: f32,
    /// Share of notes that start while another note of the part still sounds (0 = strictly
    /// one note at a time).
    pub polyphony: f32,
}

impl Part {
    /// "Melody (ch 1): 212 notes, around E5".
    pub fn describe(&self) -> String {
        let poly = if self.polyphony < MONOPHONIC {
            String::new()
        } else {
            format!(", chords {:.0}%", self.polyphony * 100.0)
        };
        format!(
            "{} (ch {}): {} notes, around {}{poly}",
            self.name,
            self.channel,
            self.notes.len(),
            crate::dsp::pitch_detect::note_name(self.average_key)
        )
    }
}

/// A part counts as "one note at a time" below this polyphony.
const MONOPHONIC: f32 = 0.2;
/// Parts with fewer notes are not considered as the melody by default.
const MIN_NOTES: usize = 8;

#[derive(Clone, Debug, PartialEq)]
pub struct MidiSong {
    pub parts: Vec<Part>,
    /// Tempo at the start (later tempo changes are ignored).
    pub bpm: f32,
    pub ticks_per_beat: f32,
    /// The part most likely to be the melody.
    pub default_part: usize,
}

impl MidiSong {
    /// The melody of `part` (skyline reduced), in beats.
    pub fn melody(&self, part: usize, name: &str) -> Option<Melody> {
        let p = self.parts.get(part)?;
        let tpb = self.ticks_per_beat;
        let notes = skyline(&p.notes, (tpb / 32.0) as u64)
            .into_iter()
            .map(|n| Note {
                start: n.start as f32 / tpb,
                len: (n.end - n.start) as f32 / tpb,
                key: n.key,
            })
            .collect();
        Some(Melody {
            name: name.to_string(),
            notes,
            bpm: self.bpm,
        })
    }
}

/// Reads a MIDI file from memory.
pub fn parse(bytes: &[u8]) -> Result<MidiSong, String> {
    let smf = Smf::parse(bytes).map_err(|e| format!("not a readable MIDI file ({e})"))?;
    // Ticks per beat; timecode files are read as if at 120 BPM.
    let (ticks_per_beat, timecode) = match smf.header.timing {
        Timing::Metrical(t) => (f32::from(t.as_int().max(1)), false),
        Timing::Timecode(fps, sub) => (fps.as_f32() * f32::from(sub.max(1)) / 2.0, true),
    };
    let mut bpm = None;
    let mut parts = Vec::new();
    for (track_index, track) in smf.tracks.iter().enumerate() {
        let mut tick = 0u64;
        let mut name = None;
        // Per channel: notes being held (key -> start ticks, oldest first) and finished ones.
        let mut held: Vec<Vec<(u8, u64)>> = vec![Vec::new(); 16];
        let mut done: Vec<Vec<RawNote>> = vec![Vec::new(); 16];
        for ev in track {
            tick += u64::from(ev.delta.as_int());
            match ev.kind {
                TrackEventKind::Meta(MetaMessage::Tempo(us)) if bpm.is_none() && !timecode => {
                    bpm = Some(60_000_000.0 / us.as_int().max(1) as f32);
                }
                TrackEventKind::Meta(MetaMessage::TrackName(n)) if name.is_none() => {
                    let n = String::from_utf8_lossy(n).trim().to_string();
                    if !n.is_empty() {
                        name = Some(n);
                    }
                }
                TrackEventKind::Midi { channel, message } => {
                    let ch = usize::from(channel.as_int());
                    let (key, on) = match message {
                        MidiMessage::NoteOn { key, vel } => (key.as_int(), vel.as_int() > 0),
                        MidiMessage::NoteOff { key, .. } => (key.as_int(), false),
                        _ => continue,
                    };
                    if on {
                        held[ch].push((key, tick));
                    } else if let Some(i) = held[ch].iter().position(|&(k, _)| k == key) {
                        let (_, start) = held[ch].remove(i);
                        if tick > start {
                            done[ch].push(RawNote {
                                start,
                                end: tick,
                                key,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        for ch in 0..16 {
            // Notes never released end with the track.
            for &(key, start) in &held[ch] {
                if tick > start {
                    done[ch].push(RawNote {
                        start,
                        end: tick,
                        key,
                    });
                }
            }
            if ch == 9 || done[ch].is_empty() {
                continue; // drums, or nothing played
            }
            let mut notes = std::mem::take(&mut done[ch]);
            notes.sort_by_key(|n| (n.start, n.key));
            let average_key =
                notes.iter().map(|n| f32::from(n.key)).sum::<f32>() / notes.len() as f32;
            parts.push(Part {
                track: track_index,
                channel: ch as u8 + 1,
                name: name
                    .clone()
                    .unwrap_or_else(|| format!("Track {}", track_index + 1)),
                polyphony: polyphony(&notes),
                average_key,
                notes,
            });
        }
    }
    if parts.is_empty() {
        return Err("the MIDI file has no notes (or only drums)".into());
    }
    let default_part = pick_melody(&parts);
    Ok(MidiSong {
        parts,
        bpm: bpm.unwrap_or(120.0).clamp(20.0, 300.0),
        ticks_per_beat,
        default_part,
    })
}

/// Share of notes starting while an earlier note still sounds (ignoring tiny overlaps from
/// legato playing).
fn polyphony(sorted: &[RawNote]) -> f32 {
    let mut overlapping = 0;
    let mut sounding_until = 0u64;
    for n in sorted {
        let len = n.end - n.start;
        if n.start + len / 4 < sounding_until {
            overlapping += 1;
        }
        sounding_until = sounding_until.max(n.end);
    }
    overlapping as f32 / sorted.len().max(1) as f32
}

/// Highest average pitch among the mostly-monophonic parts (falling back to any part).
fn pick_melody(parts: &[Part]) -> usize {
    let best = |mono_only: bool| {
        parts
            .iter()
            .enumerate()
            .filter(|(_, p)| p.notes.len() >= MIN_NOTES)
            .filter(|(_, p)| !mono_only || p.polyphony < MONOPHONIC)
            .max_by(|a, b| a.1.average_key.total_cmp(&b.1.average_key))
            .map(|(i, _)| i)
    };
    best(true).or_else(|| best(false)).unwrap_or(0)
}

/// One note at a time: the top note at each onset (see the module docs). `tolerance`: notes
/// starting this close together count as one chord.
pub fn skyline(notes: &[RawNote], tolerance: u64) -> Vec<RawNote> {
    let mut sorted = notes.to_vec();
    sorted.sort_by_key(|n| n.start);
    let mut out: Vec<RawNote> = Vec::new();
    let mut i = 0;
    while i < sorted.len() {
        let group_start = sorted[i].start;
        let mut top = sorted[i];
        let mut j = i + 1;
        while j < sorted.len() && sorted[j].start <= group_start + tolerance {
            if sorted[j].key > top.key {
                top = sorted[j];
            }
            j += 1;
        }
        i = j;
        if let Some(last) = out.last_mut()
            && last.end > top.start
        {
            if top.key < last.key {
                continue; // accompaniment under a held melody note
            }
            last.end = top.start;
        }
        out.push(top);
    }
    out.retain(|n| n.end > n.start);
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use midly::num::{u4, u7, u15, u24, u28};
    use midly::{Format, Header, TrackEvent};

    /// (channel 0-15, key, start tick, end tick)
    pub type TestNote = (u8, u8, u32, u32);

    /// Builds a MIDI file: each track is (name, notes). 480 ticks per beat, 90 BPM.
    pub fn build(format: Format, tracks: &[(&str, &[TestNote])]) -> Vec<u8> {
        let mut smf_tracks = Vec::new();
        for (i, (name, notes)) in tracks.iter().enumerate() {
            let mut events: Vec<(u32, TrackEventKind)> = Vec::new();
            if i == 0 {
                events.push((
                    0,
                    TrackEventKind::Meta(MetaMessage::Tempo(u24::new(666_667))),
                ));
            }
            events.push((
                0,
                TrackEventKind::Meta(MetaMessage::TrackName(name.as_bytes())),
            ));
            for &(ch, key, start, end) in notes.iter() {
                let midi = |on: bool| TrackEventKind::Midi {
                    channel: u4::new(ch),
                    message: if on {
                        MidiMessage::NoteOn {
                            key: u7::new(key),
                            vel: u7::new(100),
                        }
                    } else {
                        MidiMessage::NoteOff {
                            key: u7::new(key),
                            vel: u7::new(0),
                        }
                    },
                };
                events.push((start, midi(true)));
                events.push((end, midi(false)));
            }
            // Offs before ons at the same tick, like real files.
            events.sort_by_key(|(t, k)| {
                let on = matches!(
                    k,
                    TrackEventKind::Midi {
                        message: MidiMessage::NoteOn { .. },
                        ..
                    }
                );
                (*t, on)
            });
            let mut last = 0;
            let mut track: Vec<TrackEvent> = events
                .into_iter()
                .map(|(t, kind)| {
                    let delta = u28::new(t - last);
                    last = t;
                    TrackEvent { delta, kind }
                })
                .collect();
            track.push(TrackEvent {
                delta: u28::new(0),
                kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
            });
            smf_tracks.push(track);
        }
        let smf = Smf {
            header: Header::new(format, Timing::Metrical(u15::new(480))),
            tracks: smf_tracks,
        };
        let mut bytes = Vec::new();
        smf.write_std(&mut bytes).unwrap();
        bytes
    }

    const B: u32 = 480; // one beat

    #[test]
    fn parses_tempo_parts_and_notes_in_beats() {
        let melody: &[TestNote] = &(0..10)
            .map(|i| (0, 72 + (i % 3) as u8, i * B, i * B + B))
            .collect::<Vec<_>>();
        let bass: &[TestNote] = &(0..10)
            .map(|i| (1, 40, i * B, i * B + B / 2))
            .collect::<Vec<_>>();
        let bytes = build(Format::Parallel, &[("Lead", melody), ("Bass", bass)]);
        let song = parse(&bytes).unwrap();
        assert!((song.bpm - 90.0).abs() < 0.01);
        assert_eq!(song.parts.len(), 2);
        assert_eq!(song.parts[0].name, "Lead");
        assert_eq!(song.parts[1].channel, 2);
        assert_eq!(song.default_part, 0, "the higher part is the melody");
        let m = song.melody(0, "Lead").unwrap();
        assert_eq!(m.notes.len(), 10);
        assert_eq!(
            m.notes[1],
            Note {
                start: 1.0,
                len: 1.0,
                key: 73
            }
        );
        assert!(song.parts[0].describe().contains("10 notes, around C#5"));
    }

    #[test]
    fn the_melody_is_the_highest_part_that_plays_one_note_at_a_time() {
        // Strings: high but chords (three notes at once). Flute: a bit lower, one at a time.
        // Bass: low, one at a time. Drums on channel 10: ignored. The flute wins.
        let strings: Vec<TestNote> = (0..12)
            .flat_map(|i| {
                [
                    (0, 79, i * B, i * B + B),
                    (0, 83, i * B, i * B + B),
                    (0, 86, i * B, i * B + B),
                ]
            })
            .collect();
        let flute: Vec<TestNote> = (0..12)
            .map(|i| (1, 74 + (i % 4) as u8, i * B, i * B + B))
            .collect();
        let bass: Vec<TestNote> = (0..12).map(|i| (2, 36, i * B, i * B + B)).collect();
        let drums: Vec<TestNote> = (0..12).map(|i| (9, 90, i * B, i * B + 10)).collect();
        // Format 0: everything in one track, told apart by channel.
        let all: Vec<TestNote> = [strings, flute, bass, drums].concat();
        let song = parse(&build(Format::SingleTrack, &[("Song", &all)])).unwrap();
        assert_eq!(song.parts.len(), 3, "drums are skipped");
        let picked = &song.parts[song.default_part];
        assert_eq!(picked.channel, 2, "the flute: {}", picked.describe());
        assert!(song.parts.iter().any(|p| p.polyphony > 0.6));
        // If every part plays chords, the highest one is picked anyway.
        let chords: Vec<TestNote> = (0..12)
            .flat_map(|i| [(0, 60, i * B, i * B + B), (0, 64, i * B, i * B + B)])
            .collect();
        let song = parse(&build(Format::Parallel, &[("Pad", &chords)])).unwrap();
        assert_eq!(song.default_part, 0);
    }

    #[test]
    fn skyline_keeps_the_top_line() {
        let n = |key, start, end| RawNote { start, end, key };
        let notes = [
            // A chord: the top note (67) wins.
            n(60, 0, 480),
            n(64, 0, 480),
            n(67, 2, 480), // starts 2 ticks late: still the same chord
            // A held melody note with accompaniment starting under it: ignored.
            n(72, 480, 1440),
            n(55, 960, 1440),
            // A higher note entering cuts the held one short.
            n(69, 1440, 2400),
            n(76, 1920, 2400),
            // Repeated notes stay separate.
            n(74, 2400, 2640),
            n(74, 2640, 2880),
        ];
        let line = skyline(&notes, 15);
        let keys: Vec<u8> = line.iter().map(|n| n.key).collect();
        assert_eq!(keys, [67, 72, 69, 76, 74, 74]);
        assert_eq!(line[2].end, 1920, "cut where the higher note starts");
        for w in line.windows(2) {
            assert!(w[1].start >= w[0].end, "one note at a time");
        }
    }

    #[test]
    fn polyphonic_parts_become_one_line() {
        let chords: Vec<TestNote> = (0..8)
            .flat_map(|i| {
                let top = 72 + (i % 5) as u8;
                [(0, 60, i * B, i * B + B), (0, top, i * B, i * B + B)]
            })
            .collect();
        let song = parse(&build(Format::Parallel, &[("Piano", &chords)])).unwrap();
        let m = song.melody(0, "Piano").unwrap();
        let keys: Vec<u8> = m.notes.iter().map(|n| n.key).collect();
        assert_eq!(keys, [72, 73, 74, 75, 76, 72, 73, 74]);
    }

    #[test]
    fn running_status_note_on_velocity_zero_and_unclosed_notes() {
        // Hand-written bytes: note on C4, then "note on" with velocity 0 using running status
        // (the status byte is not repeated), then E4 that is never released.
        let track = [
            0x00, 0x90, 60, 100, // note on C4
            0x83, 0x60, 60, 0, // delta 480, running status: C4 velocity 0 = off
            0x00, 64, 100, // E4 on
            0x83, 0x60, 0xFF, 0x2F, 0x00, // delta 480, end of track
        ];
        let mut bytes = b"MThd\0\0\0\x06\0\0\0\x01\x01\xE0MTrk".to_vec();
        bytes.extend_from_slice(&(track.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&track);
        let song = parse(&bytes).unwrap();
        let notes = &song.parts[0].notes;
        assert_eq!(notes.len(), 2);
        assert_eq!((notes[0].key, notes[0].start, notes[0].end), (60, 0, 480));
        assert_eq!(
            (notes[1].key, notes[1].end),
            (64, 960),
            "closed at the track end"
        );
        assert_eq!(song.bpm, 120.0, "no tempo event: the MIDI default");
    }

    #[test]
    fn broken_or_empty_files_are_refused_politely() {
        assert!(
            parse(b"not midi at all")
                .unwrap_err()
                .contains("not a readable MIDI")
        );
        let empty = build(Format::Parallel, &[("Nothing", &[])]);
        assert!(parse(&empty).unwrap_err().contains("no notes"));
        let drums: Vec<TestNote> = vec![(9, 36, 0, 100)];
        assert!(parse(&build(Format::Parallel, &[("Kit", &drums)])).is_err());
    }
}
