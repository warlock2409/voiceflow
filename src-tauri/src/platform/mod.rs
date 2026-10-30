//! OS integrations: windows/overlay placement, global hotkey, focus tracking.
//! Everything OS-specific lives here so the rest of the app stays portable.

pub mod caps_lock;
pub mod hotkey;
pub mod input;
pub mod secrets;
pub mod windows;

/// Handle of the window that had focus when dictation started (Phase 2 insertion target).
#[cfg(target_os = "windows")]
pub fn foreground_window() -> Option<isize> {
    use ::windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    // SAFETY: GetForegroundWindow has no preconditions and returns null when nothing is focused.
    let hwnd = unsafe { GetForegroundWindow() };
    (!hwnd.0.is_null()).then_some(hwnd.0 as isize)
}

#[cfg(not(target_os = "windows"))]
pub fn foreground_window() -> Option<isize> {
    None
}

/// Make an overlay non-activating and show it without taking focus.
/// Tauri's `focusable(false)` alone still lets `show()` activate the window on Windows.
#[cfg(target_os = "windows")]
pub fn show_no_activate<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> tauri::Result<()> {
    use ::windows::Win32::Foundation::HWND;
    use ::windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWL_EXSTYLE, HWND_TOPMOST, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOSIZE, SW_SHOWNOACTIVATE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };
    // Tauri may link a different `windows` crate version, so pass the handle as a raw pointer.
    let hwnd = HWND(window.hwnd()?.0 as *mut core::ffi::c_void);
    // SAFETY: hwnd is a live top-level window owned by this process.
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn show_no_activate<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> tauri::Result<()> {
    window.show()
}
