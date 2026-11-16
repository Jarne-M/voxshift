//! The list of sounds, shared by the GUI, the loader thread and the hotkey thread (all
//! non-real-time, so a mutex is fine). Playing a sound hands its preloaded buffer to the engine.

use super::hotkeys::Hotkey;
use super::mixer::{SoundCmd, SoundData, SoundId};
use super::playmode::{Cmd, PlayMode, Triggers};
use super::{decode, names, organize};
use crate::audio::device_manager::EnginePort;
use crate::audio::engine::EngineCmd;
use crate::audio::shared::Shared;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};

/// What is saved in the config. Every field has a default, so clips saved by older versions
/// load (they get a `uid` on load and start in "Unsorted").
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundEntry {
    pub name: String,
    pub path: PathBuf,
    pub volume: f32,
    pub hotkey: Option<Hotkey>,
    /// Permanent id (folders, favourites and recents refer to it). 0 = not given yet.
    pub uid: u64,
    /// The folder it lives in (`None` = Unsorted).
    pub folder: Option<u32>,
    pub favorite: bool,
    /// Palette index for the button tint (`None` = no colour).
    pub color: Option<u8>,
    /// How often it was played (button or hotkey).
    pub plays: u32,
    /// Normal, hold, toggle or loop while held (see `soundboard::playmode`).
    pub play_mode: PlayMode,
}

impl Default for SoundEntry {
    fn default() -> Self {
        Self {
            name: String::new(),
            path: PathBuf::new(),
            volume: 0.8,
            hotkey: None,
            uid: 0,
            folder: None,
            favorite: false,
            color: None,
            plays: 0,
            play_mode: PlayMode::Normal,
        }
    }
}

impl SoundEntry {
    /// A clip named exactly after its file.
    #[cfg(test)]
    pub fn from_path(path: PathBuf) -> Self {
        Self {
            name: names::file_stem(&path),
            path,
            ..Default::default()
        }
    }

    /// A clip for a file the user just added: its name is the file's name tidied up (see
    /// [`names::clean_name`]). The name can be changed later; the file is never renamed.
    pub fn imported(path: PathBuf) -> Self {
        Self {
            name: names::imported_name(&path),
            path,
            ..Default::default()
        }
    }

    /// The file's own name ("airhorn.mp3"), shown next to the display name.
    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// A clip taken off the board, with what is needed to put it back where it was (undo).
#[derive(Clone, Debug)]
pub struct Removed {
    pub entry: SoundEntry,
    /// Its place on the board.
    pub index: usize,
    /// Its place in "Recent", if it was there.
    pub recent_index: Option<usize>,
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
    /// Clip uids, most recently played first (see [`organize::push_recent`]).
    pub recent: Vec<u64>,
    /// Bumped whenever something the config saves changes here (play counts, recents), so
    /// the window knows to copy it; plays also happen on the hotkey thread.
    pub revision: u64,
    /// Clips playing because their key or the mouse is held.
    triggers: Triggers,
    /// What is playing (for toggle mode); set by the app.
    pub shared: Option<Arc<Shared>>,
}

impl Board {
    pub fn entries(&self) -> Vec<SoundEntry> {
        self.sounds.iter().map(|s| s.entry.clone()).collect()
    }

    pub fn add(&mut self, mut entry: SoundEntry) -> SoundId {
        if entry.uid == 0 || self.sounds.iter().any(|s| s.entry.uid == entry.uid) {
            entry.uid = self.sounds.iter().map(|s| s.entry.uid).max().unwrap_or(0) + 1;
        }
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

    #[cfg(test)]
    pub fn remove(&mut self, id: SoundId) {
        self.take(id);
    }

    /// Removes a clip and returns it, so it can be put back with [`Board::restore`].
    pub fn take(&mut self, id: SoundId) -> Option<Removed> {
        self.port.send(EngineCmd::Sound(SoundCmd::Stop(id)));
        self.port.send(EngineCmd::Listen(SoundCmd::Stop(id)));
        self.triggers.forget(id);
        let index = self.sounds.iter().position(|s| s.id == id)?;
        let entry = self.sounds.remove(index).entry;
        let recent_index = self.recent.iter().position(|&u| u == entry.uid);
        self.recent.retain(|&u| u != entry.uid);
        self.revision += 1;
        Some(Removed {
            entry,
            index,
            recent_index,
        })
    }

    /// Puts a removed clip back in its place (it is loaded from its file again). Returns
    /// its new id.
    pub fn restore(&mut self, removed: Removed) -> SoundId {
        let uid_wanted = removed.entry.uid;
        let id = self.add(removed.entry);
        if let Some(sound) = self.sounds.pop() {
            let uid = sound.entry.uid;
            self.sounds
                .insert(removed.index.min(self.sounds.len()), sound);
            // Back into Recent only if it kept its identity.
            if let Some(at) = removed.recent_index
                && uid == uid_wanted
            {
                self.recent.insert(at.min(self.recent.len()), uid);
            }
        }
        self.revision += 1;
        id
    }

    pub fn get_mut(&mut self, id: SoundId) -> Option<&mut Sound> {
        self.sounds.iter_mut().find(|s| s.id == id)
    }

    /// Points a clip at another file (it was moved or deleted) and loads it from there. Its
    /// name, key, folder, colour and favourite stay.
    pub fn relocate(&mut self, id: SoundId, path: PathBuf) {
        let rate = self.out_rate;
        if let Some(s) = self.get_mut(id) {
            s.entry.path = path.clone();
            s.state = LoadState::Loading;
            let _ = self.jobs.send(Job { id, path, rate });
            self.revision += 1;
        }
    }

    /// Starts (or restarts) a sound. False if it is not loaded or no engine is running.
    #[cfg(test)]
    pub fn play(&mut self, id: SoundId) -> bool {
        self.start(id, false)
    }

    /// Starts a sound, looping it until stopped if `looped`; counts the play and puts the
    /// clip at the top of "Recent".
    pub fn start(&mut self, id: SoundId, looped: bool) -> bool {
        if self
            .shared
            .as_ref()
            .is_some_and(|s| s.safe_mode.load(std::sync::atomic::Ordering::Relaxed))
        {
            return false; // panic / safe mode: no clips (the engine refuses them too)
        }
        let Some(s) = self.sounds.iter_mut().find(|s| s.id == id) else {
            return false;
        };
        let LoadState::Ready(data) = &s.state else {
            return false;
        };
        let sent = self.port.send(EngineCmd::Sound(SoundCmd::Play {
            id,
            data: data.clone(),
            volume: s.entry.volume,
            looped,
        }));
        if sent {
            s.entry.plays = s.entry.plays.saturating_add(1);
            let uid = s.entry.uid;
            organize::push_recent(&mut self.recent, uid);
            self.revision += 1;
        }
        sent
    }

    /// Plays a clip in the headphones only (it never reaches the virtual mic), from the top;
    /// any other preview stops. Not counted as a play. False if it is not loaded, safe mode
    /// is on or no engine is running.
    pub fn preview(&mut self, id: SoundId) -> bool {
        if self
            .shared
            .as_ref()
            .is_some_and(|s| s.safe_mode.load(std::sync::atomic::Ordering::Relaxed))
        {
            return false;
        }
        let Some(s) = self.sounds.iter().find(|s| s.id == id) else {
            return false;
        };
        let LoadState::Ready(data) = &s.state else {
            return false;
        };
        let play = SoundCmd::Play {
            id,
            data: data.clone(),
            volume: s.entry.volume,
            looped: false,
        };
        self.port.send(EngineCmd::Listen(SoundCmd::StopAll));
        self.port.send(EngineCmd::Listen(play))
    }

    pub fn stop_preview(&self) {
        self.port.send(EngineCmd::Listen(SoundCmd::StopAll));
    }

    /// The clip being previewed, if any.
    pub fn previewing(&self) -> Option<SoundId> {
        let id = self
            .shared
            .as_ref()?
            .listen_id
            .load(std::sync::atomic::Ordering::Relaxed);
        (id != 0).then_some(id)
    }

    /// Empties "Recent".
    pub fn clear_recent(&mut self) {
        self.recent.clear();
        self.revision += 1;
    }

    pub fn stop(&self, id: SoundId) {
        self.port.send(EngineCmd::Sound(SoundCmd::Stop(id)));
    }

    /// Stops every clip, held ones included.
    pub fn stop_all(&mut self) {
        self.triggers.release_all();
        self.port.send(EngineCmd::Sound(SoundCmd::StopAll));
    }

    pub fn is_playing(&self, id: SoundId) -> bool {
        self.shared.as_ref().is_some_and(|s| s.is_playing(id))
    }

    fn apply(&mut self, cmd: Cmd) -> bool {
        match cmd {
            Cmd::Start { id, looped } => self.start(id, looped),
            Cmd::Stop(id) => {
                self.stop(id);
                true
            }
        }
    }

    /// A press on clip `id` by key `vk` (or the mouse, `playmode::MOUSE`), per its play mode.
    pub fn press(&mut self, id: SoundId, vk: u32) -> bool {
        let Some(mode) = self
            .sounds
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.entry.play_mode)
        else {
            return false;
        };
        let playing = self.is_playing(id);
        match self.triggers.press(id, mode, vk, playing) {
            Some(cmd) => {
                let ok = self.apply(cmd);
                if !ok {
                    self.triggers.forget(id); // refused (safe mode, not loaded): holds nothing
                }
                ok
            }
            None => true, // held already: nothing to do
        }
    }

    /// Key `vk` (or the mouse) went up: clips held by it stop.
    pub fn release(&mut self, vk: u32) {
        for id in self.triggers.release(vk) {
            self.stop(id);
        }
    }

    /// Stops every held clip (all keys up, or a missed key-up).
    pub fn release_all(&mut self) {
        for id in self.triggers.release_all() {
            self.stop(id);
        }
    }

    /// Keys currently holding clips (the listener checks them against the keyboard).
    pub fn held_keys(&self) -> Vec<u32> {
        self.triggers.held_keys()
    }

    /// A clip's mode changed: if it was held, it stops (the old mode's rules no longer apply).
    pub fn set_play_mode(&mut self, id: SoundId, mode: PlayMode) {
        if self.triggers.is_held(id) {
            self.triggers.forget(id);
            self.stop(id);
        }
        if let Some(s) = self.get_mut(id) {
            s.entry.play_mode = mode;
        }
    }

    pub fn set_volume(&mut self, id: SoundId, volume: f32) {
        if let Some(s) = self.get_mut(id) {
            s.entry.volume = volume;
        }
        self.port
            .send(EngineCmd::Sound(SoundCmd::SetVolume(id, volume)));
        self.port
            .send(EngineCmd::Listen(SoundCmd::SetVolume(id, volume)));
    }

    /// Triggers every sound bound to this key combination (per its play mode); returns how
    /// many.
    pub fn hotkey_pressed(&mut self, key: &Hotkey) -> usize {
        let ids: Vec<SoundId> = self
            .sounds
            .iter()
            .filter(|s| s.entry.hotkey.as_ref().is_some_and(|h| h.same_keys(key)))
            .map(|s| s.id)
            .collect();
        ids.iter().filter(|&&id| self.press(id, key.vk)).count()
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
            recent: Vec::new(),
            revision: 0,
            triggers: Triggers::default(),
            shared: None,
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
            // Clips saved before ids existed get new ones above every saved id, so a saved
            // clip later in the list never has to give up its own.
            let mut next = entries.iter().map(|e| e.uid).max().unwrap_or(0);
            let mut b = board.lock().unwrap_or_else(|e| e.into_inner());
            for mut e in entries {
                if e.uid == 0 {
                    next += 1;
                    e.uid = next;
                }
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
    use crate::soundboard::playmode;
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
        let mut b = sb.lock();
        assert!(matches!(b.sounds[0].state, LoadState::Ready(_)));
        assert!(matches!(b.sounds[1].state, LoadState::Failed(_)));
        assert_eq!(b.sounds[0].entry.name, "good");
        // No engine is running: play reports failure instead of panicking.
        let id = b.sounds[0].id;
        assert!(!b.play(id));
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
    fn plays_are_counted_recent_follows_and_a_missing_file_can_be_relocated() {
        use crate::audio::engine::Engine;
        use crate::audio::shared::Shared;
        let dir = temp_dir("board-recent");
        let a = dir.join("a.wav");
        let b = dir.join("b.wav");
        write_wav(&a, 48_000, 1, &[1000; 4800]);
        write_wav(&b, 48_000, 1, &[1000; 4800]);
        let (_engine, link) = Engine::new(Arc::new(Shared::default()), 48_000);
        let port = Arc::new(EnginePort::default());
        port.set(Some(link.cmds));
        let gone = dir.join("moved away.wav");
        let sb = Soundboard::new(
            port,
            vec![
                SoundEntry::from_path(a),
                SoundEntry {
                    uid: 40, // from a saved config: kept
                    favorite: true,
                    color: Some(2),
                    folder: Some(3),
                    ..SoundEntry::from_path(gone)
                },
                SoundEntry::from_path(b),
            ],
        );
        wait_loaded(&sb, 3);
        let mut board = sb.lock();
        let uids: Vec<u64> = board.sounds.iter().map(|s| s.entry.uid).collect();
        assert_eq!(
            uids,
            [41, 40, 42],
            "new clips get fresh ids, saved ones keep theirs"
        );
        let ids: Vec<SoundId> = board.sounds.iter().map(|s| s.id).collect();
        let rev = board.revision;
        assert!(board.play(ids[0]));
        assert!(board.play(ids[2]));
        assert!(board.play(ids[0]));
        assert_eq!(board.recent, [41, 42], "most recent first, once each");
        assert_eq!(board.sounds[0].entry.plays, 2);
        assert!(board.revision > rev, "the window is told");
        assert!(!board.play(ids[1]), "a missing file does not play...");
        assert!(matches!(board.sounds[1].state, LoadState::Failed(_)));
        board.remove(ids[2]);
        assert_eq!(board.recent, [41], "a removed clip leaves Recent");
        board.clear_recent();
        assert!(board.recent.is_empty());

        // ...until it is located again: name, folder, colour and star are kept.
        let found = dir.join("found it.wav");
        write_wav(&found, 48_000, 1, &[1000; 4800]);
        board.relocate(ids[1], found.clone());
        drop(board);
        wait_loaded(&sb, 2);
        let board = sb.lock();
        let s = &board.sounds[1];
        assert!(matches!(s.state, LoadState::Ready(_)));
        assert_eq!(s.entry.path, found);
        assert_eq!(s.entry.name, "moved away");
        assert!(s.entry.favorite && s.entry.color == Some(2) && s.entry.folder == Some(3));
        drop(board);
        std::fs::remove_dir_all(dir).ok();
    }

    /// A board wired to a real engine, with one loaded clip (`seconds` long, a steady 0.5) in
    /// `mode` on F1. Returns the board, the engine, the shared state and the clip's id.
    fn rig(
        name: &str,
        seconds: f32,
        mode: PlayMode,
    ) -> (
        Soundboard,
        crate::audio::engine::Engine,
        Arc<Shared>,
        SoundId,
        PathBuf,
    ) {
        use crate::audio::engine::Engine;
        let dir = temp_dir(name);
        let path = dir.join("clip.wav");
        write_wav(
            &path,
            48_000,
            1,
            &vec![16384; (seconds * 48_000.0) as usize],
        );
        let shared = Arc::new(Shared::default());
        let (engine, link) = Engine::new(shared.clone(), 48_000);
        let port = Arc::new(EnginePort::default());
        port.set(Some(link.cmds));
        let f1 = Hotkey::new(0x70, false, false, false, false, "F1");
        let sb = Soundboard::new(
            port,
            vec![SoundEntry {
                hotkey: Some(f1),
                play_mode: mode,
                volume: 1.0,
                ..SoundEntry::from_path(path)
            }],
        );
        wait_loaded(&sb, 1);
        sb.lock().shared = Some(shared.clone());
        let id = sb.lock().sounds[0].id;
        (sb, engine, shared, id, dir)
    }

    /// Renders `ms` of output; returns the left channel.
    fn render(engine: &mut crate::audio::engine::Engine, ms: f32) -> Vec<f32> {
        use crate::audio::block::{BLOCK, BlockSource};
        let mut out = [0.0; BLOCK * 2];
        let mut left = Vec::new();
        for _ in 0..((ms * 48.0) as usize).div_ceil(BLOCK) {
            engine.render(&mut out);
            left.extend(out.as_chunks::<2>().0.iter().map(|f| f[0]));
        }
        left
    }

    fn f1() -> Hotkey {
        Hotkey::new(0x70, false, false, false, false, "F1")
    }

    #[test]
    fn hold_plays_while_the_key_is_down_and_fades_out_on_release() {
        let (sb, mut engine, shared, id, dir) = rig("mode-hold", 3.0, PlayMode::Hold);
        assert_eq!(sb.lock().hotkey_pressed(&f1()), 1);
        let held = render(&mut engine, 200.0);
        assert!(shared.is_playing(id));
        // Key repeat while held: never restarts (a restart would fade in again from 0).
        for _ in 0..10 {
            sb.lock().hotkey_pressed(&f1());
            let more = render(&mut engine, 20.0);
            assert!(more.iter().all(|x| (x - 0.5).abs() < 0.01), "uninterrupted");
        }
        assert!((held.last().unwrap() - 0.5).abs() < 0.01);
        sb.lock().release(0x70); // key-up
        let after = render(&mut engine, 40.0);
        let gone = after.iter().position(|x| x.abs() < 1e-6).unwrap() as f32 / 48.0;
        assert!(gone > 5.0 && gone < 22.0, "fades out in {gone} ms");
        let step = after
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        assert!(step < 0.003, "no click: {step}");
        assert!(!shared.is_playing(id));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_missed_key_up_cannot_leave_a_clip_playing() {
        let (sb, mut engine, shared, id, dir) = rig("mode-missed", 3.0, PlayMode::LoopWhileHeld);
        sb.lock().hotkey_pressed(&f1());
        render(&mut engine, 50.0);
        assert!(shared.is_playing(id));
        assert_eq!(sb.lock().held_keys(), [0x70], "the listener watches F1");
        // The key-up never arrives; the listener notices every key is up (or the keyboard
        // check finds F1 up) and lets go of everything.
        sb.lock().release_all();
        render(&mut engine, 40.0);
        assert!(!shared.is_playing(id));
        assert!(sb.lock().held_keys().is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn loop_while_held_plays_past_its_end_until_released() {
        let (sb, mut engine, shared, id, dir) = rig("mode-loop", 0.1, PlayMode::LoopWhileHeld);
        sb.lock().hotkey_pressed(&f1());
        let out = render(&mut engine, 500.0); // five times the clip's length
        assert!(shared.is_playing(id), "still going");
        assert!(
            out[(0.45 * 48_000.0) as usize].abs() > 0.4,
            "sound at 450 ms"
        );
        sb.lock().release(0x70);
        render(&mut engine, 40.0);
        assert!(!shared.is_playing(id));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn safe_mode_plays_nothing_and_counts_nothing_until_restored() {
        use std::sync::atomic::Ordering::Relaxed;
        let (sb, mut engine, shared, id, dir) = rig("mode-safe", 3.0, PlayMode::Hold);
        // A held clip is playing when panic hits: it stops (the engine sees the flag).
        sb.lock().hotkey_pressed(&f1());
        render(&mut engine, 50.0);
        assert!(shared.is_playing(id));
        shared.safe_mode.store(true, Relaxed);
        render(&mut engine, 40.0);
        assert!(!shared.is_playing(id), "stopped by the panic");
        // Key-up afterwards is harmless; new presses (key or mouse) start nothing.
        sb.lock().release(0x70);
        sb.lock().hotkey_pressed(&f1());
        assert!(
            sb.lock().held_keys().is_empty(),
            "a refused press holds nothing"
        );
        assert!(!sb.lock().press(id, playmode::MOUSE));
        render(&mut engine, 50.0);
        assert!(!shared.is_playing(id));
        let b = sb.lock();
        assert_eq!(b.sounds[0].entry.plays, 1, "refused plays not counted");
        assert_eq!(b.recent, [b.sounds[0].entry.uid]);
        drop(b);
        // Restore: the next press plays again (nothing comes back by itself).
        sb.lock().release_all();
        shared.safe_mode.store(false, Relaxed);
        render(&mut engine, 50.0);
        assert!(!shared.is_playing(id), "restore does not restart clips");
        sb.lock().hotkey_pressed(&f1());
        render(&mut engine, 50.0);
        assert!(shared.is_playing(id));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn toggle_starts_and_stops_and_normal_ignores_the_release() {
        let (sb, mut engine, shared, id, dir) = rig("mode-toggle", 3.0, PlayMode::Toggle);
        sb.lock().hotkey_pressed(&f1());
        render(&mut engine, 50.0);
        assert!(shared.is_playing(id));
        sb.lock().release(0x70);
        render(&mut engine, 50.0);
        assert!(
            shared.is_playing(id),
            "a toggle keeps playing after the key-up"
        );
        sb.lock().hotkey_pressed(&f1());
        render(&mut engine, 40.0);
        assert!(!shared.is_playing(id), "second press stops it");
        // The same clip in Normal mode: plays on, release or not; the mouse works the same.
        sb.lock().set_play_mode(id, PlayMode::Normal);
        assert!(sb.lock().press(id, playmode::MOUSE));
        sb.lock().release(playmode::MOUSE);
        render(&mut engine, 50.0);
        assert!(shared.is_playing(id));
        assert_eq!(
            sb.lock().sounds[0].entry.plays,
            2,
            "two starts (the stop is not a play)"
        );
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
