//! The list of sounds, shared by the GUI, the loader thread and the hotkey thread (all
//! non-real-time, so a mutex is fine). Playing a sound hands its preloaded buffer to the engine.

use super::decode;
use super::hotkeys::Hotkey;
use super::mixer::{SoundCmd, SoundData, SoundId};
use crate::audio::device_manager::EnginePort;
use crate::audio::engine::EngineCmd;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};

/// What is saved in the config.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundEntry {
    pub name: String,
    pub path: PathBuf,
    pub volume: f32,
    pub hotkey: Option<Hotkey>,
}

impl Default for SoundEntry {
    fn default() -> Self {
        Self {
            name: String::new(),
            path: PathBuf::new(),
            volume: 0.8,
            hotkey: None,
        }
    }
}

impl SoundEntry {
    pub fn from_path(path: PathBuf) -> Self {
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "sound".into());
        Self {
            name,
            path,
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug)]
pub enum LoadState {
    Loading,
    Ready(Arc<SoundData>),
    Failed(String),
}

pub struct Sound {
    pub id: SoundId,
    pub entry: SoundEntry,
    pub state: LoadState,
}

struct Job {
    id: SoundId,
    path: PathBuf,
    rate: u32,
}

pub struct Board {
    pub sounds: Vec<Sound>,
    next_id: SoundId,
    out_rate: u32,
    port: Arc<EnginePort>,
    jobs: mpsc::Sender<Job>,
}

impl Board {
    pub fn entries(&self) -> Vec<SoundEntry> {
        self.sounds.iter().map(|s| s.entry.clone()).collect()
    }

    pub fn add(&mut self, entry: SoundEntry) -> SoundId {
        self.next_id += 1;
        let id = self.next_id;
        let _ = self.jobs.send(Job {
            id,
            path: entry.path.clone(),
            rate: self.out_rate,
        });
        self.sounds.push(Sound {
            id,
            entry,
            state: LoadState::Loading,
        });
        id
    }

    pub fn remove(&mut self, id: SoundId) {
        self.port.send(EngineCmd::Sound(SoundCmd::Stop(id)));
        self.sounds.retain(|s| s.id != id);
    }

    pub fn get_mut(&mut self, id: SoundId) -> Option<&mut Sound> {
        self.sounds.iter_mut().find(|s| s.id == id)
    }

    /// Starts (or restarts) a sound. False if it is not loaded or no engine is running.
    pub fn play(&self, id: SoundId) -> bool {
        let Some(s) = self.sounds.iter().find(|s| s.id == id) else {
            return false;
        };
        let LoadState::Ready(data) = &s.state else {
            return false;
        };
        self.port.send(EngineCmd::Sound(SoundCmd::Play {
            id,
            data: data.clone(),
            volume: s.entry.volume,
        }))
    }

    pub fn stop(&self, id: SoundId) {
        self.port.send(EngineCmd::Sound(SoundCmd::Stop(id)));
    }

    pub fn stop_all(&self) {
        self.port.send(EngineCmd::Sound(SoundCmd::StopAll));
    }

    pub fn set_volume(&mut self, id: SoundId, volume: f32) {
        if let Some(s) = self.get_mut(id) {
            s.entry.volume = volume;
        }
        self.port
            .send(EngineCmd::Sound(SoundCmd::SetVolume(id, volume)));
    }

    /// Plays every sound bound to this key combination; returns how many.
    pub fn hotkey_pressed(&self, key: &Hotkey) -> usize {
        let ids: Vec<SoundId> = self
            .sounds
            .iter()
            .filter(|s| s.entry.hotkey.as_ref().is_some_and(|h| h.same_keys(key)))
            .map(|s| s.id)
            .collect();
        ids.iter().filter(|&&id| self.play(id)).count()
    }

    /// The sound that already uses this key combination, if any.
    #[cfg(test)]
    pub fn hotkey_owner(&self, key: &Hotkey) -> Option<SoundId> {
        self.sounds
            .iter()
            .find(|s| s.entry.hotkey.as_ref().is_some_and(|h| h.same_keys(key)))
            .map(|s| s.id)
    }

    /// When the output rate changes, sounds are re-decoded at the new rate so playback never
    /// needs to resample in real time.
    pub fn set_output_rate(&mut self, rate: u32) {
        if rate == 0 || rate == self.out_rate {
            return;
        }
        self.out_rate = rate;
        for s in &self.sounds {
            let stale = match &s.state {
                LoadState::Ready(d) => d.sample_rate != rate,
                LoadState::Loading | LoadState::Failed(_) => false,
            };
            if stale {
                let _ = self.jobs.send(Job {
                    id: s.id,
                    path: s.entry.path.clone(),
                    rate,
                });
            }
        }
    }
}

/// Owns the board and its loader thread.
pub struct Soundboard {
    pub board: Arc<Mutex<Board>>,
}

impl Soundboard {
    pub fn new(port: Arc<EnginePort>, entries: Vec<SoundEntry>) -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        let board = Arc::new(Mutex::new(Board {
            sounds: Vec::new(),
            next_id: 0,
            out_rate: 0,
            port,
            jobs: tx,
        }));
        let b = board.clone();
        std::thread::Builder::new()
            .name("sound-loader".into())
            .spawn(move || {
                // Ends when the board (and with it the job sender) is dropped.
                while let Ok(job) = rx.recv() {
                    let result = decode::load(&job.path, job.rate);
                    let mut board = b.lock().unwrap_or_else(|e| e.into_inner());
                    // A later rate change may have queued a newer job; this result is
                    // still better than nothing and gets replaced when that one lands.
                    if let Some(s) = board.get_mut(job.id) {
                        s.state = match result {
                            Ok(data) => LoadState::Ready(Arc::new(data)),
                            Err(e) => LoadState::Failed(e),
                        };
                    }
                }
            })
            .expect("spawn loader");
        {
            let mut b = board.lock().unwrap_or_else(|e| e.into_inner());
            for e in entries {
                b.add(e);
            }
        }
        Self { board }
    }

    pub fn lock(&self) -> std::sync::MutexGuard<'_, Board> {
        self.board.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::soundboard::decode::tests::{temp_dir, write_wav};
    use std::time::{Duration, Instant};

    fn wait_loaded(sb: &Soundboard, n: usize) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            let ready = sb
                .lock()
                .sounds
                .iter()
                .filter(|s| !matches!(s.state, LoadState::Loading))
                .count();
            if ready == n {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("sounds did not load");
    }

    #[test]
    fn loads_in_the_background_and_reports_failures() {
        let dir = temp_dir("board");
        let good = dir.join("good.wav");
        write_wav(&good, 48_000, 1, &[1000; 4800]);
        let sb = Soundboard::new(
            Arc::new(EnginePort::default()),
            vec![
                SoundEntry::from_path(good),
                SoundEntry::from_path(dir.join("missing.wav")),
            ],
        );
        wait_loaded(&sb, 2);
        let b = sb.lock();
        assert!(matches!(b.sounds[0].state, LoadState::Ready(_)));
        assert!(matches!(b.sounds[1].state, LoadState::Failed(_)));
        assert_eq!(b.sounds[0].entry.name, "good");
        // No engine is running: play reports failure instead of panicking.
        assert!(!b.play(b.sounds[0].id));
        drop(b);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_played_sound_reaches_the_engine_output() {
        use crate::audio::block::{BLOCK, BlockSource};
        use crate::audio::engine::Engine;
        use crate::audio::shared::Shared;
        let dir = temp_dir("play");
        let path = dir.join("beep.wav");
        let tone: Vec<i16> = (0..9600)
            .map(|i| ((i as f32 * 0.1).sin() * 16000.0) as i16)
            .collect();
        write_wav(&path, 48_000, 1, &tone);

        let shared = Arc::new(Shared::default());
        let (mut engine, link) = Engine::new(shared.clone(), 48_000);
        let port = Arc::new(EnginePort::default());
        port.set(Some(link.cmds));
        let sb = Soundboard::new(port, vec![SoundEntry::from_path(path)]);
        wait_loaded(&sb, 1);
        let id = sb.lock().sounds[0].id;
        assert!(sb.lock().play(id));

        let mut out = [0.0; BLOCK * 2];
        let mut peak = 0.0f32;
        for _ in 0..20 {
            engine.render(&mut out);
            peak = out.iter().fold(peak, |m, x| m.max(x.abs()));
        }
        assert!(peak > 0.3, "sound audible in the output: {peak}");
        assert!(shared.is_playing(id));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn hotkeys_find_their_sounds() {
        let sb = Soundboard::new(Arc::new(EnginePort::default()), vec![]);
        let mut b = sb.lock();
        let key = Hotkey::new(0x70, false, true, false, false, "Shift+F1");
        let id = b.add(SoundEntry {
            hotkey: Some(key.clone()),
            ..SoundEntry::from_path("x.wav".into())
        });
        assert_eq!(b.hotkey_owner(&key), Some(id));
        let other = Hotkey::new(0x70, false, false, false, false, "F1");
        assert_eq!(b.hotkey_owner(&other), None, "modifiers must match");
        b.remove(id);
        assert_eq!(b.hotkey_owner(&key), None);
    }
}
