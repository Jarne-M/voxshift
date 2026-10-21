//! Global hotkeys that work while another app (a game) has focus.
//!
//! A low-level keyboard hook (WH_KEYBOARD_LL) sees every key press system-wide. It always
//! passes the key on (CallNextHookEx), so the game still receives it too. The hook procedure
//! only forwards the key to a worker thread; Windows silently removes hooks that take too long.

use super::library::Board;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{Arc, Mutex, mpsc};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hotkey {
    /// Windows virtual-key code.
    pub vk: u32,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub win: bool,
    /// Human-readable, e.g. "Ctrl+Num 1" (captured once, when the key is assigned).
    pub label: String,
}

impl PartialEq for Hotkey {
    fn eq(&self, other: &Self) -> bool {
        self.same_keys(other)
    }
}

impl Hotkey {
    pub fn new(vk: u32, ctrl: bool, shift: bool, alt: bool, win: bool, key_name: &str) -> Self {
        Self {
            vk,
            ctrl,
            shift,
            alt,
            win,
            label: key_name.to_string(),
        }
    }

    pub fn same_keys(&self, o: &Hotkey) -> bool {
        (self.vk, self.ctrl, self.shift, self.alt, self.win)
            == (o.vk, o.ctrl, o.shift, o.alt, o.win)
    }
}

/// One raw key event from the hook.
#[derive(Clone, Copy, Debug)]
pub struct RawKey {
    pub vk: u32,
    pub scan: u32,
    pub extended: bool,
    pub down: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub win: bool,
}

pub const VK_ESCAPE: u32 = 0x1B;

pub fn is_modifier(vk: u32) -> bool {
    matches!(vk, 0x10..=0x12 | 0xA0..=0xA5 | 0x5B | 0x5C)
}

/// Turns raw key events into "combination pressed" events: ignores modifiers on their own and
/// the auto-repeat of a held key.
#[derive(Default)]
pub struct KeyTracker {
    held: HashSet<u32>,
}

impl KeyTracker {
    pub fn feed(&mut self, k: RawKey, name: impl FnOnce(&RawKey) -> String) -> Option<Hotkey> {
        if !k.down {
            self.held.remove(&k.vk);
            return None;
        }
        if is_modifier(k.vk) || !self.held.insert(k.vk) {
            return None;
        }
        let mut label = String::new();
        for (on, text) in [
            (k.ctrl, "Ctrl+"),
            (k.shift, "Shift+"),
            (k.alt, "Alt+"),
            (k.win, "Win+"),
        ] {
            if on {
                label.push_str(text);
            }
        }
        label.push_str(&name(&k));
        Some(Hotkey::new(k.vk, k.ctrl, k.shift, k.alt, k.win, &label))
    }
}

/// What the GUI hears back while assigning a key.
pub enum Captured {
    Key(Hotkey),
    Cancelled,
}

pub struct HotkeyService {
    capture: Arc<Mutex<bool>>,
    captured: mpsc::Receiver<Captured>,
    #[cfg(windows)]
    hook: Option<win::HookThread>,
    worker: Option<std::thread::JoinHandle<()>>,
    available: bool,
}

impl HotkeyService {
    /// Installs the hook. Key presses play matching sounds of `board` directly from the worker
    /// thread, so hotkeys work even while the window is minimised and not repainting.
    pub fn start(board: Arc<Mutex<Board>>) -> Self {
        let (raw_tx, raw_rx) = mpsc::channel::<RawKey>();
        let (cap_tx, cap_rx) = mpsc::channel();
        let capture = Arc::new(Mutex::new(false));
        #[cfg(windows)]
        let hook = win::HookThread::start(raw_tx);
        #[cfg(windows)]
        let available = hook.is_some();
        #[cfg(not(windows))]
        let available = {
            drop(raw_tx);
            false
        };
        let cap = capture.clone();
        let worker = std::thread::Builder::new()
            .name("hotkeys".into())
            .spawn(move || {
                let mut tracker = KeyTracker::default();
                while let Ok(raw) = raw_rx.recv() {
                    #[cfg(windows)]
                    let pressed = tracker.feed(raw, win::key_name);
                    #[cfg(not(windows))]
                    let pressed = tracker.feed(raw, |k| format!("Key {}", k.vk));
                    let Some(key) = pressed else { continue };
                    let mut capturing = cap.lock().unwrap_or_else(|e| e.into_inner());
                    if *capturing {
                        *capturing = false;
                        let msg = if key.vk == VK_ESCAPE
                            && !(key.ctrl || key.shift || key.alt || key.win)
                        {
                            Captured::Cancelled
                        } else {
                            Captured::Key(key)
                        };
                        let _ = cap_tx.send(msg);
                        continue;
                    }
                    drop(capturing);
                    board
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .hotkey_pressed(&key);
                }
            })
            .ok();
        Self {
            capture,
            captured: cap_rx,
            #[cfg(windows)]
            hook,
            worker,
            available,
        }
    }

    pub fn available(&self) -> bool {
        self.available
    }

    /// The next key combination pressed (anywhere) is reported instead of triggering sounds.
    pub fn begin_capture(&self) {
        while self.captured.try_recv().is_ok() {}
        *self.capture.lock().unwrap_or_else(|e| e.into_inner()) = true;
    }

    pub fn cancel_capture(&self) {
        *self.capture.lock().unwrap_or_else(|e| e.into_inner()) = false;
    }

    pub fn poll_capture(&self) -> Option<Captured> {
        self.captured.try_recv().ok()
    }
}

impl Drop for HotkeyService {
    fn drop(&mut self) {
        #[cfg(windows)]
        drop(self.hook.take()); // unhooks and drops the raw sender, ending the worker
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

#[cfg(windows)]
mod win {
    use super::RawKey;
    use std::sync::{Mutex, mpsc};
    use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, GetKeyNameTextW};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetMessageW, KBDLLHOOKSTRUCT, LLKHF_EXTENDED, MSG, PM_NOREMOVE,
        PeekMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL,
        WM_KEYDOWN, WM_QUIT, WM_SYSKEYDOWN, WM_USER,
    };

    /// The hook procedure has no user data pointer, so its sender lives in a static.
    static SENDER: Mutex<Option<mpsc::Sender<RawKey>>> = Mutex::new(None);

    fn held(vk: i32) -> bool {
        // High bit set = key is down.
        (unsafe { GetAsyncKeyState(vk) } as u16 & 0x8000) != 0
    }

    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            // SAFETY: for WH_KEYBOARD_LL, lparam points to a KBDLLHOOKSTRUCT.
            let kb = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
            let msg = wparam as u32;
            let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
            let key = RawKey {
                vk: kb.vkCode,
                scan: kb.scanCode,
                extended: kb.flags & LLKHF_EXTENDED != 0,
                down,
                ctrl: held(0x11),
                shift: held(0x10),
                alt: held(0x12),
                win: held(0x5B) || held(0x5C),
            };
            if let Ok(guard) = SENDER.try_lock()
                && let Some(tx) = guard.as_ref()
            {
                let _ = tx.send(key);
            }
        }
        // Always pass the key on: the focused app (game) still gets it.
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
    }

    pub fn key_name(k: &RawKey) -> String {
        let lparam = ((k.scan as i32) << 16) | if k.extended { 1 << 24 } else { 0 };
        let mut buf = [0u16; 64];
        let n = unsafe { GetKeyNameTextW(lparam, buf.as_mut_ptr(), buf.len() as i32) };
        if n > 0 {
            String::from_utf16_lossy(&buf[..n as usize])
        } else {
            format!("Key 0x{:02X}", k.vk)
        }
    }

    pub struct HookThread {
        thread_id: u32,
        handle: Option<std::thread::JoinHandle<()>>,
    }

    impl HookThread {
        pub fn start(tx: mpsc::Sender<RawKey>) -> Option<Self> {
            *SENDER.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
            let (id_tx, id_rx) = mpsc::channel();
            let handle = std::thread::Builder::new()
                .name("keyboard-hook".into())
                .spawn(move || unsafe {
                    let module = GetModuleHandleW(std::ptr::null());
                    let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), module, 0);
                    if hook.is_null() {
                        let _ = id_tx.send(None);
                        return;
                    }
                    // Make sure this thread has a message queue before anyone posts to it.
                    let mut msg: MSG = std::mem::zeroed();
                    PeekMessageW(
                        &mut msg,
                        std::ptr::null_mut(),
                        WM_USER,
                        WM_USER,
                        PM_NOREMOVE,
                    );
                    let _ = id_tx.send(Some(GetCurrentThreadId()));
                    // The hook procedure runs inside this message loop.
                    while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {}
                    UnhookWindowsHookEx(hook);
                })
                .ok()?;
            match id_rx.recv() {
                Ok(Some(thread_id)) => Some(Self {
                    thread_id,
                    handle: Some(handle),
                }),
                _ => {
                    let _ = handle.join();
                    *SENDER.lock().unwrap_or_else(|e| e.into_inner()) = None;
                    None
                }
            }
        }
    }

    impl Drop for HookThread {
        fn drop(&mut self) {
            unsafe {
                PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
            }
            if let Some(h) = self.handle.take() {
                let _ = h.join();
            }
            *SENDER.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(vk: u32, down: bool, ctrl: bool) -> RawKey {
        RawKey {
            vk,
            scan: 0,
            extended: false,
            down,
            ctrl,
            shift: false,
            alt: false,
            win: false,
        }
    }

    #[test]
    fn tracker_reports_each_press_once_and_ignores_bare_modifiers() {
        let mut t = KeyTracker::default();
        let name = |_: &RawKey| "F5".to_string();
        assert!(t.feed(key(0x11, true, true), name).is_none(), "Ctrl alone");
        let hk = t.feed(key(0x74, true, true), name).unwrap();
        assert_eq!(hk.label, "Ctrl+F5");
        assert!(hk.ctrl && hk.vk == 0x74);
        assert!(t.feed(key(0x74, true, true), name).is_none(), "auto-repeat");
        t.feed(key(0x74, false, true), name);
        assert!(
            t.feed(key(0x74, true, false), name).is_some(),
            "pressed again"
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_global_hook_installs_and_shuts_down() {
        use crate::audio::device_manager::EnginePort;
        use crate::soundboard::library::Soundboard;
        let sb = Soundboard::new(Arc::new(EnginePort::default()), vec![]);
        let service = HotkeyService::start(sb.board.clone());
        assert!(service.available(), "SetWindowsHookExW failed");
        service.begin_capture();
        assert!(service.poll_capture().is_none());
        drop(service); // must not hang
    }

    #[test]
    fn hotkey_equality_ignores_the_label() {
        let a = Hotkey::new(0x61, true, false, false, false, "Ctrl+Num 1");
        let b = Hotkey::new(0x61, true, false, false, false, "whatever");
        assert_eq!(a, b);
        assert_ne!(a, Hotkey::new(0x61, false, false, false, false, "Num 1"));
    }
}
