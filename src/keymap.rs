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
        .find(|(t, _, h)| *t != target && h.is_some_and(|h| h.matches(key)))
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

/// The key `target` has now (with its second key), if any.
pub fn current<'a>(
    target: KeyTarget,
    voice: &'a VoiceHotkeys,
    sounds: impl IntoIterator<Item = (SoundId, &'a str, Option<&'a Hotkey>)>,
) -> Option<Hotkey> {
    bindings(voice, sounds)
        .into_iter()
        .find(|(t, _, _)| *t == target)
        .and_then(|(_, _, h)| h.cloned())
}

/// What assigning `key` to `target` (as its main or its second key) would do.
pub fn check<'a>(
    key: &Hotkey,
    target: KeyTarget,
    voice: &'a VoiceHotkeys,
    sounds: impl IntoIterator<Item = (SoundId, &'a str, Option<&'a Hotkey>)> + Clone,
) -> Check {
    if let Some(why) = reserved(key) {
        return Check::Reserved(why);
    }
    if current(target, voice, sounds.clone()).is_some_and(|c| c.matches(key)) {
        return Check::AlreadyYours;
    }
    match owner(key, target, voice, sounds) {
        Some(o) => Check::Conflict(o),
        None => Check::Assign,
    }
}

/// Which of an action's two keys.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Slot {
    /// The key every action has (the one shown first).
    #[default]
    Main,
    /// An extra key that does the same, e.g. "8" next to "Ctrl + 8".
    Second,
}

/// Where an action's key is stored.
fn slot_of<'s>(
    voice: &'s mut VoiceHotkeys,
    sounds: &'s mut [SoundKey],
    target: KeyTarget,
) -> Option<&'s mut Option<Hotkey>> {
    Some(match target {
        KeyTarget::Sound(id) => &mut *sounds.iter_mut().find(|s| s.id == id)?.hotkey,
        KeyTarget::Toggle => &mut voice.toggle,
        KeyTarget::Mute => &mut voice.mute,
        KeyTarget::MelodyToggle => &mut voice.melody_toggle,
        KeyTarget::MelodyRestart => &mut voice.melody_restart,
        KeyTarget::MelodyNext => &mut voice.melody_next,
        KeyTarget::MonitorToggle => &mut voice.monitor_toggle,
        KeyTarget::MonitorSoundsToggle => &mut voice.monitor_sounds_toggle,
        KeyTarget::StopSounds => &mut voice.stop_sounds,
        KeyTarget::Panic => &mut voice.panic,
        KeyTarget::Preset(i) => &mut voice.presets.get_mut(i)?.hotkey,
    })
}

/// Sets (or, with `None`, removes) one action's main key; its second key stays. Without a
/// main key the second key becomes the main one. False if there is no such action.
#[cfg(test)]
pub fn assign(
    voice: &mut VoiceHotkeys,
    sounds: &mut [SoundKey],
    target: KeyTarget,
    key: Option<Hotkey>,
) -> bool {
    assign_slot(voice, sounds, target, Slot::Main, key)
}

/// Sets (or removes) one of an action's two keys, leaving the other. A second key for an
/// action without a main key becomes its main key. False if there is no such action.
pub fn assign_slot(
    voice: &mut VoiceHotkeys,
    sounds: &mut [SoundKey],
    target: KeyTarget,
    which: Slot,
    key: Option<Hotkey>,
) -> bool {
    let Some(slot) = slot_of(voice, sounds, target) else {
        return false;
    };
    let main = slot.as_ref().map(Hotkey::single);
    let second = slot.as_ref().and_then(|h| h.second.as_deref()).cloned();
    *slot = match which {
        Slot::Main => Hotkey::pair(key, second),
        Slot::Second => Hotkey::pair(main, key),
    };
    true
}

/// Removes both of an action's keys. False if there is no such action.
pub fn clear(voice: &mut VoiceHotkeys, sounds: &mut [SoundKey], target: KeyTarget) -> bool {
    slot_of(voice, sounds, target).map(|s| *s = None).is_some()
}

/// Takes the combination `key` away from `target` (whichever of its keys it is); its other
/// key stays.
fn remove_key(voice: &mut VoiceHotkeys, sounds: &mut [SoundKey], target: KeyTarget, key: &Hotkey) {
    let Some(slot) = slot_of(voice, sounds, target) else {
        return;
    };
    let Some(h) = slot.take() else { return };
    let mut keys: Vec<Hotkey> = h.keys().map(Hotkey::single).collect();
    // The last one: a (hand-edited) pair of two equal keys keeps one of them.
    if let Some(i) = keys.iter().rposition(|k| k.same_keys(key)) {
        keys.remove(i);
    }
    let mut keys = keys.into_iter();
    *slot = Hotkey::pair(keys.next(), keys.next());
}

/// "Change" in the conflict dialog: `key` moves from `owner` to `target` (as its `which`
/// key); `owner` loses that key (its other key, if any, stays).
pub fn change(
    voice: &mut VoiceHotkeys,
    sounds: &mut [SoundKey],
    key: Hotkey,
    target: KeyTarget,
    which: Slot,
    owner: KeyTarget,
) {
    remove_key(voice, sounds, owner, &key);
    assign_slot(voice, sounds, target, which, Some(key));
}

/// Makes every key unique again (a hand-edited or old config): the first action in keymap
/// order keeps it, the others are cleared. Returns what was cleared, e.g.
/// "F8 (the sound \"airhorn\")".
pub fn dedupe(voice: &mut VoiceHotkeys, sounds: &mut [SoundKey]) -> Vec<String> {
    let losers: Vec<(KeyTarget, Hotkey, String)> = {
        let list = bindings(
            voice,
            sounds.iter().map(|s| (s.id, s.name, s.hotkey.as_ref())),
        );
        let mut seen: Vec<&Hotkey> = Vec::new();
        let mut losers = Vec::new();
        for (target, name, h) in &list {
            let Some(h) = h else { continue };
            for k in h.keys() {
                if seen.iter().any(|s| s.same_keys(k)) {
                    losers.push((*target, k.single(), format!("{} ({name})", k.pretty())));
                } else {
                    seen.push(k);
                }
            }
        }
        losers
    };
    for (target, key, _) in &losers {
        remove_key(voice, sounds, *target, key);
    }
    losers.into_iter().map(|(_, _, what)| what).collect()
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
            Slot::Main,
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
        change(
            &mut v,
            &mut b.slots(),
            f12,
            KeyTarget::Sound(3),
            Slot::Main,
            o.target,
        );
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
            Slot::Main,
            o.target,
        );
        assert!(v.presets[0].hotkey.is_none() && v.melody_next.is_some());
    }

    #[test]
    fn a_second_key_does_the_same_and_counts_in_the_keymap() {
        let (mut v, mut b) = (voice(), Board::new());
        // Stop all has F12; "8" becomes its second key (Ctrl + 8 is the user's case).
        let eight = hk(0x38, false, "8");
        assert_eq!(
            check(&eight, KeyTarget::StopSounds, &v, b.read()),
            Check::Assign
        );
        assert!(assign_slot(
            &mut v,
            &mut b.slots(),
            KeyTarget::StopSounds,
            Slot::Second,
            Some(eight.clone())
        ));
        let stop = v.stop_sounds.clone().unwrap();
        assert_eq!(stop.label, "F12", "the main key stays");
        assert!(stop.matches(&eight) && stop.matches(&hk(0x7B, false, "F12")));
        assert!(!stop.matches(&hk(0x38, true, "Ctrl+8")));
        assert_eq!(stop.pretty_all(), "F12 or 8");
        // Either key is "already yours" for stop all, and taken for anything else.
        assert_eq!(
            check(&eight, KeyTarget::StopSounds, &v, b.read()),
            Check::AlreadyYours
        );
        let Check::Conflict(o) = check(&eight, KeyTarget::Sound(2), &v, b.read()) else {
            panic!()
        };
        assert_eq!(o.target, KeyTarget::StopSounds);
        // Changing the main key keeps the second one.
        assign(
            &mut v,
            &mut b.slots(),
            KeyTarget::StopSounds,
            Some(hk(0x38, true, "Ctrl+8")),
        );
        assert_eq!(
            v.stop_sounds.as_ref().unwrap().pretty_all(),
            "Ctrl + 8 or 8"
        );
        // The clip "bruh" takes "8": stop all keeps Ctrl + 8.
        change(
            &mut v,
            &mut b.slots(),
            eight.clone(),
            KeyTarget::Sound(2),
            Slot::Main,
            KeyTarget::StopSounds,
        );
        assert_eq!(v.stop_sounds.as_ref().unwrap().pretty_all(), "Ctrl + 8");
        assert_eq!(b.keys[1].as_ref().unwrap().label, "8");
        // Removing the main key of a pair leaves the second as the main key.
        assign_slot(
            &mut v,
            &mut b.slots(),
            KeyTarget::StopSounds,
            Slot::Second,
            Some(hk(0x7B, false, "F12")),
        );
        assign(&mut v, &mut b.slots(), KeyTarget::StopSounds, None);
        assert_eq!(v.stop_sounds.as_ref().unwrap().pretty_all(), "F12");
        // A second key without a main one becomes the main key; clear removes both.
        assign_slot(&mut v, &mut b.slots(), KeyTarget::Mute, Slot::Main, None);
        assign_slot(
            &mut v,
            &mut b.slots(),
            KeyTarget::Mute,
            Slot::Second,
            Some(hk(0x4D, true, "Ctrl+M")),
        );
        assert_eq!(v.mute.as_ref().unwrap().pretty_all(), "Ctrl + M");
        assign_slot(
            &mut v,
            &mut b.slots(),
            KeyTarget::Mute,
            Slot::Second,
            Some(hk(0x4E, true, "Ctrl+N")),
        );
        assert!(clear(&mut v, &mut b.slots(), KeyTarget::Mute));
        assert!(v.mute.is_none());
        assert!(!clear(&mut v, &mut b.slots(), KeyTarget::Sound(99)));
    }

    #[test]
    fn duplicate_second_keys_in_a_loaded_config_are_cleared_too() {
        let mut v = voice();
        let mut b = Board::new();
        // The toggle's second key is the clip airhorn's key, and wow's second key is F8.
        v.toggle = Hotkey::pair(v.toggle.clone(), Some(hk(0x7A, false, "F11")));
        b.keys[0] = Hotkey::pair(b.keys[0].clone(), Some(hk(0x7A, false, "F11")));
        b.keys[2] = Hotkey::pair(b.keys[2].clone(), Some(hk(0x77, false, "F8")));
        let cleared = dedupe(&mut v, &mut b.slots());
        assert_eq!(
            cleared,
            ["F11 (the sound \"airhorn\")", "F8 (the sound \"wow\")"]
        );
        assert_eq!(v.toggle.as_ref().unwrap().pretty_all(), "F8 or F11");
        assert_eq!(b.keys[0].as_ref().unwrap().pretty_all(), "Num 1");
        assert_eq!(b.keys[2].as_ref().unwrap().pretty_all(), "F9");
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
