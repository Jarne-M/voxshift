//! The tray icon, on its own thread with its own Win32 message loop, so it keeps working
//! whether or not the egui window is visible or repainting.
//!
//! The thread owns a hidden top-level window. It receives:
//! - clicks on the notify icon (left/double click = show, right click = menu),
//! - "show yourself" from a second launch of the app ([`instance::WM_SHOW_APP`]),
//! - a timer that refreshes icon and tooltip from the app's status every 500 ms,
//! - `TaskbarCreated` (Explorer restarted: the icon must be added again),
//! - `WM_ENDSESSION` (Windows shutdown/logoff: save now). Message-only windows do not get
//!   that one, which is why this is a real (but never shown) top-level window.

use super::instance::WM_SHOW_APP;
use super::{Health, TrayAction, TrayStatus, WINDOW_CLASS, copy_wide, icon, wide};
use std::cell::RefCell;
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
    DeleteObject,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon,
    DestroyMenu, DestroyWindow, DispatchMessageW, GWLP_USERDATA, GetCursorPos, GetMessageW,
    GetWindowLongPtrW, HICON, ICONINFO, KillTimer, MF_CHECKED, MF_GRAYED, MF_SEPARATOR, MF_STRING,
    MSG, PostMessageW, PostQuitMessage, RegisterClassExW, RegisterWindowMessageW,
    SetForegroundWindow, SetMenuDefaultItem, SetTimer, SetWindowLongPtrW, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WM_APP, WM_CLOSE, WM_DESTROY, WM_ENDSESSION,
    WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_NULL, WM_QUERYENDSESSION, WM_RBUTTONUP, WM_TIMER,
    WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_OVERLAPPED,
};

const WM_TRAY: u32 = WM_APP + 1;
const WM_BALLOON: u32 = WM_APP + 3;
const WM_REFRESH: u32 = WM_APP + 4;
/// wparam: 1 = show "ON", 0 = "OFF".
const WM_OVERLAY: u32 = WM_APP + 5;
const ICON_ID: u32 = 1;
const TIMER_ID: usize = 1;
const REFRESH_MS: u32 = 500;

// Menu command ids.
const CMD_SHOW: usize = 1;
const CMD_VOICE: usize = 2;
const CMD_MUTE: usize = 3;
const CMD_STOP: usize = 4;
const CMD_QUIT: usize = 5;
const CMD_MONITOR: usize = 6;
const CMD_PANIC: usize = 7;
/// The preset quick picks: `CMD_PRESET + i` for the i-th favourite.
const CMD_PRESET: usize = 100;

type StatusFn = Box<dyn Fn() -> TrayStatus + Send>;
type ActionFn = Box<dyn Fn(TrayAction) + Send>;

/// Lives on the tray thread; reached from the window procedure through GWLP_USERDATA.
/// Only shared references are handed out (the menu's modal loop re-enters the window
/// procedure), so mutable parts are in `RefCell`s that are never held across calls out.
struct Ctx {
    hwnd: HWND,
    status: StatusFn,
    action: ActionFn,
    icons: Vec<(Health, HICON)>,
    last: RefCell<Option<TrayStatus>>,
    balloon: Arc<Mutex<Option<(String, String)>>>,
    taskbar_created: u32,
    /// The ON/OFF banner, created the first time it is needed.
    overlay: RefCell<HWND>,
}

impl Ctx {
    fn icon(&self, h: Health) -> HICON {
        self.icons
            .iter()
            .find(|(k, _)| *k == h)
            .map_or(std::ptr::null_mut(), |(_, i)| *i)
    }

    fn base_data(&self) -> NOTIFYICONDATAW {
        // SAFETY: NOTIFYICONDATAW is plain data; all-zero is a valid starting point.
        let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = self.hwnd;
        nid.uID = ICON_ID;
        nid
    }

    /// Adds the icon (`add`) or updates icon and tooltip if the status changed (or `force`).
    fn refresh(&self, add: bool, force: bool) {
        let status = (self.status)();
        let changed = self.last.borrow().as_ref() != Some(&status);
        if !(add || force || changed) {
            return;
        }
        let mut nid = self.base_data();
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        nid.uCallbackMessage = WM_TRAY;
        nid.hIcon = self.icon(status.health);
        copy_wide(&mut nid.szTip, &status.tooltip);
        unsafe { Shell_NotifyIconW(if add { NIM_ADD } else { NIM_MODIFY }, &nid) };
        *self.last.borrow_mut() = Some(status);
    }

    fn show_balloon(&self) {
        let Some((title, text)) = self
            .balloon
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        else {
            return;
        };
        let mut nid = self.base_data();
        nid.uFlags = NIF_INFO;
        nid.dwInfoFlags = NIIF_INFO;
        copy_wide(&mut nid.szInfoTitle, &title);
        copy_wide(&mut nid.szInfo, &text);
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
    }

    fn remove(&self) {
        let nid = self.base_data();
        unsafe { Shell_NotifyIconW(NIM_DELETE, &nid) };
    }

    fn menu(&self) {
        let s = (self.status)();
        let item = |text: &str| wide(text);
        unsafe {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                return;
            }
            let show = item("Show VoxShift");
            let voice = item(if s.voice_on {
                "Voice changer: ON"
            } else {
                "Voice changer: OFF"
            });
            let mute = item("Mute mic");
            let monitor = item("Hear myself (monitor)");
            let active = item(&s.active);
            let stop = item("Stop all sounds");
            let panic = item(if s.safe {
                "SAFE MODE: Restore everything"
            } else {
                "Panic: safe mode (all off)"
            });
            let quit = item("Quit VoxShift");
            let checked = |on: bool| if on { MF_CHECKED } else { 0 };
            AppendMenuW(menu, MF_STRING, CMD_SHOW, show.as_ptr());
            AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
            AppendMenuW(menu, MF_STRING | checked(s.safe), CMD_PANIC, panic.as_ptr());
            AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
            AppendMenuW(
                menu,
                MF_STRING | checked(s.voice_on),
                CMD_VOICE,
                voice.as_ptr(),
            );
            AppendMenuW(menu, MF_STRING | checked(s.muted), CMD_MUTE, mute.as_ptr());
            AppendMenuW(
                menu,
                MF_STRING | checked(s.monitor),
                CMD_MONITOR,
                monitor.as_ptr(),
            );
            AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, active.as_ptr());
            // Favourite presets as quick picks (built from this status, so the menu always
            // shows the favourites as they are when it opens).
            let picks: Vec<(Vec<u16>, bool)> = super::preset_items(&s)
                .into_iter()
                .map(|(label, on)| (item(&label), on))
                .collect();
            if !picks.is_empty() {
                AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
                let title = item("Favourite presets");
                AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, title.as_ptr());
                for (i, (label, on)) in picks.iter().enumerate() {
                    AppendMenuW(
                        menu,
                        MF_STRING | checked(*on),
                        CMD_PRESET + i,
                        label.as_ptr(),
                    );
                }
            }
            AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
            AppendMenuW(menu, MF_STRING, CMD_STOP, stop.as_ptr());
            AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
            AppendMenuW(menu, MF_STRING, CMD_QUIT, quit.as_ptr());
            SetMenuDefaultItem(menu, CMD_SHOW as u32, 0);

            let mut pt = POINT { x: 0, y: 0 };
            GetCursorPos(&mut pt);
            // Required so the menu closes when clicking elsewhere (documented quirk).
            SetForegroundWindow(self.hwnd);
            let cmd = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON,
                pt.x,
                pt.y,
                0,
                self.hwnd,
                std::ptr::null(),
            );
            PostMessageW(self.hwnd, WM_NULL, 0, 0);
            DestroyMenu(menu);
            let action = match cmd as usize {
                CMD_SHOW => Some(TrayAction::Show),
                CMD_VOICE => Some(TrayAction::ToggleVoice),
                CMD_MUTE => Some(TrayAction::ToggleMute),
                CMD_MONITOR => Some(TrayAction::ToggleMonitor),
                CMD_PANIC => Some(TrayAction::TogglePanic),
                CMD_STOP => Some(TrayAction::StopAllSounds),
                CMD_QUIT => Some(TrayAction::Quit),
                n if (CMD_PRESET..CMD_PRESET + picks.len()).contains(&n) => s
                    .presets
                    .get(n - CMD_PRESET)
                    .cloned()
                    .map(TrayAction::Preset),
                _ => None,
            };
            if let Some(a) = action {
                (self.action)(a);
                self.refresh(false, true);
            }
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Ctx;
    if ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    // SAFETY: set right after creation and freed only after the message loop ended.
    let ctx = unsafe { &*ptr };
    match msg {
        WM_TRAY => {
            match lparam as u32 {
                WM_LBUTTONUP | WM_LBUTTONDBLCLK => (ctx.action)(TrayAction::Show),
                WM_RBUTTONUP => ctx.menu(),
                _ => {}
            }
            0
        }
        WM_SHOW_APP => {
            (ctx.action)(TrayAction::Show);
            0
        }
        WM_TIMER | WM_REFRESH => {
            ctx.refresh(false, msg == WM_REFRESH);
            0
        }
        WM_BALLOON => {
            ctx.show_balloon();
            0
        }
        WM_OVERLAY => {
            let mut banner = ctx.overlay.borrow_mut();
            if banner.is_null() {
                *banner = super::overlay::create();
            }
            super::overlay::show(*banner, wparam != 0);
            0
        }
        WM_QUERYENDSESSION => 1,
        WM_ENDSESSION => {
            if wparam != 0 {
                (ctx.action)(TrayAction::SessionEnd);
                ctx.remove();
            }
            0
        }
        WM_CLOSE => {
            ctx.remove();
            unsafe {
                KillTimer(hwnd, TIMER_ID);
                DestroyWindow(hwnd);
            }
            0
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            0
        }
        m if m == ctx.taskbar_created => {
            ctx.refresh(true, true); // Explorer restarted: add the icon again
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Turns RGBA pixels into an HICON (32-bit with alpha).
fn make_icon(rgba: &[u8], size: u32) -> HICON {
    unsafe {
        // SAFETY: plain data, zero-initialised then filled in.
        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = size as i32;
        bmi.bmiHeader.biHeight = -(size as i32); // top-down rows
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB;
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let color = CreateDIBSection(
            std::ptr::null_mut(),
            &bmi,
            DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        );
        if color.is_null() || bits.is_null() {
            return std::ptr::null_mut();
        }
        let dst = std::slice::from_raw_parts_mut(bits.cast::<u8>(), rgba.len());
        for (d, s) in dst
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(rgba.as_chunks::<4>().0)
        {
            *d = [s[2], s[1], s[0], s[3]]; // RGBA -> BGRA
        }
        let mask_bits = vec![0u8; (size * size / 8) as usize];
        let mask = CreateBitmap(size as i32, size as i32, 1, 1, mask_bits.as_ptr().cast());
        let info = ICONINFO {
            fIcon: 1,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let icon = CreateIconIndirect(&info);
        DeleteObject(color);
        DeleteObject(mask);
        icon
    }
}

/// Posts to the tray thread from anywhere (posting to a window that is gone does nothing).
#[derive(Clone, Copy, Debug)]
pub struct TrayPoster(isize);

impl TrayPoster {
    /// Re-reads the status now (icon, tooltip).
    pub fn refresh(self) {
        unsafe { PostMessageW(self.0 as HWND, WM_REFRESH, 0, 0) };
    }

    /// Shows the "Voice changer: ON/OFF" banner for about a second.
    pub fn overlay(self, on: bool) {
        unsafe { PostMessageW(self.0 as HWND, WM_OVERLAY, usize::from(on), 0) };
    }
}

/// Owned by the app. Dropping it removes the icon and ends the thread.
pub struct Tray {
    hwnd: isize,
    balloon: Arc<Mutex<Option<(String, String)>>>,
    thread: Option<JoinHandle<()>>,
}

impl Tray {
    /// Starts the tray thread. `status` is polled for the icon, tooltip and menu; `action`
    /// is called (on the tray thread) for clicks and menu choices.
    pub fn start(
        status: impl Fn() -> TrayStatus + Send + 'static,
        action: impl Fn(TrayAction) + Send + 'static,
    ) -> Result<Self, String> {
        let balloon = Arc::new(Mutex::new(None));
        let b = balloon.clone();
        let (tx, rx) = mpsc::channel::<Result<isize, String>>();
        let status: StatusFn = Box::new(status);
        let action: ActionFn = Box::new(action);
        let thread = std::thread::Builder::new()
            .name("tray".into())
            .spawn(move || run(status, action, b, tx))
            .map_err(|e| e.to_string())?;
        match rx.recv() {
            Ok(Ok(hwnd)) => Ok(Self {
                hwnd,
                balloon,
                thread: Some(thread),
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => Err("tray thread ended unexpectedly".into()),
        }
    }

    /// Shows a balloon notification next to the tray icon.
    pub fn notify(&self, title: &str, text: &str) {
        *self.balloon.lock().unwrap_or_else(|e| e.into_inner()) =
            Some((title.to_string(), text.to_string()));
        unsafe { PostMessageW(self.hwnd as HWND, WM_BALLOON, 0, 0) };
    }

    /// Re-reads the status right away (instead of waiting for the next timer tick).
    pub fn refresh(&self) {
        self.poster().refresh();
    }

    /// A handle other threads can use to poke the tray.
    pub fn poster(&self) -> TrayPoster {
        TrayPoster(self.hwnd)
    }

    /// Removes the icon and stops the thread. Safe to call twice.
    pub fn shutdown(&mut self) {
        if let Some(t) = self.thread.take() {
            unsafe { PostMessageW(self.hwnd as HWND, WM_CLOSE, 0, 0) };
            let _ = t.join();
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run(
    status: StatusFn,
    action: ActionFn,
    balloon: Arc<Mutex<Option<(String, String)>>>,
    started: mpsc::Sender<Result<isize, String>>,
) {
    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        let class = wide(WINDOW_CLASS);
        // SAFETY: plain data, zero-initialised then filled in.
        let mut wc: WNDCLASSEXW = std::mem::zeroed();
        wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
        wc.lpfnWndProc = Some(wndproc);
        wc.hInstance = module;
        wc.lpszClassName = class.as_ptr();
        RegisterClassExW(&wc); // fails harmlessly if already registered (tests)
        let title = wide("VoxShift tray");
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPED, // never shown
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            module,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            let _ = started.send(Err("could not create the tray window".into()));
            return;
        }
        let icons = [
            Health::Ok,
            Health::VoiceOff,
            Health::Muted,
            Health::NoMic,
            Health::NoOutput,
            Health::Safe,
        ]
        .into_iter()
        .map(|h| (h, make_icon(&icon::render(h, 32), 32)))
        .collect();
        let ctx = Box::new(Ctx {
            hwnd,
            status,
            action,
            icons,
            last: RefCell::new(None),
            balloon,
            taskbar_created: RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
            overlay: RefCell::new(std::ptr::null_mut()),
        });
        let ptr = Box::into_raw(ctx);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr as isize);
        (*ptr).refresh(true, true);
        SetTimer(hwnd, TIMER_ID, REFRESH_MS, None);
        let _ = started.send(Ok(hwnd as isize));

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        // The window is destroyed: nothing can reach the context any more.
        let ctx = Box::from_raw(ptr);
        for (_, icon) in &ctx.icons {
            DestroyIcon(*icon);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Run by hand (it shows the banner on screen): the overlay appears and hides itself.
    #[test]
    #[ignore]
    fn overlay_shows_and_hides_itself() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, IsWindowVisible};
        let mut tray = Tray::start(
            || TrayStatus {
                health: Health::Ok,
                voice_on: true,
                muted: false,
                monitor: true,
                safe: false,
                active: String::new(),
                tooltip: "VoxShift overlay test".into(),
                presets: Vec::new(),
                preset: None,
            },
            |_| {},
        )
        .unwrap();
        tray.poster().overlay(true);
        std::thread::sleep(std::time::Duration::from_millis(300));
        let class = wide("VoxShift.Overlay");
        let hwnd = unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) };
        assert!(!hwnd.is_null(), "banner window exists");
        assert!(unsafe { IsWindowVisible(hwnd) } != 0, "banner visible");
        std::thread::sleep(std::time::Duration::from_millis(1200));
        assert!(unsafe { IsWindowVisible(hwnd) } == 0, "hidden again after ~1 s");
        tray.shutdown();
    }

    #[test]
    fn icon_conversion_produces_a_handle() {
        let icon = make_icon(&icon::render(Health::NoMic, 32), 32);
        assert!(!icon.is_null());
        unsafe { DestroyIcon(icon) };
    }

    /// Real tray icon (visible for under a second): added, polled for status, removed again.
    /// No balloon here, so running the tests does not pop notifications.
    #[test]
    fn tray_starts_polls_status_and_shuts_down() {
        let polls = Arc::new(AtomicUsize::new(0));
        let p = polls.clone();
        let mut tray = Tray::start(
            move || {
                p.fetch_add(1, Ordering::Relaxed);
                TrayStatus {
                    health: Health::Ok,
                    voice_on: true,
                    muted: false,
                    monitor: true,
                    safe: false,
                    active: "Active: Primary mic".into(),
                    tooltip: "VoxShift test".into(),
                    presets: vec!["Late night".into()],
                    preset: Some("Late night".into()),
                }
            },
            |_| {},
        )
        .expect("tray started");
        tray.refresh();
        std::thread::sleep(std::time::Duration::from_millis(700));
        assert!(polls.load(Ordering::Relaxed) >= 2, "added + timer/refresh");
        tray.shutdown();
        tray.shutdown(); // second call is a no-op
    }
}
