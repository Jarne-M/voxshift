//! Global hotkeys that work while another app (a game) has focus.
//!
//! Key presses arrive through Windows' Raw Input API on a hidden window registered with
//! RIDEV_INPUTSINK, so they are seen even while another app is focused, and the focused app
//! still gets every key (we only listen). An earlier version used a low-level keyboard hook,
//! which Windows silently removes if it ever answers too slowly; raw input has no such limit.
//! A worker thread turns the raw events into "combination pressed" and plays matching sounds.

use super::library::Board;
use crate::keynames;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};

/// Troubleshooting log for assigning hotkeys, written next to the config. It only records
/// what happens while VoxShift is waiting for a hotkey; other key presses are never logged.
pub mod diag {
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::OnceLock;
    use std::time::Instant;

    static PATH: OnceLock<PathBuf> = OnceLock::new();
    static START: OnceLock<Instant> = OnceLock::new();
    const MAX_BYTES: u64 = 64 * 1024;

    pub fn set_path(path: PathBuf) {
        let _ = START.set(Instant::now());
        let _ = PATH.set(path);
        log("--- VoxShift started ---");
    }

    pub fn log(msg: impl AsRef<str>) {
        let Some(path) = PATH.get() else { return };
        let t = START.get().map_or(0.0, |s| s.elapsed().as_secs_f64());
        let too_big = std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_BYTES);
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(!too_big)
            .write(true)
            .truncate(too_big)
            .open(path);
        if let Ok(mut f) = file {
            let _ = writeln!(f, "{t:9.3}s  {}", msg.as_ref());
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hotkey {
    /// Windows virtual-key code.
    pub vk: u32,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub win: bool,
    /// The name saved when the key was assigned, e.g. "Ctrl + Num 1". Older versions saved
    /// Windows' own (translated) key name here; for showing a hotkey use [`Hotkey::pretty`],
    /// which only falls back to this for keys it cannot name itself.
    pub label: String,
    /// A second combination that does the same, e.g. "8" next to "Ctrl + 8" (so the action
    /// still works while Ctrl is held for something else in a game). Only set on an assigned
    /// key, never on a pressed one, and never nested further.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second: Option<Box<Hotkey>>,
}

impl PartialEq for Hotkey {
    fn eq(&self, other: &Self) -> bool {
        self.same_keys(other) && self.second == other.second
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
            second: None,
        }
    }

    /// The same combination as `o` (only the main keys: neither's second key counts).
    pub fn same_keys(&self, o: &Hotkey) -> bool {
        (self.vk, self.ctrl, self.shift, self.alt, self.win)
            == (o.vk, o.ctrl, o.shift, o.alt, o.win)
    }

    /// This assigned key and its second key, if it has one.
    pub fn keys(&self) -> impl Iterator<Item = &Hotkey> + Clone {
        std::iter::once(self).chain(self.second.as_deref())
    }

    /// Whether the combination `pressed` triggers this assigned key (its main or second key).
    pub fn matches(&self, pressed: &Hotkey) -> bool {
        self.keys().any(|k| k.same_keys(pressed))
    }

    /// The main key alone, without a second key.
    pub fn single(&self) -> Hotkey {
        Hotkey {
            second: None,
            ..self.clone()
        }
    }

    /// Builds an assigned key from a main and a second key (either may be missing). A second
    /// key without a main one becomes the main key; a second key equal to the main one is
    /// dropped.
    pub fn pair(first: Option<Hotkey>, second: Option<Hotkey>) -> Option<Hotkey> {
        let (first, second) = match (first, second) {
            (None, s) => (s, None),
            (f, s) => (f, s),
        };
        first.map(|f| {
            let mut f = f.single();
            f.second = second
                .map(|s| s.single())
                .filter(|s| !s.same_keys(&f))
                .map(Box::new);
            f
        })
    }

    /// How the key and its second key are shown in text: "Ctrl + 8 or 8".
    pub fn pretty_all(&self) -> String {
        self.keys()
            .map(Hotkey::pretty)
            .collect::<Vec<_>>()
            .join(" or ")
    }

    /// The combination as its keys, modifiers first: `["Ctrl", "Shift", "F8"]`. The key is
    /// named from its code (see [`crate::keynames`]), so a label saved by a Windows in another
    /// language ("7 (PAVE NUM.)") still shows as "Num 7"; only a key the table does not know
    /// keeps its saved name.
    pub fn parts(&self) -> Vec<String> {
        let key = keynames::key_name(self.vk).unwrap_or_else(|| {
            let saved = keynames::strip_modifiers(&self.label);
            if saved.is_empty() {
                format!("Key 0x{:02X}", self.vk)
            } else {
                saved.to_string()
            }
        });
        keynames::parts(self.ctrl, self.shift, self.alt, self.win, key)
    }

    /// How the combination is shown everywhere: "Ctrl + Shift + F8".
    pub fn pretty(&self) -> String {
        self.parts().join(keynames::JOIN)
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

/// What a key did, after filtering.
#[derive(Clone, Debug, PartialEq)]
pub enum KeyEvent {
    /// A key combination went down (once: auto-repeat is ignored).
    Pressed(Hotkey),
    /// The (non-modifier) key with this code went up.
    Released(u32),
}

/// Turns raw key events into "combination pressed" / "key released" events: ignores modifiers
/// on their own and the auto-repeat of a held key.
#[derive(Default)]
pub struct KeyTracker {
    held: HashSet<u32>,
}

impl KeyTracker {
    #[cfg(test)]
    pub fn feed(&mut self, k: RawKey, name: impl FnOnce(&RawKey) -> String) -> Option<Hotkey> {
        match self.feed_event(k, name) {
            Some(KeyEvent::Pressed(h)) => Some(h),
            _ => None,
        }
    }

    pub fn feed_event(
        &mut self,
        k: RawKey,
        name: impl FnOnce(&RawKey) -> String,
    ) -> Option<KeyEvent> {
        if !k.down {
            let was_held = self.held.remove(&k.vk);
            return (was_held && !is_modifier(k.vk)).then_some(KeyEvent::Released(k.vk));
        }
        if is_modifier(k.vk) || !self.held.insert(k.vk) {
            return None;
        }
        // `name` (what Windows calls the key) is only kept for keys our own table lacks.
        let mut key = Hotkey::new(k.vk, k.ctrl, k.shift, k.alt, k.win, &name(&k));
        key.label = key.pretty();
        Some(KeyEvent::Pressed(key))
    }

    /// Forgets that `vk` is down (its key-up was missed), so its next press counts.
    pub fn forget(&mut self, vk: u32) {
        self.held.remove(&vk);
    }

    /// True when no (non-modifier) key is down.
    pub fn none_held(&self) -> bool {
        self.held.is_empty()
    }
}

/// Something besides the soundboard that reacts to hotkeys (the voice changer controls).
/// Called on the hotkey worker thread.
pub trait KeyHandler: Send + Sync {
    /// Returns true if the key belonged to it (then no sound plays).
    fn pressed(&self, key: &Hotkey) -> bool;
    fn released(&self, vk: u32);
}

/// What the GUI hears back while assigning a key.
pub enum Captured {
    Key(Hotkey),
    Cancelled,
}

/// How often keys holding a clip are checked against the real keyboard.
const HELD_CHECK: std::time::Duration = std::time::Duration::from_millis(50);

/// Stops held clips whose key the keyboard says is up (its key-up was missed).
fn check_held(board: &Mutex<Board>, tracker: &mut KeyTracker) {
    let keys = board.lock().unwrap_or_else(|e| e.into_inner()).held_keys();
    if keys.is_empty() {
        return;
    }
    #[cfg(windows)]
    let missed = super::playmode::missed_releases(&keys, win::key_down);
    #[cfg(not(windows))]
    let missed: Vec<u32> = Vec::new();
    if missed.is_empty() {
        return;
    }
    let mut b = board.lock().unwrap_or_else(|e| e.into_inner());
    for vk in missed {
        tracker.forget(vk); // so its next press counts as a press again
        b.release(vk);
    }
}

pub struct HotkeyService {
    capture: Arc<Mutex<bool>>,
    captured: mpsc::Receiver<Captured>,
    #[cfg(windows)]
    listener: Option<win::Listener>,
    worker: Option<std::thread::JoinHandle<()>>,
    available: bool,
    /// Key presses received so far (for noticing a listener that hears nothing).
    presses: Arc<AtomicU64>,
    /// While set, key presses trigger nothing (a dialog about hotkeys is open).
    paused: Arc<AtomicBool>,
}

impl HotkeyService {
    /// Starts listening. Key presses go to `handler` first (voice changer controls), then play
    /// matching sounds of `board`, directly from the worker thread, so hotkeys work even while
    /// the window is hidden and not repainting.
    pub fn start(board: Arc<Mutex<Board>>, handler: Option<Arc<dyn KeyHandler>>) -> Self {
        let (raw_tx, raw_rx) = mpsc::channel::<RawKey>();
        let (cap_tx, cap_rx) = mpsc::channel();
        let capture = Arc::new(Mutex::new(false));
        let presses = Arc::new(AtomicU64::new(0));
        #[cfg(windows)]
        let listener = win::Listener::start(raw_tx);
        #[cfg(windows)]
        let available = listener.is_some();
        #[cfg(not(windows))]
        let available = {
            drop(raw_tx);
            false
        };
        diag::log(format!("listener started: {available}"));
        let paused = Arc::new(AtomicBool::new(false));
        let hold = paused.clone();
        let cap = capture.clone();
        let counter = presses.clone();
        let worker = std::thread::Builder::new()
            .name("hotkeys".into())
            .spawn(move || {
                let mut tracker = KeyTracker::default();
                let mut last_check = std::time::Instant::now();
                loop {
                    // Clips held by a key must never get stuck: every ~50 ms the keys holding
                    // them are checked against the real keyboard, in case a key-up got lost
                    // (Alt+Tab while holding, a focus change).
                    if last_check.elapsed() >= HELD_CHECK {
                        last_check = std::time::Instant::now();
                        check_held(&board, &mut tracker);
                    }
                    let raw = match raw_rx.recv_timeout(HELD_CHECK) {
                        Ok(raw) => raw,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    if raw.down {
                        counter.fetch_add(1, Ordering::Relaxed);
                    }
                    let waiting = *cap.lock().unwrap_or_else(|e| e.into_inner());
                    if waiting {
                        diag::log(format!(
                            "listener: key {} vk=0x{:02X} scan=0x{:02X} ctrl={} shift={} alt={}",
                            if raw.down { "down" } else { "up  " },
                            raw.vk,
                            raw.scan,
                            raw.ctrl,
                            raw.shift,
                            raw.alt
                        ));
                    }
                    #[cfg(windows)]
                    let event = tracker.feed_event(raw, win::key_name);
                    #[cfg(not(windows))]
                    let event = tracker.feed_event(raw, |k| format!("Key {}", k.vk));
                    let key = match event {
                        Some(KeyEvent::Pressed(key)) => key,
                        Some(KeyEvent::Released(vk)) => {
                            if !waiting && let Some(h) = &handler {
                                h.released(vk);
                            }
                            // Hold / loop clips stop with their key; once every key is up,
                            // nothing may stay held at all.
                            let mut b = board.lock().unwrap_or_else(|e| e.into_inner());
                            b.release(vk);
                            if tracker.none_held() {
                                b.release_all();
                            }
                            continue;
                        }
                        None => {
                            if waiting && raw.down {
                                diag::log(
                                    "worker: ignored (modifier on its own, or key still held)",
                                );
                            }
                            continue;
                        }
                    };
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
                        let sent = cap_tx.send(msg).is_ok();
                        diag::log(format!(
                            "worker: capture result handed to the window (ok={sent})"
                        ));
                        continue;
                    }
                    drop(capturing);
                    if hold.load(Ordering::Relaxed) {
                        continue; // e.g. Tab/Enter/Esc in the "key already used" dialog
                    }
                    if handler.as_ref().is_some_and(|h| h.pressed(&key)) {
                        continue;
                    }
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
            listener,
            worker,
            available,
            presses,
            paused,
        }
    }

    pub fn available(&self) -> bool {
        self.available
    }

    /// While paused, no hotkey triggers anything (key capture still works).
    pub fn set_paused(&self, on: bool) {
        self.paused.store(on, Ordering::Relaxed);
    }

    #[cfg(test)]
    pub fn paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    /// Number of key presses the listener has delivered since it started.
    pub fn presses_seen(&self) -> u64 {
        self.presses.load(Ordering::Relaxed)
    }

    /// The next key combination pressed (anywhere) is reported instead of triggering sounds.
    pub fn begin_capture(&self) {
        while self.captured.try_recv().is_ok() {}
        *self.capture.lock().unwrap_or_else(|e| e.into_inner()) = true;
        diag::log(format!(
            "capture started (listener running={}, key presses so far={})",
            self.available,
            self.presses_seen()
        ));
    }

    pub fn cancel_capture(&self) {
        *self.capture.lock().unwrap_or_else(|e| e.into_inner()) = false;
        diag::log("capture cancelled");
    }

    pub fn poll_capture(&self) -> Option<Captured> {
        self.captured.try_recv().ok()
    }
}

impl Drop for HotkeyService {
    fn drop(&mut self) {
        #[cfg(windows)]
        drop(self.listener.take()); // closes the window and drops the sender, ending the worker
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

#[cfg(windows)]
mod win {
    use super::RawKey;
    use std::sync::mpsc;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, GetKeyNameTextW};
    use windows_sys::Win32::UI::Input::{
        GetRawInputData, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RID_INPUT,
        RIDEV_INPUTSINK, RIDEV_REMOVE, RIM_TYPEKEYBOARD, RegisterRawInputDevices,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA,
        GetMessageW, GetWindowLongPtrW, HWND_MESSAGE, MSG, PostMessageW, PostQuitMessage,
        RegisterClassExW, SetWindowLongPtrW, WM_CLOSE, WM_DESTROY, WM_INPUT, WNDCLASSEXW,
    };

    const CLASS: &str = "VoxShift.KeyListener";
    /// RAWKEYBOARD flags.
    const RI_KEY_BREAK: u16 = 1;
    const RI_KEY_E0: u16 = 2;
    /// Generic desktop page, keyboard usage.
    const USAGE_PAGE_GENERIC: u16 = 0x01;
    const USAGE_KEYBOARD: u16 = 0x06;

    fn held(vk: i32) -> bool {
        // High bit set = key is down.
        (unsafe { GetAsyncKeyState(vk) } as u16 & 0x8000) != 0
    }

    /// Whether the keyboard says this key is down right now.
    pub fn key_down(vk: u32) -> bool {
        i32::try_from(vk).is_ok_and(held)
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Reads one WM_INPUT message; returns the key event if it was a real keyboard key.
    fn read_key(lparam: LPARAM) -> Option<RawKey> {
        // SAFETY: plain data; GetRawInputData fills at most `size` bytes.
        let mut raw: RAWINPUT = unsafe { std::mem::zeroed() };
        let mut size = std::mem::size_of::<RAWINPUT>() as u32;
        let got = unsafe {
            GetRawInputData(
                lparam as HRAWINPUT,
                RID_INPUT,
                (&mut raw as *mut RAWINPUT).cast(),
                &mut size,
                std::mem::size_of::<RAWINPUTHEADER>() as u32,
            )
        };
        if got == u32::MAX || raw.header.dwType != RIM_TYPEKEYBOARD {
            return None;
        }
        // SAFETY: dwType says the union holds keyboard data.
        let kb = unsafe { raw.data.keyboard };
        let extended = kb.Flags & RI_KEY_E0 != 0;
        // 0xFF = no real key; scan 0x2A with E0 = the "fake shift" Windows adds around some
        // navigation keys. Neither is something the user pressed.
        if kb.VKey == 0xFF || (kb.MakeCode == 0x2A && extended) {
            return None;
        }
        Some(RawKey {
            vk: u32::from(kb.VKey),
            scan: u32::from(kb.MakeCode),
            extended,
            down: kb.Flags & RI_KEY_BREAK == 0,
            ctrl: held(0x11),
            shift: held(0x10),
            alt: held(0x12),
            win: held(0x5B) || held(0x5C),
        })
    }

    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_INPUT => {
                let tx = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) }
                    as *const mpsc::Sender<RawKey>;
                if !tx.is_null()
                    && let Some(key) = read_key(lparam)
                {
                    // SAFETY: set after creation, freed only after the message loop ended.
                    let _ = unsafe { &*tx }.send(key);
                }
                // Windows must still clean up the input: always pass WM_INPUT on.
                unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
            }
            WM_CLOSE => {
                let rid = RAWINPUTDEVICE {
                    usUsagePage: USAGE_PAGE_GENERIC,
                    usUsage: USAGE_KEYBOARD,
                    dwFlags: RIDEV_REMOVE,
                    hwndTarget: std::ptr::null_mut(),
                };
                unsafe {
                    RegisterRawInputDevices(&rid, 1, std::mem::size_of::<RAWINPUTDEVICE>() as u32);
                    DestroyWindow(hwnd);
                }
                0
            }
            WM_DESTROY => {
                unsafe { PostQuitMessage(0) };
                0
            }
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
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

    /// A hidden message-only window registered for raw keyboard input with RIDEV_INPUTSINK:
    /// Windows sends it every key press, even while another app is focused. Unlike a
    /// low-level hook it cannot be silently removed by a timeout, adds no delay to anyone's
    /// typing, and cannot block keys (the game always gets them).
    pub struct Listener {
        hwnd: isize,
        handle: Option<std::thread::JoinHandle<()>>,
    }

    impl Listener {
        pub fn start(tx: mpsc::Sender<RawKey>) -> Option<Self> {
            let (ready_tx, ready_rx) = mpsc::channel::<Option<isize>>();
            let handle = std::thread::Builder::new()
                .name("key-listener".into())
                .spawn(move || unsafe {
                    let module = GetModuleHandleW(std::ptr::null());
                    let class = wide(CLASS);
                    // SAFETY: plain data, zero-initialised then filled in.
                    let mut wc: WNDCLASSEXW = std::mem::zeroed();
                    wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
                    wc.lpfnWndProc = Some(wndproc);
                    wc.hInstance = module;
                    wc.lpszClassName = class.as_ptr();
                    RegisterClassExW(&wc); // fails harmlessly if already registered
                    let hwnd = CreateWindowExW(
                        0,
                        class.as_ptr(),
                        std::ptr::null(),
                        0,
                        0,
                        0,
                        0,
                        0,
                        HWND_MESSAGE,
                        std::ptr::null_mut(),
                        module,
                        std::ptr::null(),
                    );
                    if hwnd.is_null() {
                        let _ = ready_tx.send(None);
                        return;
                    }
                    let sender = Box::into_raw(Box::new(tx));
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, sender as isize);
                    let rid = RAWINPUTDEVICE {
                        usUsagePage: USAGE_PAGE_GENERIC,
                        usUsage: USAGE_KEYBOARD,
                        dwFlags: RIDEV_INPUTSINK,
                        hwndTarget: hwnd,
                    };
                    let size = std::mem::size_of::<RAWINPUTDEVICE>() as u32;
                    if RegisterRawInputDevices(&rid, 1, size) == 0 {
                        DestroyWindow(hwnd);
                        drop(Box::from_raw(sender));
                        let _ = ready_tx.send(None);
                        return;
                    }
                    let _ = ready_tx.send(Some(hwnd as isize));
                    let mut msg: MSG = std::mem::zeroed();
                    while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                        DispatchMessageW(&msg);
                    }
                    // The window is gone: nothing can reach the sender any more.
                    drop(Box::from_raw(sender));
                })
                .ok()?;
            match ready_rx.recv() {
                Ok(Some(hwnd)) => Some(Self {
                    hwnd,
                    handle: Some(handle),
                }),
                _ => {
                    let _ = handle.join();
                    None
                }
            }
        }
    }

    impl Drop for Listener {
        fn drop(&mut self) {
            unsafe { PostMessageW(self.hwnd as HWND, WM_CLOSE, 0, 0) };
            if let Some(h) = self.handle.take() {
                let _ = h.join();
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
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
        // Windows' own name for the key is ignored when the key code is known.
        let name = |_: &RawKey| "F5 (localised)".to_string();
        assert!(t.feed(key(0x11, true, true), name).is_none(), "Ctrl alone");
        let hk = t.feed(key(0x74, true, true), name).unwrap();
        assert_eq!(hk.label, "Ctrl + F5");
        assert!(hk.ctrl && hk.vk == 0x74);
        assert!(t.feed(key(0x74, true, true), name).is_none(), "auto-repeat");
        t.feed(key(0x74, false, true), name);
        assert!(
            t.feed(key(0x74, true, false), name).is_some(),
            "pressed again"
        );
    }

    /// Presses and releases F24 system-wide. No keyboard has F24 and no app reacts to it, so
    /// this is harmless wherever the focus is; the raw input listener still sees it.
    #[cfg(windows)]
    pub(crate) fn tap_f24() {
        f24(true);
        f24(false);
    }

    /// F24 down or up (see [`tap_f24`]).
    #[cfg(windows)]
    pub(crate) fn f24(down: bool) {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
        };
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_F24 as u16,
                    wScan: 0,
                    dwFlags: if down { 0 } else { KEYEVENTF_KEYUP },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
    }

    pub(crate) const VK_F24: u32 = 0x87;

    #[cfg(windows)]
    #[test]
    fn listener_captures_a_key_press() {
        use crate::audio::device_manager::EnginePort;
        use crate::soundboard::library::Soundboard;
        let _serial = HOOK_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let sb = Soundboard::new(Arc::new(EnginePort::default()), vec![]);
        let service = HotkeyService::start(sb.board.clone(), None);
        assert!(service.available());
        service.begin_capture();
        std::thread::sleep(std::time::Duration::from_millis(100));
        tap_f24();
        let start = std::time::Instant::now();
        let got = loop {
            if let Some(c) = service.poll_capture() {
                break Some(c);
            }
            if start.elapsed().as_secs() >= 2 {
                break None;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        match got {
            Some(Captured::Key(k)) => {
                assert_eq!(k.vk, 0x87);
                println!("captured label: {}", k.label);
            }
            Some(Captured::Cancelled) => panic!("capture was cancelled"),
            None => panic!("the listener never reported the key"),
        }
    }

    /// Raw input goes to one window per process, so listener tests must not overlap.
    #[cfg(windows)]
    pub(crate) static HOOK_TESTS: Mutex<()> = Mutex::new(());

    #[cfg(windows)]
    #[test]
    fn listener_starts_restarts_and_shuts_down() {
        let _serial = HOOK_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        use crate::audio::device_manager::EnginePort;
        use crate::soundboard::library::Soundboard;
        let sb = Soundboard::new(Arc::new(EnginePort::default()), vec![]);
        for _ in 0..2 {
            let service = HotkeyService::start(sb.board.clone(), None);
            assert!(service.available(), "raw input registration failed");
            service.begin_capture();
            assert!(service.poll_capture().is_none());
            drop(service); // must not hang
        }
    }

    #[cfg(windows)]
    #[test]
    fn presses_are_counted_even_when_not_capturing() {
        use crate::audio::device_manager::EnginePort;
        use crate::soundboard::library::Soundboard;
        let _serial = HOOK_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let sb = Soundboard::new(Arc::new(EnginePort::default()), vec![]);
        let service = HotkeyService::start(sb.board.clone(), None);
        std::thread::sleep(std::time::Duration::from_millis(100));
        let before = service.presses_seen();
        tap_f24();
        let start = std::time::Instant::now();
        while service.presses_seen() == before && start.elapsed().as_secs() < 2 {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(service.presses_seen() > before);
    }

    #[test]
    fn tracker_knows_when_all_keys_are_up_and_can_forget_a_lost_key_up() {
        let mut t = KeyTracker::default();
        let name = |_: &RawKey| "F1".to_string();
        assert!(t.none_held());
        assert!(t.feed(key(0x70, true, false), name).is_some());
        assert!(!t.none_held());
        assert!(
            t.feed(key(0x11, true, true), name).is_none(),
            "Ctrl does not count"
        );
        // F1's key-up got lost: forgetting it makes the next press count again.
        t.forget(0x70);
        assert!(t.none_held());
        assert!(
            t.feed(key(0x70, true, false), name).is_some(),
            "not taken for a repeat"
        );
        assert_eq!(
            t.feed_event(key(0x70, false, false), name),
            Some(KeyEvent::Released(0x70))
        );
        assert!(t.none_held());
    }

    #[test]
    fn hotkeys_show_the_same_in_every_windows_language() {
        // Saved by a French Windows: the label is ignored, the key code names the key.
        let num7 = Hotkey::new(0x67, false, false, false, false, "7 (PAVE NUM.)");
        assert_eq!(num7.pretty(), "Num 7");
        let combo = Hotkey::new(0x77, true, true, false, false, "Ctrl+Shift+F8");
        assert_eq!(combo.pretty(), "Ctrl + Shift + F8");
        assert_eq!(combo.parts(), ["Ctrl", "Shift", "F8"]);
        // A key the table does not know keeps the name it was saved with, without the
        // modifiers the old label carried.
        let odd = Hotkey::new(0xFF, true, false, false, false, "Ctrl+Fn Lock");
        assert_eq!(odd.pretty(), "Ctrl + Fn Lock");
        let nameless = Hotkey::new(0xFF, false, false, false, false, "");
        assert_eq!(nameless.pretty(), "Key 0xFF");
        // A freshly captured key is labelled the same way.
        let mut t = KeyTracker::default();
        let pressed = t
            .feed(key(0x67, true, true), |_| "7 (PAVE NUM.)".to_string())
            .unwrap();
        assert_eq!(pressed.label, "Ctrl + Num 7");
        let unknown = t.feed(key(0xFF, true, false), |_| "Fn Lock".to_string());
        assert_eq!(unknown.unwrap().label, "Fn Lock");
    }

    #[test]
    fn hotkey_equality_ignores_the_label() {
        let a = Hotkey::new(0x61, true, false, false, false, "Ctrl+Num 1");
        let b = Hotkey::new(0x61, true, false, false, false, "whatever");
        assert_eq!(a, b);
        assert_ne!(a, Hotkey::new(0x61, false, false, false, false, "Num 1"));
    }
}
