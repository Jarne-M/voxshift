//! Windows shell integration: tray icon, single instance, start with Windows.
//!
//! On other systems `other.rs` stands in for the Win32 parts with versions that do nothing,
//! so the app (and above all its tests) also builds there.

#[cfg(windows)]
pub mod autostart;
pub mod icon;
#[cfg(windows)]
pub mod instance;
#[cfg(windows)]
pub mod overlay;
#[cfg(windows)]
pub mod tray;

#[cfg(not(windows))]
mod other;
#[cfg(not(windows))]
pub use other::{autostart, instance, tray};

/// Overall state, shown by the tray icon colour. Ordered from fine to worst.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Health {
    Ok,
    VoiceOff,
    Muted,
    NoMic,
    NoOutput,
    /// Panic / safe mode is on (shown above everything else).
    Safe,
}

/// Everything the tray shows.
#[derive(Clone, Debug, PartialEq)]
pub struct TrayStatus {
    pub health: Health,
    pub voice_on: bool,
    pub muted: bool,
    /// "Hear myself" on the monitor.
    pub monitor: bool,
    /// Panic / safe mode.
    pub safe: bool,
    /// Read-only menu line, e.g. "Active: Fallback mic".
    pub active: String,
    pub tooltip: String,
    /// Your favourite presets, offered as quick picks in the menu (in the preset bar's order).
    pub presets: Vec<String>,
    /// The preset last loaded (its quick pick is ticked).
    pub preset: Option<String>,
    /// The accent colour: the icon's colour when all is well.
    pub accent: [u8; 3],
}

/// What the tray (or a second launch of the app) asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayAction {
    Show,
    ToggleVoice,
    ToggleMute,
    ToggleMonitor,
    StopAllSounds,
    /// Panic (safe mode) on, or restore.
    TogglePanic,
    /// Load this preset and switch the voice changer on, like a preset hotkey.
    Preset(String),
    Quit,
    /// Windows is shutting down or logging off: save now, the process is about to end.
    SessionEnd,
}

/// Class name of the tray's hidden window; a second instance finds the first one by it.
pub const WINDOW_CLASS: &str = "VoxShift.TrayWindow";

/// The area all monitors together cover (x, y, width, height), in the pixels this process
/// sees. Used to check a remembered window place before opening the window there.
#[cfg(windows)]
pub fn virtual_screen() -> Option<[f32; 4]> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };
    // SAFETY: GetSystemMetrics only reads system values.
    let m = |i| unsafe { GetSystemMetrics(i) } as f32;
    let screen = [
        m(SM_XVIRTUALSCREEN),
        m(SM_YVIRTUALSCREEN),
        m(SM_CXVIRTUALSCREEN),
        m(SM_CYVIRTUALSCREEN),
    ];
    (screen[2] > 0.0 && screen[3] > 0.0).then_some(screen)
}

/// Not known here, so a remembered window place is not checked.
#[cfg(not(windows))]
pub fn virtual_screen() -> Option<[f32; 4]> {
    None
}

/// Most quick picks the tray menu shows (a longer menu stops being quick).
pub const MAX_TRAY_PRESETS: usize = 10;

/// The tray menu's preset quick picks: the label (with `&` doubled, since a single one marks
/// a keyboard shortcut in a Win32 menu) and whether it is the preset in use.
pub fn preset_items(s: &TrayStatus) -> Vec<(String, bool)> {
    s.presets
        .iter()
        .take(MAX_TRAY_PRESETS)
        .map(|name| (name.replace('&', "&&"), s.preset.as_deref() == Some(name)))
        .collect()
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Copies `s` into a fixed UTF-16 buffer, truncated and NUL-terminated.
pub fn copy_wide(dst: &mut [u16], s: &str) {
    let mut n = 0;
    let room = dst.len().saturating_sub(1);
    for (d, c) in dst.iter_mut().zip(s.encode_utf16()).take(room) {
        *d = c;
        n += 1;
    }
    dst[n] = 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_wide_truncates_and_terminates() {
        let mut buf = [1u16; 5];
        copy_wide(&mut buf, "VoxShift");
        assert_eq!(String::from_utf16_lossy(&buf[..4]), "VoxS");
        assert_eq!(buf[4], 0);
        copy_wide(&mut buf, "ab");
        assert_eq!(&buf[..3], &[97, 98, 0]);
    }

    #[test]
    fn preset_quick_picks_tick_the_preset_in_use_and_escape_ampersands() {
        let mut s = TrayStatus {
            health: Health::Ok,
            voice_on: true,
            muted: false,
            monitor: false,
            safe: false,
            active: String::new(),
            tooltip: String::new(),
            presets: vec!["Late night".into(), "Rock & roll".into()],
            preset: Some("Rock & roll".into()),
            accent: icon::DEFAULT_ACCENT,
        };
        assert_eq!(
            preset_items(&s),
            vec![
                ("Late night".to_string(), false),
                ("Rock && roll".to_string(), true)
            ]
        );
        s.preset = Some("Cave".into());
        assert!(preset_items(&s).iter().all(|(_, on)| !on));
        s.presets = (0..15).map(|i| format!("P{i}")).collect();
        assert_eq!(preset_items(&s).len(), MAX_TRAY_PRESETS);
        s.presets.clear();
        assert!(preset_items(&s).is_empty(), "no favourites, no section");
    }

    #[test]
    fn health_orders_by_severity() {
        assert!(Health::NoOutput > Health::NoMic && Health::NoMic > Health::Ok);
    }
}
