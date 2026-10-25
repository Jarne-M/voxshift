//! How a clip reacts to its hotkey (or a mouse press on its button): the play modes.
//!
//! - Normal: press = start, it plays to the end.
//! - Hold: plays only while the key (or mouse button) is held; release = stop (with the
//!   mixer's ~12 ms fade-out).
//! - Toggle: press = start, press again = stop.
//! - Loop while held: like Hold, but starts over at the end until released.
//!
//! [`Triggers`] is the bookkeeping for the held ones, kept apart from any audio or UI so it is
//! tested on its own. A held clip must never get stuck: besides the normal key-up there is
//! [`Triggers::release_all`] (the key listener saw every key go up) and
//! [`Triggers::held_keys`], which the listener polls against the real keyboard state, so a
//! key-up lost to Alt+Tab still stops the clip.

use super::mixer::SoundId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayMode {
    #[default]
    Normal,
    Hold,
    Toggle,
    LoopWhileHeld,
}

impl PlayMode {
    pub const ALL: [PlayMode; 4] = [
        PlayMode::Normal,
        PlayMode::Hold,
        PlayMode::Toggle,
        PlayMode::LoopWhileHeld,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PlayMode::Normal => "Normal (plays to the end)",
            PlayMode::Hold => "Hold (plays while held)",
            PlayMode::Toggle => "Toggle (press again to stop)",
            PlayMode::LoopWhileHeld => "Loop while held",
        }
    }

    /// The short badge on the clip (empty for Normal).
    pub fn badge(self) -> &'static str {
        match self {
            PlayMode::Normal => "",
            PlayMode::Hold => "HOLD",
            PlayMode::Toggle => "TOGGLE",
            PlayMode::LoopWhileHeld => "LOOP",
        }
    }

    pub fn is_held(self) -> bool {
        matches!(self, PlayMode::Hold | PlayMode::LoopWhileHeld)
    }
}

/// The "key" a mouse press on a clip button holds.
pub const MOUSE: u32 = u32::MAX;

/// What to do with a clip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd {
    Start { id: SoundId, looped: bool },
    Stop(SoundId),
}

/// The clips playing because a key (or the mouse) is held down.
#[derive(Clone, Debug, Default)]
pub struct Triggers {
    held: Vec<(SoundId, u32)>,
}

impl Triggers {
    /// A press of key `vk` (or [`MOUSE`]) for clip `id`. `playing`: whether it is sounding
    /// now (for Toggle). Auto-repeat of a held key is ignored: it never restarts the clip.
    pub fn press(&mut self, id: SoundId, mode: PlayMode, vk: u32, playing: bool) -> Option<Cmd> {
        match mode {
            PlayMode::Normal => Some(Cmd::Start { id, looped: false }),
            PlayMode::Toggle if playing => Some(Cmd::Stop(id)),
            PlayMode::Toggle => Some(Cmd::Start { id, looped: false }),
            PlayMode::Hold | PlayMode::LoopWhileHeld => {
                if self.held.iter().any(|&(h, _)| h == id) {
                    return None; // already held (key repeat, or held by another input)
                }
                self.held.push((id, vk));
                Some(Cmd::Start {
                    id,
                    looped: mode == PlayMode::LoopWhileHeld,
                })
            }
        }
    }

    /// Key `vk` (or [`MOUSE`]) went up: the clips it held, to stop.
    pub fn release(&mut self, vk: u32) -> Vec<SoundId> {
        let stop = self
            .held
            .iter()
            .filter(|&&(_, k)| k == vk)
            .map(|&(id, _)| id)
            .collect();
        self.held.retain(|&(_, k)| k != vk);
        stop
    }

    /// Everything held goes (all keys reported up, Stop all, a panic): the clips to stop.
    pub fn release_all(&mut self) -> Vec<SoundId> {
        std::mem::take(&mut self.held)
            .into_iter()
            .map(|(id, _)| id)
            .collect()
    }

    /// A clip stopped some other way (it was removed, or its mode changed).
    pub fn forget(&mut self, id: SoundId) {
        self.held.retain(|&(h, _)| h != id);
    }

    /// Keyboard keys currently holding a clip (to check against the real keyboard).
    pub fn held_keys(&self) -> Vec<u32> {
        let mut keys: Vec<u32> = self
            .held
            .iter()
            .map(|&(_, k)| k)
            .filter(|&k| k != MOUSE)
            .collect();
        keys.dedup();
        keys
    }

    pub fn is_held(&self, id: SoundId) -> bool {
        self.held.iter().any(|&(h, _)| h == id)
    }
}

/// Of the keys holding clips, the ones the keyboard says are no longer down (their key-up
/// was missed).
pub fn missed_releases(held: &[u32], is_down: impl Fn(u32) -> bool) -> Vec<u32> {
    held.iter().copied().filter(|&k| !is_down(k)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const F1: u32 = 0x70;
    const F2: u32 = 0x71;

    #[test]
    fn normal_plays_to_the_end_and_ignores_the_release() {
        let mut t = Triggers::default();
        assert_eq!(
            t.press(1, PlayMode::Normal, F1, false),
            Some(Cmd::Start {
                id: 1,
                looped: false
            })
        );
        assert_eq!(
            t.press(1, PlayMode::Normal, F1, true),
            Some(Cmd::Start {
                id: 1,
                looped: false
            }),
            "pressing again restarts it, as before"
        );
        assert!(t.release(F1).is_empty());
        assert!(!t.is_held(1));
    }

    #[test]
    fn hold_starts_on_down_stops_on_up_and_ignores_key_repeat() {
        let mut t = Triggers::default();
        assert_eq!(
            t.press(1, PlayMode::Hold, F1, false),
            Some(Cmd::Start {
                id: 1,
                looped: false
            })
        );
        for _ in 0..5 {
            assert_eq!(
                t.press(1, PlayMode::Hold, F1, true),
                None,
                "repeat never restarts"
            );
        }
        assert!(t.release(F2).is_empty(), "another key's release");
        assert_eq!(t.held_keys(), [F1]);
        assert_eq!(t.release(F1), [1]);
        assert!(t.held_keys().is_empty());
        assert!(t.release(F1).is_empty(), "a second key-up does nothing");
        // Two clips on one key both stop with it.
        t.press(1, PlayMode::Hold, F1, false);
        t.press(2, PlayMode::LoopWhileHeld, F1, false);
        assert_eq!(t.release(F1), [1, 2]);
    }

    #[test]
    fn a_missed_key_up_still_stops_the_clip() {
        let mut t = Triggers::default();
        t.press(1, PlayMode::Hold, F1, false);
        t.press(2, PlayMode::LoopWhileHeld, F2, false);
        // Alt+Tab swallowed F1's key-up: the keyboard says F1 is up, F2 still down.
        let missed = missed_releases(&t.held_keys(), |k| k == F2);
        assert_eq!(missed, [F1]);
        let stopped: Vec<SoundId> = missed.into_iter().flat_map(|k| t.release(k)).collect();
        assert_eq!(stopped, [1]);
        // The listener saw every key go up: nothing may keep playing.
        assert_eq!(t.release_all(), [2]);
        assert!(t.held_keys().is_empty());
        // After a missed key-up the next press works normally again.
        assert!(t.press(1, PlayMode::Hold, F1, false).is_some());
    }

    #[test]
    fn toggle_starts_and_stops_by_what_is_playing() {
        let mut t = Triggers::default();
        assert_eq!(
            t.press(3, PlayMode::Toggle, F1, false),
            Some(Cmd::Start {
                id: 3,
                looped: false
            })
        );
        assert_eq!(t.press(3, PlayMode::Toggle, F1, true), Some(Cmd::Stop(3)));
        // It ended by itself in the meantime: the next press starts it again.
        assert_eq!(
            t.press(3, PlayMode::Toggle, F1, false),
            Some(Cmd::Start {
                id: 3,
                looped: false
            })
        );
        assert!(
            t.release(F1).is_empty(),
            "release does nothing in toggle mode"
        );
    }

    #[test]
    fn loop_while_held_loops_and_the_mouse_holds_like_a_key() {
        let mut t = Triggers::default();
        assert_eq!(
            t.press(4, PlayMode::LoopWhileHeld, MOUSE, false),
            Some(Cmd::Start {
                id: 4,
                looped: true
            })
        );
        assert!(t.held_keys().is_empty(), "the mouse is not polled as a key");
        assert_eq!(
            t.press(4, PlayMode::LoopWhileHeld, F1, true),
            None,
            "already held"
        );
        assert_eq!(t.release(MOUSE), [4]);
        t.press(4, PlayMode::Hold, F1, false);
        t.forget(4);
        assert!(!t.is_held(4) && t.release(F1).is_empty());
        assert!(PlayMode::Hold.is_held() && !PlayMode::Toggle.is_held());
        assert_eq!(PlayMode::LoopWhileHeld.badge(), "LOOP");
    }
}
