//! Only one VoxShift at a time: two copies would fight over the virtual cable.
//! The first one holds a named mutex; a second launch finds the first one's tray window and
//! asks it to show itself, then exits.

use super::{WINDOW_CLASS, wide};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    ASFW_ANY, AllowSetForegroundWindow, FindWindowW, PostMessageW,
};

/// Per-session name: each logged-in user gets their own instance.
pub const MUTEX_NAME: &str = "Local\\VoxShift.SingleInstance";

/// Message the tray window understands as "show the main window".
pub const WM_SHOW_APP: u32 = windows_sys::Win32::UI::WindowsAndMessaging::WM_APP + 2;

/// Held for the lifetime of the first instance.
pub struct Guard(HANDLE);

// The handle is only closed on drop; it is never used concurrently.
unsafe impl Send for Guard {}

impl Drop for Guard {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

pub enum Instance {
    First(Guard),
    AlreadyRunning,
}

pub fn acquire(name: &str) -> Instance {
    let w = wide(name);
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, w.as_ptr()) };
    let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    if handle.is_null() {
        // Could not create it at all: better to run than to refuse to start.
        return Instance::First(Guard(handle));
    }
    if existed {
        unsafe { CloseHandle(handle) };
        Instance::AlreadyRunning
    } else {
        Instance::First(Guard(handle))
    }
}

/// Asks the running instance to show its window. Waits up to 5 s in case it is still starting.
pub fn signal_existing() -> bool {
    let class = wide(WINDOW_CLASS);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let hwnd = unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) };
        if !hwnd.is_null() {
            unsafe {
                // We were just started by the user, so we may hand the foreground on.
                AllowSetForegroundWindow(ASFW_ANY);
                return PostMessageW(hwnd, WM_SHOW_APP, 0, 0) != 0;
            }
        }
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_acquire_of_the_same_name_sees_the_first() {
        let name = format!("Local\\VoxShift.Test.{}", std::process::id());
        let first = acquire(&name);
        assert!(matches!(first, Instance::First(_)));
        assert!(matches!(acquire(&name), Instance::AlreadyRunning));
        drop(first);
        assert!(
            matches!(acquire(&name), Instance::First(_)),
            "released on drop"
        );
    }
}
