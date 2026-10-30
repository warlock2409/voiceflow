//! Synthetic input for text insertion (Windows).
//!
//! Limitation (by OS design): Windows silently blocks `SendInput` into windows of
//! elevated (admin) apps when VoiceFlow is not elevated, and some secure fields ignore
//! synthetic input. The caller always leaves a clipboard fallback for those cases.

use std::time::{Duration, Instant};

/// True if `hwnd` is a window owned by this VoiceFlow process.
#[cfg(target_os = "windows")]
pub fn is_own_window(hwnd: isize) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    let mut pid = 0u32;
    // SAFETY: GetWindowThreadProcessId tolerates stale handles (returns 0).
    unsafe { GetWindowThreadProcessId(HWND(hwnd as *mut core::ffi::c_void), Some(&mut pid)) };
    pid == std::process::id()
}

#[cfg(target_os = "windows")]
fn modifiers_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };
    [VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN]
        .iter()
        // SAFETY: GetAsyncKeyState has no preconditions.
        .any(|vk| (unsafe { GetAsyncKeyState(vk.0 as i32) } as u16 & 0x8000) != 0)
}

/// Wait until the user has let go of Ctrl/Shift/Alt/Win, so a synthetic Ctrl+V
/// does not turn into Ctrl+Shift+V, Win+V, etc. Returns false on timeout.
#[cfg(target_os = "windows")]
pub fn wait_for_modifiers_released(timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while modifiers_down() {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    true
}

/// Press and release Ctrl+V in the foreground window.
#[cfg(target_os = "windows")]
pub fn send_ctrl_v() -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY,
        VK_CONTROL, VK_V,
    };
    let key = |vk: VIRTUAL_KEY, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: vk, dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, ..Default::default() },
        },
    };
    let inputs = [key(VK_CONTROL, false), key(VK_V, false), key(VK_V, true), key(VK_CONTROL, true)];
    // SAFETY: inputs is a valid slice of INPUT structs.
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err("Windows blocked the paste keystroke.".into())
    }
}

#[cfg(not(target_os = "windows"))]
pub fn is_own_window(_hwnd: isize) -> bool {
    false
}

#[cfg(not(target_os = "windows"))]
pub fn wait_for_modifiers_released(_timeout: Duration) -> bool {
    true
}

#[cfg(not(target_os = "windows"))]
pub fn send_ctrl_v() -> Result<(), String> {
    let _ = Instant::now();
    Err("Automatic pasting is only implemented on Windows so far.".into())
}
