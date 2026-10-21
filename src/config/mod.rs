//! Everything the app remembers between runs, stored as JSON in
//! `%APPDATA%\VoxShift\config.json`.

pub mod presets;

use crate::audio::device_manager::DeviceSettings;
use crate::audio::mic_select::MicSettings;
use crate::dsp::params::EffectSettings;
use crate::soundboard::library::SoundEntry;
use presets::Preset;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
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
}

impl Default for Config {
    fn default() -> Self {
        Self {
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
        Err(e) => return (Config::default(), Some(format!("Could not read settings: {e}"))),
    };
    match serde_json::from_str(&text) {
        Ok(cfg) => (cfg, None),
        Err(e) => {
            let backup = path.with_file_name("config.broken.json");
            let _ = std::fs::rename(path, &backup);
            (
                Config::default(),
                Some(format!("Settings file was damaged ({e}); started fresh, old file kept as {}", backup.display())),
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
    use crate::soundboard::hotkeys::Hotkey;
    use crate::soundboard::decode::tests::temp_dir;

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
        cfg.user_presets.push(Preset {
            name: "Mine".into(),
            effects: cfg.effects.clone(),
        });
        cfg.sounds.push(SoundEntry {
            name: "airhorn".into(),
            path: "C:/sounds/airhorn.mp3".into(),
            volume: 0.5,
            hotkey: Some(Hotkey::new(0x61, true, false, false, false, "Ctrl+Num 1")),
        });
        save_to(&path, &cfg).unwrap();
        let (loaded, warning) = load_from(&path);
        assert!(warning.is_none());
        assert_eq!(loaded, cfg);
        assert_eq!(loaded.sounds[0].hotkey.as_ref().unwrap().label, "Ctrl+Num 1");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn missing_partial_and_broken_files() {
        let dir = temp_dir("config-bad");
        let path = dir.join("config.json");
        assert_eq!(load_from(&path), (Config::default(), None));

        // Old or hand-edited files with missing fields load with defaults for the rest.
        std::fs::write(&path, r#"{ "voice_on": false, "mic": { "silence_secs": 3.0 } }"#).unwrap();
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
