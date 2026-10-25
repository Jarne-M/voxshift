//! Voice changer hotkeys: toggle (or hold for) effects, mute the mic, jump to a preset, Melody
//! Voice, the monitor ("hear myself") and "stop all sounds"; plus the feedback when the voice
//! changer or the monitor switches (beep in the headphones, overlay).
//!
//! The keys come from the soundboard's global key listener ([`KeyHandler`]), so they work in
//! the background, in the tray and while a game has focus, and the game still gets the key.

use crate::audio::shared::Shared;
use crate::dsp::cue::CueKind;
use crate::soundboard::hotkeys::{Hotkey, KeyHandler};
use crate::soundboard::mixer::SoundId;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};
use std::sync::{Arc, Mutex, mpsc};

/// What the voice changer hotkey does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoiceKeyMode {
    /// Each press flips effects on/off.
    #[default]
    Toggle,
    /// Effects only while the key is held ("push-to-talk effect").
    HoldForEffects,
    /// Clean only while the key is held ("push-to-clean").
    HoldForClean,
}

impl VoiceKeyMode {
    /// The voice changer state while the key is not held (`None` in toggle mode).
    pub fn resting(self) -> Option<bool> {
        match self {
            Self::Toggle => None,
            Self::HoldForEffects => Some(false),
            Self::HoldForClean => Some(true),
        }
    }

    /// New state when the key goes down.
    pub fn on_press(self, current: bool) -> bool {
        match self {
            Self::Toggle => !current,
            Self::HoldForEffects => true,
            Self::HoldForClean => false,
        }
    }

    /// New state when the key goes up (`None` = unchanged).
    pub fn on_release(self) -> Option<bool> {
        self.resting()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PresetHotkey {
    pub preset: String,
    pub hotkey: Option<Hotkey>,
}

/// Saved in the config.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceHotkeys {
    pub toggle: Option<Hotkey>,
    pub mode: VoiceKeyMode,
    pub mute: Option<Hotkey>,
    pub presets: Vec<PresetHotkey>,
    /// Melody Voice on/off, back to the first note, on to the next note.
    pub melody_toggle: Option<Hotkey>,
    pub melody_restart: Option<Hotkey>,
    pub melody_next: Option<Hotkey>,
    /// "Hear myself" (the monitor's "my voice") on/off.
    pub monitor_toggle: Option<Hotkey>,
    /// Soundboard in my headphones ("Also play in my headphones") on/off.
    pub monitor_sounds_toggle: Option<Hotkey>,
    /// Stops every soundboard clip, like the Stop all button.
    pub stop_sounds: Option<Hotkey>,
}

/// Which hotkey is being assigned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyTarget {
    Sound(SoundId),
    Toggle,
    Mute,
    Preset(usize),
    MelodyToggle,
    MelodyRestart,
    MelodyNext,
    MonitorToggle,
    MonitorSoundsToggle,
    StopSounds,
}

impl VoiceHotkeys {
    /// The one-shot action keys (Melody Voice, monitor, stop all) with their targets and how
    /// they are named to the user.
    pub fn action_keys(&self) -> [(KeyTarget, &Option<Hotkey>, &'static str); 6] {
        [
            (
                KeyTarget::MelodyToggle,
                &self.melody_toggle,
                "the Melody Voice toggle",
            ),
            (
                KeyTarget::MelodyRestart,
                &self.melody_restart,
                "restart melody",
            ),
            (KeyTarget::MelodyNext, &self.melody_next, "next note"),
            (
                KeyTarget::MonitorToggle,
                &self.monitor_toggle,
                "the monitor (hear myself) toggle",
            ),
            (
                KeyTarget::MonitorSoundsToggle,
                &self.monitor_sounds_toggle,
                "the soundboard-in-headphones toggle",
            ),
            (KeyTarget::StopSounds, &self.stop_sounds, "stop all sounds"),
        ]
    }
}

/// Who else already uses `key` (anything but `target`), described for the user. See
/// [`keymap`](crate::keymap) for the full rules.
#[cfg(test)]
pub fn conflict<'a>(
    key: &Hotkey,
    target: KeyTarget,
    voice: &'a VoiceHotkeys,
    sounds: impl IntoIterator<Item = (SoundId, &'a str, Option<&'a Hotkey>)>,
) -> Option<String> {
    crate::keymap::owner(key, target, voice, sounds).map(|o| o.name)
}

type OverlayFn = Box<dyn Fn(bool) + Send + Sync>;

/// Switches the voice changer and announces it. Used by the window button, the tray menu and
/// the hotkey, so all three stay in sync (they all set the same shared atomic).
pub struct Feedback {
    shared: Arc<Shared>,
    overlay: OverlayFn,
}

impl Feedback {
    pub fn new(shared: Arc<Shared>, overlay: impl Fn(bool) + Send + Sync + 'static) -> Self {
        Self {
            shared,
            overlay: Box::new(overlay),
        }
    }

    pub fn shared(&self) -> &Shared {
        &self.shared
    }

    /// Sets the state; `announce` plays the beep / shows the overlay if it actually changed
    /// (not used for push-to-talk, which would beep on every press).
    pub fn set_voice(&self, on: bool, announce: bool) {
        let was = self.shared.voice_on.swap(on, Relaxed);
        if announce && was != on {
            if self.shared.cue_enabled.load(Relaxed) {
                self.shared.request_cue(if on {
                    CueKind::VoiceOn
                } else {
                    CueKind::VoiceOff
                });
            }
            if self.shared.overlay_enabled.load(Relaxed) {
                (self.overlay)(on);
            }
        }
    }

    pub fn toggle_voice(&self) {
        let on = !self.shared.voice_on.load(Relaxed);
        self.set_voice(on, true);
    }

    pub fn toggle_mute(&self) {
        self.shared.mic_muted.fetch_xor(true, Relaxed);
    }

    /// Flips "hear myself" on the monitor (only the headphones; the virtual mic and the
    /// effects are untouched) and beeps the new state if beeps are on. Returns the new state.
    pub fn toggle_monitor(&self) -> bool {
        self.flip_and_beep(&self.shared.monitor_voice)
    }

    /// Flips "Also play in my headphones" (the soundboard on the monitor; the virtual mic is
    /// untouched) with the same beeps. Returns the new state.
    pub fn toggle_monitor_sounds(&self) -> bool {
        self.flip_and_beep(&self.shared.monitor_sounds)
    }

    fn flip_and_beep(&self, flag: &AtomicBool) -> bool {
        let on = !flag.fetch_xor(true, Relaxed);
        if self.shared.cue_enabled.load(Relaxed) {
            self.shared.request_cue(if on {
                CueKind::MonitorOn
            } else {
                CueKind::MonitorOff
            });
        }
        on
    }

    /// Stops every soundboard clip (they fade out over a few ms).
    pub fn stop_all_sounds(&self) {
        self.shared.stop_sounds.fetch_add(1, Relaxed);
    }
}

/// Requests that need the window (changing sliders lives there).
#[derive(Clone, Debug, PartialEq)]
pub enum VoiceRequest {
    Preset(String),
    ToggleMelody,
}

type WakeFn = Box<dyn Fn() + Send + Sync>;

/// The voice changer's hotkeys at runtime, called on the hotkey worker thread.
pub struct VoiceKeys {
    bindings: Mutex<VoiceHotkeys>,
    /// Key code held down in a hold mode (0 = none).
    held: AtomicU32,
    feedback: Arc<Feedback>,
    requests: mpsc::Sender<VoiceRequest>,
    /// Wakes the window (repaint) and refreshes the tray after a change.
    wake: WakeFn,
}

impl VoiceKeys {
    pub fn new(
        feedback: Arc<Feedback>,
        requests: mpsc::Sender<VoiceRequest>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        Self {
            bindings: Mutex::new(VoiceHotkeys::default()),
            held: AtomicU32::new(0),
            feedback,
            requests,
            wake: Box::new(wake),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, VoiceHotkeys> {
        self.bindings.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn bindings(&self) -> VoiceHotkeys {
        self.lock().clone()
    }

    /// New bindings from the settings. Switching to a hold mode puts the voice changer in that
    /// mode's resting state right away.
    pub fn set_bindings(&self, b: VoiceHotkeys) {
        let mode_changed = self.lock().mode != b.mode;
        let resting = b.mode.resting();
        *self.lock() = b;
        if mode_changed {
            self.held.store(0, Relaxed);
            if let Some(on) = resting {
                self.feedback.set_voice(on, false);
                (self.wake)();
            }
        }
    }
}

impl KeyHandler for VoiceKeys {
    fn pressed(&self, key: &Hotkey) -> bool {
        let is = |h: &Option<Hotkey>| h.as_ref().is_some_and(|h| h.same_keys(key));
        let (toggle, mute, preset, mode, action) = {
            let b = self.lock();
            let preset = b
                .presets
                .iter()
                .find(|p| is(&p.hotkey))
                .map(|p| p.preset.clone());
            let action = b.action_keys().into_iter().find(|(_, h, _)| is(h));
            (
                is(&b.toggle),
                is(&b.mute),
                preset,
                b.mode,
                action.map(|(t, _, _)| t),
            )
        };
        let shared = self.feedback.shared();
        if let Some(target) = action {
            // These act on the shared state / audio thread directly (also while hidden); only
            // the Melody Voice toggle changes a slider, which lives in the window.
            match target {
                KeyTarget::MelodyRestart => {
                    shared.melody_restart.fetch_add(1, Relaxed);
                }
                KeyTarget::MelodyNext => {
                    shared.melody_next.fetch_add(1, Relaxed);
                }
                KeyTarget::MonitorToggle => {
                    self.feedback.toggle_monitor();
                }
                KeyTarget::MonitorSoundsToggle => {
                    self.feedback.toggle_monitor_sounds();
                }
                KeyTarget::StopSounds => self.feedback.stop_all_sounds(),
                _ => {
                    let _ = self.requests.send(VoiceRequest::ToggleMelody);
                }
            }
        } else if toggle {
            match mode {
                VoiceKeyMode::Toggle => self.feedback.toggle_voice(),
                hold => {
                    self.held.store(key.vk, Relaxed);
                    let current = self.feedback.shared().voice_on.load(Relaxed);
                    self.feedback.set_voice(hold.on_press(current), false);
                }
            }
        } else if mute {
            self.feedback.toggle_mute();
        } else if let Some(name) = preset {
            let _ = self.requests.send(VoiceRequest::Preset(name));
        } else {
            return false;
        }
        (self.wake)();
        true
    }

    fn released(&self, vk: u32) {
        if vk == 0 || self.held.load(Relaxed) != vk {
            return;
        }
        self.held.store(0, Relaxed);
        let mode = self.lock().mode;
        if let Some(on) = mode.on_release() {
            self.feedback.set_voice(on, false);
            (self.wake)();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::soundboard::hotkeys::{KeyEvent, KeyTracker, RawKey};
    use std::sync::atomic::AtomicUsize;

    fn hk(vk: u32, ctrl: bool, label: &str) -> Hotkey {
        Hotkey::new(vk, ctrl, false, false, false, label)
    }

    struct Rig {
        shared: Arc<Shared>,
        keys: VoiceKeys,
        overlays: Arc<AtomicUsize>,
        wakes: Arc<AtomicUsize>,
        requests: mpsc::Receiver<VoiceRequest>,
    }

    fn rig(bindings: VoiceHotkeys) -> Rig {
        let shared = Arc::new(Shared::default());
        let overlays = Arc::new(AtomicUsize::new(0));
        let o = overlays.clone();
        let feedback = Arc::new(Feedback::new(shared.clone(), move |_| {
            o.fetch_add(1, Relaxed);
        }));
        let wakes = Arc::new(AtomicUsize::new(0));
        let w = wakes.clone();
        let (tx, rx) = mpsc::channel();
        let keys = VoiceKeys::new(feedback, tx, move || {
            w.fetch_add(1, Relaxed);
        });
        keys.set_bindings(bindings);
        Rig {
            shared,
            keys,
            overlays,
            wakes,
            requests: rx,
        }
    }

    #[test]
    fn mode_rules() {
        use VoiceKeyMode::*;
        assert_eq!((Toggle.on_press(true), Toggle.on_release()), (false, None));
        assert_eq!(
            (HoldForEffects.on_press(false), HoldForEffects.on_release()),
            (true, Some(false))
        );
        assert_eq!(
            (HoldForClean.on_press(true), HoldForClean.on_release()),
            (false, Some(true))
        );
    }

    #[test]
    fn toggle_flips_and_announces_each_press_without_touching_sliders() {
        let r = rig(VoiceHotkeys {
            toggle: Some(hk(0x77, false, "F8")),
            ..Default::default()
        });
        r.shared.cue_enabled.store(true, Relaxed);
        r.shared.overlay_enabled.store(true, Relaxed);
        let before = r.shared.params.snapshot();
        assert!(r.keys.pressed(&hk(0x77, false, "F8")));
        assert!(!r.shared.voice_on.load(Relaxed));
        assert_eq!(
            r.shared.take_cue(),
            Some(CueKind::VoiceOff),
            "OFF beep requested"
        );
        assert!(r.keys.pressed(&hk(0x77, false, "F8")));
        assert!(r.shared.voice_on.load(Relaxed));
        assert_eq!(
            r.shared.take_cue(),
            Some(CueKind::VoiceOn),
            "ON beep requested"
        );
        assert_eq!(r.overlays.load(Relaxed), 2);
        assert!(r.wakes.load(Relaxed) >= 2);
        assert_eq!(r.shared.params.snapshot(), before, "no slider changed");
        assert!(
            !r.keys.pressed(&hk(0x77, true, "Ctrl+F8")),
            "other combo is not ours"
        );
    }

    #[test]
    fn holding_the_key_toggles_once() {
        // Real key events go through the tracker, which drops auto-repeat.
        let r = rig(VoiceHotkeys {
            toggle: Some(hk(0x77, false, "F8")),
            ..Default::default()
        });
        let mut tracker = KeyTracker::default();
        let raw = |down| RawKey {
            vk: 0x77,
            scan: 0x42,
            extended: false,
            down,
            ctrl: false,
            shift: false,
            alt: false,
            win: false,
        };
        for down in [true, true, true, true, false] {
            match tracker.feed_event(raw(down), |_| "F8".into()) {
                Some(KeyEvent::Pressed(k)) => {
                    r.keys.pressed(&k);
                }
                Some(KeyEvent::Released(vk)) => r.keys.released(vk),
                None => {}
            }
        }
        assert!(!r.shared.voice_on.load(Relaxed), "one press = one toggle");
    }

    #[test]
    fn hold_modes_apply_while_held_and_rest_otherwise() {
        let r = rig(VoiceHotkeys {
            toggle: Some(hk(0x56, false, "V")),
            mode: VoiceKeyMode::HoldForEffects,
            ..Default::default()
        });
        assert!(!r.shared.voice_on.load(Relaxed), "resting = clean");
        r.shared.cue_enabled.store(true, Relaxed);
        r.keys.pressed(&hk(0x56, false, "V"));
        assert!(r.shared.voice_on.load(Relaxed));
        r.keys.released(0x41); // another key: no effect
        assert!(r.shared.voice_on.load(Relaxed));
        r.keys.released(0x56);
        assert!(!r.shared.voice_on.load(Relaxed));
        assert_eq!(r.shared.take_cue(), None, "no beeps for push-to-talk");

        r.keys.set_bindings(VoiceHotkeys {
            toggle: Some(hk(0x56, false, "V")),
            mode: VoiceKeyMode::HoldForClean,
            ..Default::default()
        });
        assert!(
            r.shared.voice_on.load(Relaxed),
            "push-to-clean rests with effects on"
        );
        r.keys.pressed(&hk(0x56, false, "V"));
        assert!(!r.shared.voice_on.load(Relaxed));
        r.keys.released(0x56);
        assert!(r.shared.voice_on.load(Relaxed));
    }

    #[test]
    fn mute_and_preset_keys() {
        let r = rig(VoiceHotkeys {
            mute: Some(hk(0x4D, true, "Ctrl+M")),
            presets: vec![PresetHotkey {
                preset: "Deep Voice".into(),
                hotkey: Some(hk(0x61, false, "Num 1")),
            }],
            ..Default::default()
        });
        assert!(r.keys.pressed(&hk(0x4D, true, "Ctrl+M")));
        assert!(r.shared.mic_muted.load(Relaxed));
        r.keys.pressed(&hk(0x4D, true, "Ctrl+M"));
        assert!(!r.shared.mic_muted.load(Relaxed));
        assert!(r.keys.pressed(&hk(0x61, false, "Num 1")));
        assert_eq!(
            r.requests.try_recv().unwrap(),
            VoiceRequest::Preset("Deep Voice".into())
        );
    }

    #[test]
    fn melody_keys_restart_step_and_toggle() {
        let r = rig(VoiceHotkeys {
            melody_toggle: Some(hk(0x4D, false, "M")),
            melody_restart: Some(hk(0x52, true, "Ctrl+R")),
            melody_next: Some(hk(0x4E, true, "Ctrl+N")),
            ..Default::default()
        });
        assert!(r.keys.pressed(&hk(0x52, true, "Ctrl+R")));
        assert_eq!(r.shared.melody_restart.load(Relaxed), 1);
        for _ in 0..3 {
            assert!(r.keys.pressed(&hk(0x4E, true, "Ctrl+N")));
        }
        assert_eq!(r.shared.melody_next.load(Relaxed), 3);
        assert!(r.keys.pressed(&hk(0x4D, false, "M")));
        assert_eq!(r.requests.try_recv().unwrap(), VoiceRequest::ToggleMelody);
        assert!(
            r.shared.voice_on.load(Relaxed),
            "the voice changer is untouched"
        );
        assert!(
            !r.keys.pressed(&hk(0x4E, false, "N")),
            "N without Ctrl is not ours"
        );
        assert!(r.wakes.load(Relaxed) >= 5);
        // Conflicts name the Melody Voice keys too.
        let b = r.keys.bindings();
        assert_eq!(
            conflict(&hk(0x4E, true, "x"), KeyTarget::Toggle, &b, []).as_deref(),
            Some("next note")
        );
        assert_eq!(
            conflict(&hk(0x4D, false, "x"), KeyTarget::MelodyRestart, &b, []).as_deref(),
            Some("the Melody Voice toggle")
        );
        assert_eq!(
            conflict(&hk(0x52, true, "x"), KeyTarget::MelodyRestart, &b, []),
            None
        );
    }

    #[test]
    fn monitor_key_toggles_hear_myself_with_its_own_beeps_and_nothing_else() {
        let r = rig(VoiceHotkeys {
            monitor_toggle: Some(hk(0x48, false, "H")),
            ..Default::default()
        });
        r.shared.cue_enabled.store(true, Relaxed);
        let params = r.shared.params.snapshot();
        assert!(
            r.shared.monitor_voice.load(Relaxed),
            "starts on (the default)"
        );
        assert!(r.keys.pressed(&hk(0x48, false, "H")));
        assert!(!r.shared.monitor_voice.load(Relaxed));
        assert_eq!(r.shared.take_cue(), Some(CueKind::MonitorOff), "low beep");
        assert!(r.keys.pressed(&hk(0x48, false, "H")));
        assert!(r.shared.monitor_voice.load(Relaxed));
        assert_eq!(r.shared.take_cue(), Some(CueKind::MonitorOn), "high beep");
        // Only the monitor: the voice changer, the mute, the soundboard, the sliders and the
        // overlay are untouched.
        assert!(r.shared.voice_on.load(Relaxed) && !r.shared.mic_muted.load(Relaxed));
        assert_eq!(r.shared.stop_sounds.load(Relaxed), 0);
        assert_eq!(r.shared.params.snapshot(), params);
        assert_eq!(r.overlays.load(Relaxed), 0);
        assert!(r.wakes.load(Relaxed) >= 2, "the window and tray are told");
        // Beeps off: it still toggles, silently.
        r.shared.cue_enabled.store(false, Relaxed);
        r.keys.pressed(&hk(0x48, false, "H"));
        assert!(!r.shared.monitor_voice.load(Relaxed));
        assert_eq!(r.shared.take_cue(), None);
        // Ctrl+H is a different combination.
        assert!(!r.keys.pressed(&hk(0x48, true, "Ctrl+H")));
    }

    #[test]
    fn holding_the_monitor_key_toggles_once_and_stop_all_asks_the_engine() {
        let r = rig(VoiceHotkeys {
            monitor_toggle: Some(hk(0x77, false, "F8")),
            stop_sounds: Some(hk(0x7B, true, "Ctrl+F12")),
            ..Default::default()
        });
        let mut tracker = KeyTracker::default();
        let raw = |vk, ctrl, down| RawKey {
            vk,
            scan: 0,
            extended: false,
            down,
            ctrl,
            shift: false,
            alt: false,
            win: false,
        };
        // F8 held with auto-repeat (five downs), then released: one toggle.
        for down in [true, true, true, true, true, false] {
            if let Some(KeyEvent::Pressed(k)) =
                tracker.feed_event(raw(0x77, false, down), |_| "F8".into())
            {
                r.keys.pressed(&k);
            }
        }
        assert!(
            !r.shared.monitor_voice.load(Relaxed),
            "one press = one toggle"
        );
        // Ctrl+F12 twice: two stop requests, nothing else changes.
        for _ in 0..2 {
            for down in [true, true, false] {
                if let Some(KeyEvent::Pressed(k)) =
                    tracker.feed_event(raw(0x7B, true, down), |_| "F12".into())
                {
                    assert!(r.keys.pressed(&k));
                }
            }
        }
        assert_eq!(r.shared.stop_sounds.load(Relaxed), 2);
        assert!(!r.shared.monitor_voice.load(Relaxed));
        assert!(r.shared.voice_on.load(Relaxed));
        assert_eq!(
            r.shared.take_cue(),
            Some(CueKind::MonitorOff),
            "only the monitor beeped"
        );
    }

    #[test]
    fn soundboard_in_headphones_key_flips_only_that() {
        let r = rig(VoiceHotkeys {
            monitor_toggle: Some(hk(0x48, false, "H")),
            monitor_sounds_toggle: Some(hk(0x4A, false, "J")),
            ..Default::default()
        });
        r.shared.cue_enabled.store(true, Relaxed);
        assert!(r.shared.monitor_sounds.load(Relaxed), "on by default");
        assert!(r.keys.pressed(&hk(0x4A, false, "J")));
        assert!(!r.shared.monitor_sounds.load(Relaxed));
        assert_eq!(r.shared.take_cue(), Some(CueKind::MonitorOff));
        assert!(
            r.shared.monitor_voice.load(Relaxed),
            "hear myself untouched"
        );
        assert!(r.shared.voice_on.load(Relaxed));
        assert_eq!(r.shared.stop_sounds.load(Relaxed), 0, "clips keep playing");
        assert!(r.keys.pressed(&hk(0x4A, false, "J")));
        assert!(r.shared.monitor_sounds.load(Relaxed));
        assert_eq!(r.shared.take_cue(), Some(CueKind::MonitorOn));
        assert_eq!(
            conflict(
                &hk(0x4A, false, "J"),
                KeyTarget::MonitorToggle,
                &r.keys.bindings(),
                []
            )
            .as_deref(),
            Some("the soundboard-in-headphones toggle")
        );
        assert!(VoiceHotkeys::default().monitor_sounds_toggle.is_none());
    }

    #[test]
    fn new_keys_start_unassigned_and_take_part_in_conflict_checks() {
        let none = VoiceHotkeys::default();
        assert!(none.monitor_toggle.is_none() && none.stop_sounds.is_none());
        let voice = VoiceHotkeys {
            toggle: Some(hk(0x77, false, "F8")),
            monitor_toggle: Some(hk(0x48, false, "H")),
            stop_sounds: Some(hk(0x7B, true, "Ctrl+F12")),
            ..Default::default()
        };
        let num1 = hk(0x61, false, "Num 1");
        let sounds = || [(7u64, "airhorn", Some(&num1))];
        let h = hk(0x48, false, "H");
        let ctrl_f12 = hk(0x7B, true, "Ctrl+F12");
        assert_eq!(
            conflict(&h, KeyTarget::MonitorToggle, &voice, sounds()),
            None
        );
        assert_eq!(
            conflict(&h, KeyTarget::Sound(7), &voice, sounds()).as_deref(),
            Some("the monitor (hear myself) toggle")
        );
        assert_eq!(
            conflict(&ctrl_f12, KeyTarget::MonitorToggle, &voice, sounds()).as_deref(),
            Some("stop all sounds")
        );
        assert_eq!(
            conflict(
                &hk(0x77, false, "F8"),
                KeyTarget::StopSounds,
                &voice,
                sounds()
            )
            .as_deref(),
            Some("the voice changer toggle")
        );
        assert_eq!(
            conflict(&num1, KeyTarget::StopSounds, &voice, sounds()).as_deref(),
            Some("the sound \"airhorn\"")
        );
        // F12 without Ctrl is free.
        assert_eq!(
            conflict(
                &hk(0x7B, false, "F12"),
                KeyTarget::MonitorToggle,
                &voice,
                sounds()
            ),
            None
        );
    }

    #[test]
    fn conflicts_are_found_everywhere_but_not_with_yourself() {
        let voice = VoiceHotkeys {
            toggle: Some(hk(0x77, false, "F8")),
            mute: Some(hk(0x4D, true, "Ctrl+M")),
            presets: vec![PresetHotkey {
                preset: "Robot".into(),
                hotkey: Some(hk(0x62, false, "Num 2")),
            }],
            ..Default::default()
        };
        let num1 = hk(0x61, false, "Num 1");
        let sounds = || [(7u64, "airhorn", Some(&num1))];
        let f8 = hk(0x77, false, "F8");
        assert_eq!(conflict(&f8, KeyTarget::Toggle, &voice, sounds()), None);
        assert_eq!(
            conflict(&f8, KeyTarget::Sound(7), &voice, sounds()).as_deref(),
            Some("the voice changer toggle")
        );
        assert_eq!(
            conflict(&num1, KeyTarget::Toggle, &voice, sounds()).as_deref(),
            Some("the sound \"airhorn\"")
        );
        assert_eq!(conflict(&num1, KeyTarget::Sound(7), &voice, sounds()), None);
        assert_eq!(
            conflict(&hk(0x62, false, "x"), KeyTarget::Mute, &voice, sounds()).as_deref(),
            Some("the \"Robot\" preset")
        );
        assert_eq!(
            conflict(&hk(0x4D, true, "x"), KeyTarget::Preset(0), &voice, sounds()).as_deref(),
            Some("mute mic")
        );
        assert_eq!(
            conflict(&hk(0x4D, false, "M"), KeyTarget::Toggle, &voice, sounds()),
            None
        );
    }
}
