//! Windows shell integration: tray icon, single instance, start with Windows.

pub mod autostart;
pub mod icon;
pub mod instance;
pub mod overlay;
pub mod tray;

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
}

/// What the tray (or a second launch of the app) asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    Show,
    ToggleVoice,
    ToggleMute,
    ToggleMonitor,
    StopAllSounds,
    /// Panic (safe mode) on, or restore.
    TogglePanic,
    Quit,
    /// Windows is shutting down or logging off: save now, the process is about to end.
    SessionEnd,
}

/// Class name of the tray's hidden window; a second instance finds the first one by it.
pub const WINDOW_CLASS: &str = "VoxShift.TrayWindow";

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
    fn health_orders_by_severity() {
        assert!(Health::NoOutput > Health::NoMic && Health::NoMic > Health::Ok);
    }
}
