//! Everything the app remembers between runs.

use crate::audio::device_manager::DeviceSettings;
use crate::audio::mic_select::MicSettings;
use crate::dsp::params::EffectSettings;
use serde::{Deserialize, Serialize};

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
        }
    }
}
