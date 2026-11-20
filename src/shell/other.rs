//! Stand-ins for the Win32 shell parts on other systems. They do nothing, but keep the same
//! names and signatures, so the rest of the app builds unchanged. This is mainly so the tests
//! can run on Linux; VoxShift itself is still a Windows app.

/// Start with Windows: there is no Run key here, so it is always off.
pub mod autostart {
    pub fn is_enabled() -> bool {
        false
    }

    pub fn points_elsewhere() -> bool {
        false
    }

    pub fn set_enabled(_on: bool) -> Result<(), String> {
        Err("Starting with the system is only available on Windows.".into())
    }
}

/// Single instance: every launch counts as the first one.
pub mod instance {
    pub const MUTEX_NAME: &str = "VoxShift.SingleInstance";

    /// Held for the lifetime of the first instance.
    pub struct Guard;

    pub enum Instance {
        First(Guard),
        #[allow(dead_code)]
        AlreadyRunning,
    }

    pub fn acquire(_name: &str) -> Instance {
        Instance::First(Guard)
    }

    pub fn signal_existing() -> bool {
        false
    }
}

/// The tray icon: never starts, so the app behaves as if the tray could not be created.
pub mod tray {
    use crate::shell::{TrayAction, TrayStatus};

    /// Posts to the tray thread from anywhere (here: does nothing).
    #[derive(Clone, Copy, Debug)]
    pub struct TrayPoster;

    impl TrayPoster {
        pub fn refresh(self) {}

        pub fn overlay(self, _on: bool) {}
    }

    pub struct Tray;

    impl Tray {
        pub fn start(
            _status: impl Fn() -> TrayStatus + Send + 'static,
            _action: impl Fn(TrayAction) + Send + 'static,
        ) -> Result<Self, String> {
            Err("The tray icon is only available on Windows.".into())
        }

        pub fn notify(&self, _title: &str, _text: &str) {}

        pub fn refresh(&self) {}

        pub fn poster(&self) -> TrayPoster {
            TrayPoster
        }

        pub fn shutdown(&mut self) {}
    }
}
