//! What the tray icon shows, derived from the audio status. Pure, so it is tested here.

use crate::audio::device_manager::DeviceStatus;
use crate::audio::shared::ActiveMic;
use crate::shell::{Health, TrayStatus};

pub fn compute(dev: &DeviceStatus, voice_on: bool, muted: bool) -> TrayStatus {
    let output_running = dev.output.device.is_some();
    let health = if !output_running {
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
    };
    TrayStatus {
        health,
        voice_on,
        muted,
        active: format!("Active: {mic}"),
        tooltip,
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
        let s = |out, act, voice, mute| compute(&status(out, act), voice, mute).health;
        assert_eq!(s(true, ActiveMic::Primary, true, false), Health::Ok);
        assert_eq!(s(true, ActiveMic::Primary, false, false), Health::VoiceOff);
        assert_eq!(s(true, ActiveMic::Primary, false, true), Health::Muted);
        assert_eq!(s(true, ActiveMic::None, true, true), Health::NoMic);
        assert_eq!(s(false, ActiveMic::Primary, true, false), Health::NoOutput);
    }

    #[test]
    fn tooltip_and_menu_line_name_the_active_mic() {
        let t = compute(&status(true, ActiveMic::Fallback), true, false);
        assert_eq!(t.tooltip, "VoxShift: running, Fallback mic active");
        assert_eq!(t.active, "Active: Fallback mic");
        let t = compute(&status(false, ActiveMic::None), true, false);
        assert!(t.tooltip.contains("output missing"));
        assert!(t.tooltip.len() < 128, "fits the tray tooltip");
    }
}
