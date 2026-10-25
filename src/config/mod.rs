//! Everything the app remembers between runs, stored as JSON in
//! `%APPDATA%\VoxShift\config.json`.

pub mod pack;
pub mod presets;

use crate::audio::device_manager::DeviceSettings;
use crate::audio::mic_select::MicSettings;
use crate::dsp::params::EffectSettings;
use crate::melody::MelodySource;
use crate::soundboard::library::SoundEntry;
use crate::soundboard::organize::BoardSettings;
use crate::voice_keys::VoiceHotkeys;
use presets::Preset;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Which page of the main area is open (remembered between runs).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tab {
    #[default]
    Effects,
    Soundboard,
}

/// What the window's close button (X / Alt+F4) does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CloseBehavior {
    /// Show the "minimize to tray / quit / cancel" dialog.
    #[default]
    Ask,
    MinimizeToTray,
    Quit,
}

/// Whether the voice changer starts with effects on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum StartVoice {
    /// As it was when VoxShift last closed.
    #[default]
    RememberLast,
    On,
    Off,
}

/// How a voice changer switch is announced.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FeedbackSettings {
    /// Beep in the headphones (monitor output only).
    pub cues: bool,
    pub cue_volume: f32,
    /// Small always-on-top "Voice changer ON/OFF" for about a second.
    pub overlay: bool,
    /// The panic button also silences "hear myself" and "hear soundboard".
    pub panic_mutes_monitor: bool,
}

impl Default for FeedbackSettings {
    fn default() -> Self {
        Self {
            cues: true,
            cue_volume: 0.5,
            overlay: false,
            panic_mutes_monitor: false,
        }
    }
}

/// Melody Voice's melody choice and your imported MIDI files.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MelodySettings {
    pub source: MelodySource,
    /// MIDI files you imported, newest first.
    pub files: Vec<PathBuf>,
}

/// How many imported MIDI files are remembered.
pub const MAX_MELODY_FILES: usize = 20;

impl MelodySettings {
    /// Remembers an imported file (moving it to the front).
    pub fn remember_file(&mut self, path: &Path) {
        self.files.retain(|p| p != path);
        self.files.insert(0, path.to_path_buf());
        self.files.truncate(MAX_MELODY_FILES);
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub tab: Tab,
    pub voice_keys: VoiceHotkeys,
    pub feedback: FeedbackSettings,
    pub start_voice: StartVoice,
    pub close_behavior: CloseBehavior,
    /// Start hidden in the tray (useful with "Start with Windows", which lives in the
    /// registry, not here).
    pub start_minimized: bool,
    pub devices: DeviceSettings,
    pub mic: MicSettings,
    pub voice_on: bool,
    pub monitor_volume: f32,
    pub monitor_voice: bool,
    pub monitor_sounds: bool,
    /// Current slider values.
    pub effects: EffectSettings,
    /// Name of the preset last applied (shown as "modified" once sliders move).
    pub preset: Option<String>,
    pub user_presets: Vec<Preset>,
    pub sounds: Vec<SoundEntry>,
    /// Soundboard folders, recents, the open tab and the sort order.
    pub soundboard: BoardSettings,
    pub melody: MelodySettings,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tab: Tab::Effects,
            voice_keys: VoiceHotkeys::default(),
            feedback: FeedbackSettings::default(),
            start_voice: StartVoice::RememberLast,
            close_behavior: CloseBehavior::Ask,
            start_minimized: false,
            devices: DeviceSettings::default(),
            mic: MicSettings::default(),
            voice_on: true,
            monitor_volume: 0.8,
            monitor_voice: true,
            monitor_sounds: true,
            effects: EffectSettings::new(),
            preset: None,
            user_presets: Vec::new(),
            sounds: Vec::new(),
            soundboard: BoardSettings::default(),
            melody: MelodySettings::default(),
        }
    }
}

/// `%APPDATA%\VoxShift` (falls back to the working directory if APPDATA is not set).
pub fn config_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("VoxShift")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

/// Loads the config. A missing file gives the defaults; a broken one is kept aside as
/// `config.broken.json` and the defaults are used, with a warning to show.
pub fn load_from(path: &Path) -> (Config, Option<String>) {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Config::default(), None),
        Err(e) => {
            return (
                Config::default(),
                Some(format!("Could not read settings: {e}")),
            );
        }
    };
    match serde_json::from_str(&text) {
        Ok(cfg) => (cfg, None),
        Err(e) => {
            let backup = path.with_file_name("config.broken.json");
            let _ = std::fs::rename(path, &backup);
            (
                Config::default(),
                Some(format!(
                    "Settings file was damaged ({e}); started fresh, old file kept as {}",
                    backup.display()
                )),
            )
        }
    }
}

/// Writes atomically: a temporary file first, then a rename, so a crash mid-write never
/// leaves a half-written config.
pub fn save_to(path: &Path, cfg: &Config) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::soundboard::decode::tests::temp_dir;
    use crate::soundboard::hotkeys::Hotkey;

    #[test]
    fn round_trips_everything() {
        let dir = temp_dir("config");
        let path = dir.join("config.json");
        let mut cfg = Config::default();
        cfg.devices.primary = "Microphone (HyperX Cloud II Wireless)".into();
        cfg.devices.fallback = "Microphone (USB PnP Audio Device)".into();
        cfg.devices.output = "CABLE Input (VB-Audio Virtual Cable)".into();
        cfg.mic.silence_secs = 7.0;
        cfg.voice_on = false;
        cfg.effects.insert("pitch_semitones".into(), -3.0);
        cfg.preset = Some("Mine".into());
        cfg.effects.insert("melody_on".into(), 1.0);
        cfg.effects.insert("melody_mode".into(), 1.0);
        cfg.effects.insert("melody_bpm".into(), 132.0);
        cfg.user_presets.push(Preset {
            name: "Mine".into(),
            effects: cfg.effects.clone(),
            melody: Some(MelodySource::File {
                path: "C:/music/my song.mid".into(),
                part: Some(1),
            }),
        });
        cfg.melody.source = MelodySource::BuiltIn("Greensleeves".into());
        cfg.melody.remember_file(Path::new("C:/music/b.mid"));
        cfg.melody.remember_file(Path::new("C:/music/a.mid"));
        cfg.melody.remember_file(Path::new("C:/music/b.mid"));
        assert_eq!(
            cfg.melody.files,
            [
                PathBuf::from("C:/music/b.mid"),
                PathBuf::from("C:/music/a.mid")
            ],
            "newest first, no duplicates"
        );
        let folder = cfg.soundboard.create_folder("Memes");
        cfg.sounds.push(SoundEntry {
            name: "airhorn".into(),
            path: "C:/sounds/airhorn.mp3".into(),
            volume: 0.5,
            hotkey: Some(Hotkey::new(0x61, true, false, false, false, "Ctrl+Num 1")),
            uid: 7,
            folder: Some(folder),
            favorite: true,
            color: Some(3),
            plays: 12,
            play_mode: crate::soundboard::playmode::PlayMode::LoopWhileHeld,
        });
        cfg.soundboard.recent = vec![7];
        cfg.soundboard.tab = crate::soundboard::organize::BoardTab::Folder(folder);
        cfg.soundboard.sort = crate::soundboard::organize::SortMode::MostPlayed;
        save_to(&path, &cfg).unwrap();
        let (loaded, warning) = load_from(&path);
        assert!(warning.is_none());
        assert_eq!(loaded, cfg);
        assert_eq!(
            loaded.sounds[0].hotkey.as_ref().unwrap().label,
            "Ctrl+Num 1"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn voice_hotkeys_feedback_and_start_state_round_trip() {
        use crate::voice_keys::{PresetHotkey, VoiceKeyMode};
        let dir = temp_dir("config-voicekeys");
        let path = dir.join("config.json");
        let cfg = Config {
            voice_keys: VoiceHotkeys {
                toggle: Some(Hotkey::new(0x77, false, false, false, false, "F8")),
                mode: VoiceKeyMode::HoldForClean,
                mute: Some(Hotkey::new(0x4D, true, false, false, false, "Ctrl+M")),
                presets: vec![PresetHotkey {
                    preset: "Deep Voice".into(),
                    hotkey: Some(Hotkey::new(0x61, false, false, true, false, "Alt+Num 1")),
                }],
                melody_toggle: Some(Hotkey::new(0x76, false, false, false, false, "F7")),
                melody_restart: Some(Hotkey::new(0x52, true, true, false, false, "Ctrl+Shift+R")),
                melody_next: Some(Hotkey::new(0x22, false, false, false, false, "Page Down")),
                monitor_toggle: Some(Hotkey::new(0x48, false, false, true, false, "Alt+H")),
                monitor_sounds_toggle: Some(Hotkey::new(0x4A, false, false, true, false, "Alt+J")),
                stop_sounds: Some(Hotkey::new(0x6E, false, false, false, false, "Num Del")),
                panic: Some(Hotkey::new(0x13, true, false, false, false, "Ctrl+Pause")),
            },
            feedback: FeedbackSettings {
                cues: false,
                cue_volume: 0.25,
                overlay: true,
                panic_mutes_monitor: true,
            },
            start_voice: StartVoice::Off,
            ..Config::default()
        };
        save_to(&path, &cfg).unwrap();
        let (loaded, warning) = load_from(&path);
        assert!(warning.is_none());
        assert_eq!(loaded, cfg);
        assert_eq!(
            loaded.voice_keys.presets[0].hotkey.as_ref().unwrap().label,
            "Alt+Num 1"
        );
        assert_eq!(
            loaded.voice_keys.melody_next.as_ref().unwrap().label,
            "Page Down"
        );
        assert_eq!(
            loaded.voice_keys.monitor_toggle.as_ref().unwrap().label,
            "Alt+H"
        );
        assert_eq!(loaded.voice_keys.stop_sounds.as_ref().unwrap().vk, 0x6E);
        assert_eq!(
            loaded.voice_keys.panic.as_ref().unwrap().label,
            "Ctrl+Pause"
        );
        assert!(loaded.feedback.panic_mutes_monitor);

        // A config from before the monitor / stop keys: everything else is kept, the new keys
        // load unassigned.
        std::fs::write(
            &path,
            r#"{ "voice_keys": { "toggle": { "vk": 119, "ctrl": false, "shift": false,
                 "alt": false, "win": false, "label": "F8" }, "mode": "Toggle", "mute": null,
                 "presets": [], "melody_toggle": null } }"#,
        )
        .unwrap();
        let (before, warning) = load_from(&path);
        assert!(warning.is_none(), "{warning:?}");
        assert_eq!(before.voice_keys.toggle.as_ref().unwrap().label, "F8");
        assert!(before.voice_keys.monitor_toggle.is_none());
        assert!(before.voice_keys.monitor_sounds_toggle.is_none());
        assert!(before.voice_keys.stop_sounds.is_none());
        assert!(
            before.voice_keys.panic.is_none(),
            "the panic key starts unassigned"
        );

        // Older files: toggle mode, beeps on, overlay off, remember the last state.
        std::fs::write(&path, r#"{ "voice_on": false }"#).unwrap();
        let (old, _) = load_from(&path);
        assert_eq!(old.voice_keys, VoiceHotkeys::default());
        assert!(old.feedback.cues && !old.feedback.overlay);
        assert!(
            !old.feedback.panic_mutes_monitor,
            "panic keeps the headphones by default"
        );
        assert_eq!(old.start_voice, StartVoice::RememberLast);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn close_and_startup_settings_round_trip_and_default_safely() {
        let dir = temp_dir("config-close");
        let path = dir.join("config.json");
        for (behavior, minimized) in [
            (CloseBehavior::MinimizeToTray, true),
            (CloseBehavior::Quit, false),
            (CloseBehavior::Ask, true),
        ] {
            let cfg = Config {
                close_behavior: behavior,
                start_minimized: minimized,
                ..Config::default()
            };
            save_to(&path, &cfg).unwrap();
            let (loaded, warning) = load_from(&path);
            assert!(warning.is_none());
            assert_eq!(loaded.close_behavior, behavior);
            assert_eq!(loaded.start_minimized, minimized);
        }
        let json = std::fs::read_to_string(&path).unwrap();
        assert!(
            json.contains("\"close_behavior\": \"Ask\""),
            "readable in the file"
        );

        // Config files from before this feature: ask on close, start visible.
        std::fs::write(&path, r#"{ "voice_on": true }"#).unwrap();
        let (old, _) = load_from(&path);
        assert_eq!(old.close_behavior, CloseBehavior::Ask);
        assert!(!old.start_minimized);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn missing_partial_and_broken_files() {
        let dir = temp_dir("config-bad");
        let path = dir.join("config.json");
        assert_eq!(load_from(&path), (Config::default(), None));

        // Old or hand-edited files with missing fields load with defaults for the rest.
        std::fs::write(
            &path,
            r#"{ "voice_on": false, "mic": { "silence_secs": 3.0 } }"#,
        )
        .unwrap();
        let (cfg, warning) = load_from(&path);
        assert!(warning.is_none());
        assert!(!cfg.voice_on);
        assert_eq!(cfg.mic.silence_secs, 3.0);
        assert!(cfg.mic.switch_back, "missing field takes its default");

        std::fs::write(&path, "{ not json").unwrap();
        let (cfg, warning) = load_from(&path);
        assert_eq!(cfg, Config::default());
        assert!(warning.unwrap().contains("damaged"));
        assert!(dir.join("config.broken.json").exists());
        std::fs::remove_dir_all(dir).ok();
    }
}
