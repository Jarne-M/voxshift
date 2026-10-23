//! The melody hand-off between the window and the audio thread: a seqlock over fixed arrays
//! of atomics.
//!
//! The window writes a whole melody (rarely: when you pick one); the audio thread checks a
//! version number every block and, only when it changed, copies the notes into a buffer it
//! allocated up front. No locks and no allocation on the audio thread, and because the store
//! lives in the shared state, the melody survives the engine being rebuilt (a new output
//! device).
//!
//! Seqlock: the writer makes the sequence number odd, writes, then makes it even again. A
//! reader that saw the same even number before and after its copy got a consistent melody;
//! otherwise it simply tries again on the next block.

use super::Note;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering, fence};

/// Longest melody the store holds (longer ones are cut).
pub const MAX_NOTES: usize = 4096;

pub struct MelodyStore {
    seq: AtomicU32,
    len: AtomicU32,
    start: Box<[AtomicU32]>,
    dur: Box<[AtomicU32]>,
    key: Box<[AtomicU32]>,
    /// Serialises writers (the window and tests); the audio thread never takes it.
    writer: Mutex<()>,
}

impl Default for MelodyStore {
    fn default() -> Self {
        let atoms = || (0..MAX_NOTES).map(|_| AtomicU32::new(0)).collect();
        Self {
            seq: AtomicU32::new(0),
            len: AtomicU32::new(0),
            start: atoms(),
            dur: atoms(),
            key: atoms(),
            writer: Mutex::new(()),
        }
    }
}

impl MelodyStore {
    /// Replaces the melody (window side). Returns how many notes were kept.
    pub fn publish(&self, notes: &[Note]) -> usize {
        let _guard = self.writer.lock().unwrap_or_else(|e| e.into_inner());
        let n = notes.len().min(MAX_NOTES);
        self.seq.fetch_add(1, Ordering::Relaxed); // odd: writing
        fence(Ordering::Release);
        for (i, note) in notes[..n].iter().enumerate() {
            self.start[i].store(note.start.to_bits(), Ordering::Relaxed);
            self.dur[i].store(note.len.to_bits(), Ordering::Relaxed);
            self.key[i].store(u32::from(note.key), Ordering::Relaxed);
        }
        self.len.store(n as u32, Ordering::Relaxed);
        self.seq.fetch_add(1, Ordering::Release); // even: done
        n
    }

    /// Changes whenever a new melody is published (cheap to poll every block).
    pub fn version(&self) -> u32 {
        self.seq.load(Ordering::Acquire)
    }

    /// Copies the melody into `out` (audio side). `out` must have room for [`MAX_NOTES`] so
    /// this never allocates. Returns the version copied, or `None` if a write was under way
    /// (try again later).
    pub fn read_into(&self, out: &mut Vec<Note>) -> Option<u32> {
        let before = self.seq.load(Ordering::Acquire);
        if before % 2 == 1 {
            return None;
        }
        out.clear();
        let n = (self.len.load(Ordering::Relaxed) as usize).min(MAX_NOTES);
        for i in 0..n.min(out.capacity()) {
            out.push(Note {
                start: f32::from_bits(self.start[i].load(Ordering::Relaxed)),
                len: f32::from_bits(self.dur[i].load(Ordering::Relaxed)),
                key: self.key[i].load(Ordering::Relaxed).min(127) as u8,
            });
        }
        fence(Ordering::Acquire);
        let after = self.seq.load(Ordering::Relaxed);
        (before == after).then_some(before)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn melody(len: usize, key: u8) -> Vec<Note> {
        (0..len)
            .map(|i| Note {
                start: i as f32,
                len: 1.0,
                key,
            })
            .collect()
    }

    #[test]
    fn publish_and_read_back() {
        let store = MelodyStore::default();
        let mut out = Vec::with_capacity(MAX_NOTES);
        let v0 = store.version();
        assert_eq!(store.read_into(&mut out), Some(v0));
        assert!(out.is_empty());
        let notes = melody(10, 64);
        assert_eq!(store.publish(&notes), 10);
        assert_ne!(store.version(), v0);
        assert!(store.read_into(&mut out).is_some());
        assert_eq!(out, notes);
    }

    #[test]
    fn long_melodies_are_cut_and_reading_never_allocates() {
        let store = MelodyStore::default();
        assert_eq!(store.publish(&melody(MAX_NOTES + 50, 60)), MAX_NOTES);
        let mut out = Vec::with_capacity(MAX_NOTES);
        let cap = out.capacity();
        store.read_into(&mut out).unwrap();
        assert_eq!(out.len(), MAX_NOTES);
        assert_eq!(out.capacity(), cap, "no reallocation");
    }

    #[test]
    fn a_reader_never_sees_a_half_written_melody() {
        // A writer flips between two melodies (all notes 60, or all notes 72) while a reader
        // copies as fast as it can: every successful copy must be all one or all the other.
        let store = Arc::new(MelodyStore::default());
        let w = store.clone();
        let writer = std::thread::spawn(move || {
            for i in 0..2000 {
                let key = if i % 2 == 0 { 60 } else { 72 };
                w.publish(&melody(1000 + i % 7, key));
            }
        });
        let mut out = Vec::with_capacity(MAX_NOTES);
        let (mut ok, mut torn) = (0, 0);
        while !writer.is_finished() {
            match store.read_into(&mut out) {
                Some(_) if out.is_empty() => {} // before the first publish
                Some(_) => {
                    ok += 1;
                    let first = out[0].key;
                    assert!(out.iter().all(|n| n.key == first), "mixed melody");
                    assert!((1000..1007).contains(&out.len()));
                }
                None => torn += 1,
            }
        }
        writer.join().unwrap();
        println!("seqlock: {ok} consistent copies, {torn} retried");
        assert!(ok > 0);
    }
}
