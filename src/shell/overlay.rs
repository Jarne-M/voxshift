//! A small always-on-top "Voice changer: ON/OFF" banner, shown for about a second at the top
//! of the screen when the voice changer is switched. It never takes focus and clicks go
//! through it. It lives on the tray thread, so it works while the main window is hidden.
//! (Games in exclusive fullscreen draw over everything, so it shows in windowed/borderless.)

use super::wide;
use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DT_CENTER, DT_SINGLELINE, DT_VCENTER, DeleteObject,
    DrawTextW, EndPaint, FW_BOLD, FillRect, InvalidateRect, PAINTSTRUCT, SelectObject, SetBkMode,
    SetTextColor, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GWLP_USERDATA, GetClientRect, GetSystemMetrics,
    GetWindowLongPtrW, HWND_TOPMOST, KillTimer, LWA_ALPHA, RegisterClassExW, SM_CXSCREEN, SW_HIDE,
    SWP_NOACTIVATE, SWP_SHOWWINDOW, SetLayeredWindowAttributes, SetTimer, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, WM_PAINT, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

const CLASS: &str = "VoxShift.Overlay";
const WIDTH: i32 = 320;
const HEIGHT: i32 = 60;
const TOP: i32 = 48;
const SHOW_MS: u32 = 1000;
const TIMER_HIDE: usize = 1;

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16)
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let on = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } != 0;
            unsafe {
                // SAFETY: plain data filled in by BeginPaint.
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let hdc = BeginPaint(hwnd, &mut ps);
                let mut rect: RECT = std::mem::zeroed();
                GetClientRect(hwnd, &mut rect);
                let brush = if on {
                    CreateSolidBrush(rgb(40, 150, 90))
                } else {
                    CreateSolidBrush(rgb(70, 70, 78))
                };
                FillRect(hdc, &rect, brush);
                DeleteObject(brush);
                let face = wide("Segoe UI");
                let font = CreateFontW(
                    26,
                    0,
                    0,
                    0,
                    FW_BOLD as i32,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    5,
                    0,
                    face.as_ptr(),
                );
                let old = SelectObject(hdc, font);
                SetBkMode(hdc, TRANSPARENT as i32);
                SetTextColor(hdc, rgb(255, 255, 255));
                let mut text = wide(if on {
                    "Voice changer: ON"
                } else {
                    "Voice changer: OFF"
                });
                DrawTextW(
                    hdc,
                    text.as_mut_ptr(),
                    -1,
                    &mut rect,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                );
                SelectObject(hdc, old);
                DeleteObject(font);
                EndPaint(hwnd, &ps);
            }
            0
        }
        WM_TIMER => {
            unsafe {
                KillTimer(hwnd, TIMER_HIDE);
                ShowWindow(hwnd, SW_HIDE);
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Creates the (hidden) banner window on the calling thread, which must run a message loop.
pub fn create() -> HWND {
    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        let class = wide(CLASS);
        // SAFETY: plain data, zero-initialised then filled in.
        let mut wc: WNDCLASSEXW = std::mem::zeroed();
        wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
        wc.lpfnWndProc = Some(proc);
        wc.hInstance = module;
        wc.lpszClassName = class.as_ptr();
        RegisterClassExW(&wc);
        let title = wide("VoxShift overlay");
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TRANSPARENT,
            class.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            0,
            0,
            WIDTH,
            HEIGHT,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            module,
            std::ptr::null(),
        );
        if !hwnd.is_null() {
            SetLayeredWindowAttributes(hwnd, 0, 235, LWA_ALPHA);
        }
        hwnd
    }
}

/// Shows "ON" or "OFF" for about a second, top-centre of the main screen.
pub fn show(hwnd: HWND, on: bool) {
    if hwnd.is_null() {
        return;
    }
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, isize::from(on));
        let x = (GetSystemMetrics(SM_CXSCREEN) - WIDTH) / 2;
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            x,
            TOP,
            WIDTH,
            HEIGHT,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        InvalidateRect(hwnd, std::ptr::null(), 1);
        SetTimer(hwnd, TIMER_HIDE, SHOW_MS, None); // restarts the timer if already showing
    }
}
