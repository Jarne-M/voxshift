//! What the tray icon shows, derived from the audio status. Pure, so it is tested here.

use crate::audio::device_manager::DeviceStatus;
use crate::audio::shared::ActiveMic;
use crate::config::Config;
use crate::shell::{Health, TrayStatus};

/// The tray's preset quick picks: your favourite presets in the preset bar's order, and the
/// preset last loaded.
pub fn presets(cfg: &Config) -> (Vec<String>, Option<String>) {
    let favorites = cfg
        .user_presets
        .iter()
        .filter(|p| p.favorite)
        .map(|p| p.name.clone())
        .collect();
    (favorites, cfg.preset.clone())
}

/// The accent colour from the config, for the "all good" icon.
pub fn accent(cfg: &Config) -> [u8; 3] {
    let c = crate::ui::theme::accent_of(&cfg.accent);
    [c.r(), c.g(), c.b()]
}

pub fn compute(
    dev: &DeviceStatus,
    voice_on: bool,
    muted: bool,
    monitor: bool,
    safe: bool,
) -> TrayStatus {
    let output_running = dev.output.device.is_some();
    // Safe mode is something the user switched on, so it is shown first: it explains why the
    // voice and the sounds are off.
    let health = if safe {
        Health::Safe
    } else if !output_running {
        Health::NoOutput
    } else if dev.active == ActiveMic::None {
        Health::NoMic
    } else if muted {
        Health::Muted
    } else if !voice_on {
        Health::VoiceOff
    } else {
        Health::Ok
    };
    let mic = match dev.active {
        ActiveMic::Primary => "Primary mic",
        ActiveMic::Fallback => "Fallback mic",
        ActiveMic::None => "no mic",
    };
    let tooltip = match health {
        Health::NoOutput => "VoxShift: virtual mic output missing!".to_string(),
        Health::NoMic => "VoxShift: no microphone available!".to_string(),
        Health::Muted => format!("VoxShift: mic muted ({mic})"),
        Health::VoiceOff => format!("VoxShift: running, voice changer off, {mic}"),
        Health::Ok => format!("VoxShift: running, {mic} active"),
        Health::Safe => "VoxShift: SAFE MODE (panic) - clean mic, no sounds".to_string(),
    };
    TrayStatus {
        health,
        voice_on,
        muted,
        monitor,
        safe,
        active: format!("Active: {mic}"),
        tooltip,
        presets: Vec::new(),
        preset: None,
        accent: crate::shell::icon::DEFAULT_ACCENT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::device_manager::Endpoint;

    fn status(output: bool, active: ActiveMic) -> DeviceStatus {
        DeviceStatus {
            output: Endpoint {
                device: output.then(|| "CABLE Input (VB-Audio Virtual Cable)".to_string()),
                ..Default::default()
            },
            active,
            ..Default::default()
        }
    }

    #[test]
    fn the_worst_problem_wins() {
        let s = |out, act, voice, mute| compute(&status(out, act), voice, mute, true, false).health;
        assert_eq!(s(true, ActiveMic::Primary, true, false), Health::Ok);
        assert_eq!(s(true, ActiveMic::Primary, false, false), Health::VoiceOff);
        assert_eq!(s(true, ActiveMic::Primary, false, true), Health::Muted);
        assert_eq!(s(true, ActiveMic::None, true, true), Health::NoMic);
        assert_eq!(s(false, ActiveMic::Primary, true, false), Health::NoOutput);
    }

    #[test]
    fn tooltip_and_menu_line_name_the_active_mic() {
        let t = compute(&status(true, ActiveMic::Fallback), true, false, true, false);
        assert_eq!(t.tooltip, "VoxShift: running, Fallback mic active");
        assert_eq!(t.active, "Active: Fallback mic");
        let t = compute(&status(false, ActiveMic::None), true, false, false, false);
        assert!(t.tooltip.contains("output missing"));
        assert!(t.tooltip.len() < 128, "fits the tray tooltip");
    }

    #[test]
    fn quick_picks_are_the_favourites_in_order() {
        use crate::config::presets::Preset;
        let mut cfg = Config::default();
        for (name, favorite) in [("A", true), ("B", false), ("C", true)] {
            cfg.user_presets.push(Preset {
                name: name.into(),
                effects: Default::default(),
                melody: None,
                favorite,
            });
        }
        cfg.preset = Some("C".into());
        assert_eq!(
            presets(&cfg),
            (
                vec!["A".to_string(), "C".to_string()],
                Some("C".to_string())
            )
        );
    }

    #[test]
    fn the_icon_follows_the_accent_and_a_broken_value_gives_teal() {
        let mut cfg = Config::default();
        assert_eq!(accent(&cfg), crate::shell::icon::DEFAULT_ACCENT);
        cfg.accent = "#3B82F6".into();
        assert_eq!(accent(&cfg), [0x3B, 0x82, 0xF6]);
        cfg.accent = "purple".into();
        assert_eq!(accent(&cfg), crate::shell::icon::DEFAULT_ACCENT);
    }

    #[test]
    fn safe_mode_is_shown_above_everything() {
        for (out, act) in [(true, ActiveMic::Primary), (false, ActiveMic::None)] {
            let t = compute(&status(out, act), true, true, true, true);
            assert_eq!(t.health, Health::Safe);
            assert!(t.safe && t.tooltip.contains("SAFE MODE"), "{}", t.tooltip);
            assert!(t.tooltip.len() < 128);
        }
    }
}
