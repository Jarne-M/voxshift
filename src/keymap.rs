//! Who owns which hotkey. All hotkeys (voice changer, mute, presets, Melody Voice, monitor,
//! stop all and every soundboard clip) are one keymap: a key combination belongs to at most
//! one action. This module decides what happens when you assign a key, which keys are off
//! limits, and cleans up duplicates in a loaded config. No UI here, so it is tested on its own.
//!
//! Every binding has a fixed place in one list (voice toggle, mute, presets, Melody Voice,
//! monitor, stop all, then the soundboard clips in their order). "The first one wins" on load
//! means first in that list.

use crate::soundboard::hotkeys::Hotkey;
use crate::soundboard::mixer::SoundId;
use crate::voice_keys::{KeyTarget, VoiceHotkeys};

/// A soundboard clip's hotkey, as the keymap sees it (the clip's id, name and key slot).
pub struct SoundKey<'a> {
    pub id: SoundId,
    pub name: &'a str,
    pub hotkey: &'a mut Option<Hotkey>,
}

/// The action that already has a key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Owner {
    pub target: KeyTarget,
    /// "the sound \"airhorn\"", "mute mic", ...
    pub name: String,
}

/// What assigning a key would do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Check {
    /// Free: assign it.
    Assign,
    /// The action already has exactly this key: nothing to do.
    AlreadyYours,
    /// Another action has it: ask whether to keep that or move the key over.
    Conflict(Owner),
    /// Windows or VoxShift needs this key; refuse with this explanation.
    Reserved(&'static str),
}

const VK_TAB: u32 = 0x09;
const VK_ESCAPE: u32 = 0x1B;
const VK_DELETE: u32 = 0x2E;
const VK_F4: u32 = 0x73;

/// Keys that cannot be hotkeys, and why.
pub fn reserved(k: &Hotkey) -> Option<&'static str> {
    let plain = !(k.ctrl || k.shift || k.alt || k.win);
    if k.win {
        return Some("Windows key combinations belong to Windows");
    }
    if k.vk == VK_ESCAPE {
        return Some(if plain {
            "Esc cancels assigning a key"
        } else {
            "that Esc combination belongs to Windows (Start menu, Task Manager, switching apps)"
        });
    }
    if k.ctrl && k.alt && k.vk == VK_DELETE {
        return Some("Ctrl+Alt+Del belongs to Windows");
    }
    if k.alt && !k.ctrl && (k.vk == VK_TAB || k.vk == VK_F4) {
        return Some("Alt+Tab and Alt+F4 belong to Windows");
    }
    None
}

/// Every action's name and key, in keymap order (the sounds after the voice keys).
fn bindings<'a>(
    voice: &'a VoiceHotkeys,
    sounds: impl IntoIterator<Item = (SoundId, &'a str, Option<&'a Hotkey>)>,
) -> Vec<(KeyTarget, String, Option<&'a Hotkey>)> {
    let mut out = vec![
        (
            KeyTarget::Toggle,
            "the voice changer toggle".to_string(),
            voice.toggle.as_ref(),
        ),
        (KeyTarget::Mute, "mute mic".to_string(), voice.mute.as_ref()),
    ];
    for (i, p) in voice.presets.iter().enumerate() {
        out.push((
            KeyTarget::Preset(i),
            format!("the \"{}\" preset", p.preset),
            p.hotkey.as_ref(),
        ));
    }
    for (t, h, name) in voice.action_keys() {
        out.push((t, name.to_string(), h.as_ref()));
    }
    for (id, name, h) in sounds {
        out.push((KeyTarget::Sound(id), format!("the sound \"{name}\""), h));
    }
    out
}

/// The action that has `key`, other than `target` itself.
pub fn owner<'a>(
    key: &Hotkey,
    target: KeyTarget,
    voice: &'a VoiceHotkeys,
    sounds: impl IntoIterator<Item = (SoundId, &'a str, Option<&'a Hotkey>)>,
) -> Option<Owner> {
    bindings(voice, sounds)
        .into_iter()
        .find(|(t, _, h)| *t != target && h.is_some_and(|h| h.same_keys(key)))
        .map(|(target, name, _)| Owner { target, name })
}

/// How an action is named to the user (for messages like "now stops all sounds").
pub fn name_of<'a>(
    target: KeyTarget,
    voice: &'a VoiceHotkeys,
    sounds: impl IntoIterator<Item = (SoundId, &'a str, Option<&'a Hotkey>)>,
) -> String {
    bindings(voice, sounds)
        .into_iter()
        .find(|(t, _, _)| *t == target)
        .map_or_else(|| "that action".to_string(), |(_, name, _)| name)
}

/// What assigning `key` to `target` would do.
pub fn check<'a>(
    key: &Hotkey,
    target: KeyTarget,
    voice: &'a VoiceHotkeys,
    sounds: impl IntoIterator<Item = (SoundId, &'a str, Option<&'a Hotkey>)> + Clone,
) -> Check {
    if let Some(why) = reserved(key) {
        return Check::Reserved(why);
    }
    let current = bindings(voice, sounds.clone())
        .into_iter()
        .find(|(t, _, _)| *t == target)
        .and_then(|(_, _, h)| h.cloned());
    if current.is_some_and(|c| c.same_keys(key)) {
        return Check::AlreadyYours;
    }
    match owner(key, target, voice, sounds) {
        Some(o) => Check::Conflict(o),
        None => Check::Assign,
    }
}

/// Sets (or clears) one action's key. False if there is no such action.
pub fn assign(
    voice: &mut VoiceHotkeys,
    sounds: &mut [SoundKey],
    target: KeyTarget,
    key: Option<Hotkey>,
) -> bool {
    let slot = match target {
        KeyTarget::Sound(id) => match sounds.iter_mut().find(|s| s.id == id) {
            Some(s) => &mut *s.hotkey,
            None => return false,
        },
        KeyTarget::Toggle => &mut voice.toggle,
        KeyTarget::Mute => &mut voice.mute,
        KeyTarget::MelodyToggle => &mut voice.melody_toggle,
        KeyTarget::MelodyRestart => &mut voice.melody_restart,
        KeyTarget::MelodyNext => &mut voice.melody_next,
        KeyTarget::MonitorToggle => &mut voice.monitor_toggle,
        KeyTarget::MonitorSoundsToggle => &mut voice.monitor_sounds_toggle,
        KeyTarget::StopSounds => &mut voice.stop_sounds,
        KeyTarget::Panic => &mut voice.panic,
        KeyTarget::Preset(i) => match voice.presets.get_mut(i) {
            Some(p) => &mut p.hotkey,
            None => return false,
        },
    };
    *slot = key;
    true
}

/// "Change" in the conflict dialog: `key` moves from `owner` to `target`; `owner` is left
/// without a key.
pub fn change(
    voice: &mut VoiceHotkeys,
    sounds: &mut [SoundKey],
    key: Hotkey,
    target: KeyTarget,
    owner: KeyTarget,
) {
    assign(voice, sounds, owner, None);
    assign(voice, sounds, target, Some(key));
}

/// Makes every key unique again (a hand-edited or old config): the first action in keymap
/// order keeps it, the others are cleared. Returns what was cleared, e.g.
/// "F8 (the sound \"airhorn\")".
pub fn dedupe(voice: &mut VoiceHotkeys, sounds: &mut [SoundKey]) -> Vec<String> {
    let losers: Vec<(KeyTarget, String)> = {
        let list = bindings(
            voice,
            sounds.iter().map(|s| (s.id, s.name, s.hotkey.as_ref())),
        );
        let mut seen: Vec<&Hotkey> = Vec::new();
        let mut losers = Vec::new();
        for (target, name, h) in &list {
            let Some(h) = h else { continue };
            if seen.iter().any(|s| s.same_keys(h)) {
                losers.push((*target, format!("{} ({name})", h.label)));
            } else {
                seen.push(h);
            }
        }
        losers
    };
    for (target, _) in &losers {
        assign(voice, sounds, *target, None);
    }
    losers.into_iter().map(|(_, what)| what).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice_keys::PresetHotkey;

    fn hk(vk: u32, ctrl: bool, label: &str) -> Hotkey {
        Hotkey::new(vk, ctrl, false, false, false, label)
    }

    /// Three clips: airhorn has Num 1, bruh has nothing, wow has F9.
    struct Board {
        names: Vec<String>,
        keys: Vec<Option<Hotkey>>,
    }

    impl Board {
        fn new() -> Self {
            Self {
                names: vec!["airhorn".into(), "bruh".into(), "wow".into()],
                keys: vec![
                    Some(hk(0x61, false, "Num 1")),
                    None,
                    Some(hk(0x78, false, "F9")),
                ],
            }
        }

        fn read(&self) -> impl Iterator<Item = (SoundId, &str, Option<&Hotkey>)> + Clone {
            self.names
                .iter()
                .zip(&self.keys)
                .enumerate()
                .map(|(i, (n, k))| (i as SoundId + 1, n.as_str(), k.as_ref()))
        }

        fn slots(&mut self) -> Vec<SoundKey<'_>> {
            self.names
                .iter()
                .zip(self.keys.iter_mut())
                .enumerate()
                .map(|(i, (n, k))| SoundKey {
                    id: i as SoundId + 1,
                    name: n,
                    hotkey: k,
                })
                .collect()
        }
    }

    fn voice() -> VoiceHotkeys {
        VoiceHotkeys {
            toggle: Some(hk(0x77, false, "F8")),
            mute: Some(hk(0x4D, true, "Ctrl+M")),
            presets: vec![PresetHotkey {
                preset: "Robot".into(),
                hotkey: Some(hk(0x62, false, "Num 2")),
            }],
            stop_sounds: Some(hk(0x7B, false, "F12")),
            ..Default::default()
        }
    }

    #[test]
    fn a_free_key_is_assigned() {
        let (mut v, mut b) = (voice(), Board::new());
        let k = hk(0x79, false, "F10");
        assert_eq!(
            check(&k, KeyTarget::MonitorToggle, &v, b.read()),
            Check::Assign
        );
        assert!(assign(
            &mut v,
            &mut b.slots(),
            KeyTarget::MonitorToggle,
            Some(k)
        ));
        assert_eq!(v.monitor_toggle.as_ref().unwrap().label, "F10");
        // A clip too.
        let k = hk(0x7A, false, "F11");
        assert_eq!(check(&k, KeyTarget::Sound(2), &v, b.read()), Check::Assign);
        assert!(assign(&mut v, &mut b.slots(), KeyTarget::Sound(2), Some(k)));
        assert_eq!(b.keys[1].as_ref().unwrap().label, "F11");
        assert!(
            !assign(&mut v, &mut b.slots(), KeyTarget::Sound(99), None),
            "no such clip"
        );
    }

    #[test]
    fn a_taken_key_names_its_owner_and_keep_changes_nothing() {
        let (v, b) = (voice(), Board::new());
        // Num 1 belongs to the clip "airhorn": the dialog names it.
        let c = check(&hk(0x61, false, "x"), KeyTarget::Toggle, &v, b.read());
        assert_eq!(
            c,
            Check::Conflict(Owner {
                target: KeyTarget::Sound(1),
                name: "the sound \"airhorn\"".into(),
            })
        );
        // Every kind of owner is found, by its name.
        let owner_of = |k: Hotkey, t| match check(&k, t, &v, b.read()) {
            Check::Conflict(o) => o.name,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            owner_of(hk(0x77, false, "F8"), KeyTarget::Sound(1)),
            "the voice changer toggle"
        );
        assert_eq!(
            owner_of(hk(0x4D, true, "Ctrl+M"), KeyTarget::Toggle),
            "mute mic"
        );
        assert_eq!(
            owner_of(hk(0x62, false, "Num 2"), KeyTarget::Mute),
            "the \"Robot\" preset"
        );
        assert_eq!(
            owner_of(hk(0x7B, false, "F12"), KeyTarget::Toggle),
            "stop all sounds"
        );
        // "Keep" means: nothing is written. The keymap is exactly as before.
        assert_eq!(v, voice());
        assert_eq!(b.keys, Board::new().keys);
        // Ctrl+Num 1 is a different combination: free.
        assert_eq!(
            check(
                &hk(0x61, true, "Ctrl+Num 1"),
                KeyTarget::Toggle,
                &v,
                b.read()
            ),
            Check::Assign
        );
    }

    #[test]
    fn change_moves_the_key_and_leaves_the_old_owner_unassigned() {
        let (mut v, mut b) = (voice(), Board::new());
        let num1 = hk(0x61, false, "Num 1");
        // The voice changer toggle takes Num 1 from the clip "airhorn".
        let Check::Conflict(o) = check(&num1, KeyTarget::Toggle, &v, b.read()) else {
            panic!()
        };
        change(
            &mut v,
            &mut b.slots(),
            num1.clone(),
            KeyTarget::Toggle,
            o.target,
        );
        assert_eq!(v.toggle.as_ref().unwrap().label, "Num 1");
        assert!(b.keys[0].is_none(), "airhorn is not set any more");
        assert_eq!(
            check(&num1, KeyTarget::Toggle, &v, b.read()),
            Check::AlreadyYours
        );
        // And the other way: clip "wow" takes F12 from stop all.
        let f12 = hk(0x7B, false, "F12");
        let Check::Conflict(o) = check(&f12, KeyTarget::Sound(3), &v, b.read()) else {
            panic!()
        };
        change(&mut v, &mut b.slots(), f12, KeyTarget::Sound(3), o.target);
        assert!(v.stop_sounds.is_none());
        assert_eq!(b.keys[2].as_ref().unwrap().label, "F12");
        // A preset key moves too.
        let num2 = hk(0x62, false, "Num 2");
        let Check::Conflict(o) = check(&num2, KeyTarget::MelodyNext, &v, b.read()) else {
            panic!()
        };
        change(
            &mut v,
            &mut b.slots(),
            num2,
            KeyTarget::MelodyNext,
            o.target,
        );
        assert!(v.presets[0].hotkey.is_none() && v.melody_next.is_some());
    }

    #[test]
    fn the_same_key_again_is_not_a_conflict() {
        let (v, b) = (voice(), Board::new());
        // Pressing F8 again for the toggle that already has F8 (labels do not matter).
        assert_eq!(
            check(
                &hk(0x77, false, "F8 (again)"),
                KeyTarget::Toggle,
                &v,
                b.read()
            ),
            Check::AlreadyYours
        );
        assert_eq!(
            check(&hk(0x78, false, "F9"), KeyTarget::Sound(3), &v, b.read()),
            Check::AlreadyYours
        );
        // But a different action pressing F8 is.
        assert!(matches!(
            check(&hk(0x77, false, "F8"), KeyTarget::Mute, &v, b.read()),
            Check::Conflict(_)
        ));
    }

    #[test]
    fn reserved_keys_are_refused_with_a_reason() {
        let (v, b) = (voice(), Board::new());
        let key = |vk, ctrl, shift, alt, win| Hotkey::new(vk, ctrl, shift, alt, win, "k");
        for k in [
            key(0x52, false, false, false, true),  // Win+R
            key(0x44, true, false, false, true),   // Ctrl+Win+D
            key(0x1B, false, false, false, false), // Esc
            key(0x1B, true, true, false, false),   // Ctrl+Shift+Esc
            key(0x1B, true, false, false, false),  // Ctrl+Esc
            key(0x2E, true, false, true, false),   // Ctrl+Alt+Del
            key(0x09, false, false, true, false),  // Alt+Tab
            key(0x73, false, false, true, false),  // Alt+F4
        ] {
            assert!(
                matches!(
                    check(&k, KeyTarget::Toggle, &v, b.read()),
                    Check::Reserved(_)
                ),
                "{k:?}"
            );
        }
        assert!(
            reserved(&key(0x1B, false, false, false, false))
                .unwrap()
                .contains("cancels")
        );
        // Ordinary keys and combos are fine, Delete and F4 included.
        for k in [
            key(0x2E, false, false, false, false),
            key(0x2E, true, false, false, false),
            key(0x73, false, false, false, false),
            key(0x41, true, true, true, false),
        ] {
            assert_eq!(reserved(&k), None, "{k:?}");
        }
    }

    #[test]
    fn duplicates_in_a_loaded_config_keep_the_first_and_report_the_rest() {
        let mut v = VoiceHotkeys {
            toggle: Some(hk(0x77, false, "F8")),
            mute: Some(hk(0x77, false, "F8")), // same as the toggle
            monitor_toggle: Some(hk(0x61, false, "Num 1")), // same as clip airhorn
            ..Default::default()
        };
        let mut b = Board::new();
        b.keys[2] = Some(hk(0x77, false, "F8")); // clip "wow" has F8 too
        let cleared = dedupe(&mut v, &mut b.slots());
        assert_eq!(
            cleared,
            [
                "F8 (mute mic)",
                "Num 1 (the sound \"airhorn\")",
                "F8 (the sound \"wow\")",
            ]
        );
        assert_eq!(v.toggle.as_ref().unwrap().label, "F8", "the first keeps it");
        assert!(v.mute.is_none());
        assert_eq!(
            v.monitor_toggle.as_ref().unwrap().label,
            "Num 1",
            "voice keys come first"
        );
        assert!(b.keys[0].is_none() && b.keys[2].is_none());
        // Now unique: a second pass changes nothing.
        assert!(dedupe(&mut v, &mut b.slots()).is_empty());
        // A clean config is left alone.
        let (mut v, mut b) = (voice(), Board::new());
        assert!(dedupe(&mut v, &mut b.slots()).is_empty());
        assert_eq!(v, voice());
    }
}
