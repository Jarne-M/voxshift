//! Which mic to use: the primary when it works, the fallback otherwise.
//! Pure logic driven by the device manager, so it can be tested without hardware.

use super::shared::ActiveMic;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MicMode {
    #[default]
    Auto,
    ForcePrimary,
    ForceFallback,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MicSettings {
    pub mode: MicMode,
    /// Go back to the primary as soon as it works again.
    pub switch_back: bool,
    /// Treat a primary that stays (almost) silent as unplugged.
    pub silence_detect: bool,
    pub silence_secs: f32,
    pub silence_db: f32,
}

impl Default for MicSettings {
    fn default() -> Self {
        Self {
            mode: MicMode::Auto,
            switch_back: true,
            silence_detect: true,
            silence_secs: 5.0,
            // A real mic's noise floor sits around -60..-75 dBFS; a detached boom mic delivers
            // (near) digital silence. -85 dB separates the two with margin.
            silence_db: -85.0,
        }
    }
}

/// How far above the threshold the level must rise before a "silent" mic counts as back.
pub const RECOVER_MARGIN_DB: f32 = 10.0;
/// How long it must stay there.
pub const RECOVER_SECS: f32 = 0.3;

/// Notices a device that is present but delivers silence (a detached headset boom mic).
#[derive(Clone, Debug, Default)]
pub struct SilenceDetector {
    below_for: f32,
    above_for: f32,
    silent: bool,
}

impl SilenceDetector {
    /// Feeds the current level; returns true while the mic counts as silent.
    pub fn update(&mut self, level_db: f32, dt: f32, s: &MicSettings) -> bool {
        if !s.silence_detect {
            *self = Self::default();
            return false;
        }
        if level_db < s.silence_db {
            self.below_for += dt;
            self.above_for = 0.0;
            if self.below_for >= s.silence_secs {
                self.silent = true;
            }
        } else {
            self.below_for = 0.0;
            if level_db >= s.silence_db + RECOVER_MARGIN_DB {
                self.above_for += dt;
                if self.above_for >= RECOVER_SECS {
                    self.silent = false;
                }
            } else {
                self.above_for = 0.0;
            }
        }
        self.silent
    }

    pub fn is_silent(&self) -> bool {
        self.silent
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Picks the mic to use. `primary_ok`/`fallback_ok`: stream running and (for the primary)
/// not silent. `current` is what is in use now, which matters when `switch_back` is off.
pub fn choose(
    mode: MicMode,
    switch_back: bool,
    primary_ok: bool,
    fallback_ok: bool,
    current: ActiveMic,
) -> ActiveMic {
    match mode {
        MicMode::ForcePrimary => {
            if primary_ok {
                ActiveMic::Primary
            } else {
                ActiveMic::None
            }
        }
        MicMode::ForceFallback => {
            if fallback_ok {
                ActiveMic::Fallback
            } else {
                ActiveMic::None
            }
        }
        MicMode::Auto => {
            let stay_on_fallback = !switch_back && current == ActiveMic::Fallback && fallback_ok;
            if primary_ok && !stay_on_fallback {
                ActiveMic::Primary
            } else if fallback_ok {
                ActiveMic::Fallback
            } else if primary_ok {
                ActiveMic::Primary
            } else {
                ActiveMic::None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ActiveMic::*;

    #[test]
    fn auto_prefers_primary_and_falls_back() {
        let auto = |p, f, cur| choose(MicMode::Auto, true, p, f, cur);
        assert_eq!(auto(true, true, None), Primary);
        assert_eq!(auto(false, true, Primary), Fallback);
        assert_eq!(auto(true, true, Fallback), Primary, "switches back");
        assert_eq!(auto(false, false, Fallback), None);
        assert_eq!(auto(true, false, None), Primary);
    }

    #[test]
    fn switch_back_off_stays_on_fallback_until_it_fails() {
        let auto = |p, f, cur| choose(MicMode::Auto, false, p, f, cur);
        assert_eq!(auto(true, true, Fallback), Fallback);
        assert_eq!(auto(true, false, Fallback), Primary);
        assert_eq!(auto(true, true, None), Primary, "fresh start uses the primary");
    }

    #[test]
    fn forced_modes_never_switch() {
        assert_eq!(choose(MicMode::ForcePrimary, true, false, true, Primary), None);
        assert_eq!(choose(MicMode::ForceFallback, true, true, true, Primary), Fallback);
        assert_eq!(choose(MicMode::ForceFallback, true, true, false, Fallback), None);
    }

    #[test]
    fn silence_detection_needs_the_full_hold_time_and_clear_signal_to_recover() {
        let s = MicSettings {
            silence_db: -70.0,
            ..MicSettings::default()
        }; // -70 dB for 5 s
        let mut d = SilenceDetector::default();
        let dt = 0.1;
        for _ in 0..48 {
            assert!(!d.update(-100.0, dt, &s));
        }
        let ticks = (0..5).take_while(|_| !d.update(-100.0, dt, &s)).count();
        assert!(ticks <= 2 && d.is_silent(), "silent after ~5 s");
        // A level just above the threshold is not enough to come back...
        for _ in 0..20 {
            assert!(d.update(-65.0, dt, &s));
        }
        // ...a clear signal for 0.3 s is.
        assert!(d.update(-40.0, dt, &s));
        assert!(d.update(-40.0, dt, &s));
        let ticks = (0..5).take_while(|_| d.update(-40.0, dt, &s)).count();
        assert!(ticks <= 1 && !d.is_silent());
    }

    #[test]
    fn short_pauses_do_not_trigger_and_disabled_never_triggers() {
        let s = MicSettings::default();
        let mut d = SilenceDetector::default();
        for i in 0..200 {
            let level = if i % 30 == 0 { -50.0 } else { -100.0 }; // speech every 3 s
            assert!(!d.update(level, 0.1, &s));
        }
        let off = MicSettings {
            silence_detect: false,
            ..s
        };
        for _ in 0..100 {
            assert!(!d.update(-120.0, 0.1, &off));
        }
    }
}
