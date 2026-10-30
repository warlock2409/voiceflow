//! Caps Lock as a dictation key (Windows).
//!
//! `RegisterHotKey`-based shortcuts cannot use Caps Lock cleanly: each press would still
//! toggle the caps lock state. A low-level keyboard hook sees press/release and swallows
//! the key so the caps state never changes. Shift/Ctrl/Alt + Caps Lock is passed through,
//! so the user can still toggle caps lock deliberately.
//!
//! Privacy: the hook receives every keystroke system-wide but only inspects whether the
//! virtual key is Caps Lock; nothing else is read, stored or logged.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Mutex};

/// What the hook should do with one Caps Lock event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Let Windows handle it (caps lock toggles as usual).
    Pass,
    /// Eat the key; `Some(pressed)` reports a hold/release edge, `None` is key auto-repeat.
    Swallow(Option<bool>),
}

/// Pure decision logic, separated from the Win32 plumbing so it can be unit-tested.
/// `held` tracks whether we own the current press.
pub fn decide(held: &AtomicBool, key_down: bool, modifier_held: bool) -> Action {
    if key_down {
        if held.load(Ordering::SeqCst) {
            return Action::Swallow(None); // auto-repeat while held
        }
        if modifier_held {
            return Action::Pass;
        }
        held.store(true, Ordering::SeqCst);
        Action::Swallow(Some(true))
    } else if held.swap(false, Ordering::SeqCst) {
        Action::Swallow(Some(false))
    } else {
        Action::Pass // release of a press we passed through
    }
}

static HELD: AtomicBool = AtomicBool::new(false);
static HOOK_THREAD: AtomicU32 = AtomicU32::new(0);
static EVENTS: Mutex<Option<mpsc::Sender<bool>>> = Mutex::new(None);

/// Install the hook. `on_change(true)` on press, `on_change(false)` on release; it runs on a
/// dedicated thread, never inside the hook (Windows drops hooks that respond slowly).
#[cfg(target_os = "windows")]
pub fn install(on_change: impl Fn(bool) + Send + 'static) -> Result<(), String> {
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetMessageW, SetWindowsHookExW, UnhookWindowsHookEx, MSG, WH_KEYBOARD_LL,
    };

    uninstall();
    let (tx, rx) = mpsc::channel::<bool>();
    *EVENTS.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
    std::thread::Builder::new()
        .name("voiceflow-capslock-events".into())
        .spawn(move || {
            while let Ok(pressed) = rx.recv() {
                on_change(pressed);
            }
        })
        .map_err(|e| e.to_string())?;

    let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), String>>(1);
    std::thread::Builder::new()
        .name("voiceflow-capslock-hook".into())
        .spawn(move || {
            // SAFETY: standard LL-hook setup; the hook proc is a plain extern fn with static state,
            // and the hook is removed on this same thread before it exits.
            unsafe {
                let module = GetModuleHandleW(None).ok().map(Into::into);
                let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), module, 0) {
                    Ok(h) => h,
                    Err(e) => {
                        let _ = ready_tx.send(Err(format!("Could not watch the Caps Lock key ({e}).")));
                        return;
                    }
                };
                HOOK_THREAD.store(GetCurrentThreadId(), Ordering::SeqCst);
                let _ = ready_tx.send(Ok(()));
                // LL hooks are serviced by this thread's message loop; WM_QUIT ends it.
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
                let _ = UnhookWindowsHookEx(hook);
            }
        })
        .map_err(|e| e.to_string())?;

    ready_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap_or_else(|_| Err("Timed out installing the Caps Lock hook.".into()))
}

/// Remove the hook (no-op if not installed). Also releases any in-progress hold.
#[cfg(target_os = "windows")]
pub fn uninstall() {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};

    let thread = HOOK_THREAD.swap(0, Ordering::SeqCst);
    if thread != 0 {
        // SAFETY: posting WM_QUIT to our own hook thread's queue.
        unsafe {
            let _ = PostThreadMessageW(thread, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
    HELD.store(false, Ordering::SeqCst);
    // Dropping the sender ends the event thread.
    EVENTS.lock().unwrap_or_else(|e| e.into_inner()).take();
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn hook_proc(
    code: i32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::Foundation::LRESULT;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CAPITAL, VK_CONTROL, VK_MENU, VK_SHIFT};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, HC_ACTION, KBDLLHOOKSTRUCT, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    if code == HC_ACTION as i32 {
        // SAFETY: for WH_KEYBOARD_LL with HC_ACTION, lparam points to a KBDLLHOOKSTRUCT.
        let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let msg = wparam.0 as u32;
        let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
        let is_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
        if kb.vkCode == VK_CAPITAL.0 as u32 && (is_down || is_up) {
            let pressed = |vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY| {
                // SAFETY: GetAsyncKeyState has no preconditions.
                (unsafe { GetAsyncKeyState(vk.0 as i32) } as u16 & 0x8000) != 0
            };
            let modifier = pressed(VK_SHIFT) || pressed(VK_CONTROL) || pressed(VK_MENU);
            match decide(&HELD, is_down, modifier) {
                Action::Pass => {}
                Action::Swallow(edge) => {
                    if let Some(p) = edge {
                        if let Some(tx) = EVENTS.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                            let _ = tx.send(p);
                        }
                    }
                    return LRESULT(1);
                }
            }
        }
    }
    // SAFETY: forwarding to the next hook with unchanged arguments.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

#[cfg(not(target_os = "windows"))]
pub fn install(_on_change: impl Fn(bool) + Send + 'static) -> Result<(), String> {
    Err("Caps Lock dictation is only available on Windows.".into())
}

#[cfg(not(target_os = "windows"))]
pub fn uninstall() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hold_and_release_produce_one_edge_each() {
        let held = AtomicBool::new(false);
        assert_eq!(decide(&held, true, false), Action::Swallow(Some(true)));
        // Auto-repeat while held is swallowed silently.
        assert_eq!(decide(&held, true, false), Action::Swallow(None));
        assert_eq!(decide(&held, true, false), Action::Swallow(None));
        assert_eq!(decide(&held, false, false), Action::Swallow(Some(false)));
    }

    #[test]
    fn modifier_plus_caps_toggles_caps_lock_normally() {
        let held = AtomicBool::new(false);
        assert_eq!(decide(&held, true, true), Action::Pass);
        assert_eq!(decide(&held, false, true), Action::Pass);
        assert!(!held.load(Ordering::SeqCst));
    }

    /// Real OS hook. Injects Caps Lock (press+release, twice) system-wide, so it is opt-in:
    /// `cargo test --lib -- --ignored real_hook`. The hook swallows the key, so the
    /// caps-lock state must be unchanged afterwards.
    #[cfg(target_os = "windows")]
    #[test]
    #[ignore]
    fn real_hook_reports_edges_and_swallows_caps() {
        use std::sync::{Arc, Mutex as M};
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            GetKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CAPITAL,
        };
        let edges = Arc::new(M::new(Vec::new()));
        let sink = edges.clone();
        install(move |p| sink.lock().unwrap().push(p)).unwrap();
        let caps_before = unsafe { GetKeyState(VK_CAPITAL.0 as i32) } & 1;
        let key = |up: bool| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT { wVk: VK_CAPITAL, dwFlags: if up { KEYEVENTF_KEYUP } else { Default::default() }, ..Default::default() },
            },
        };
        for _ in 0..2 {
            unsafe { SendInput(&[key(false), key(true)], std::mem::size_of::<INPUT>() as i32) };
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
        uninstall();
        let caps_after = unsafe { GetKeyState(VK_CAPITAL.0 as i32) } & 1;
        assert_eq!(*edges.lock().unwrap(), vec![true, false, true, false]);
        assert_eq!(caps_before, caps_after, "caps lock state changed");
    }

    #[test]
    fn releasing_modifier_mid_hold_still_ends_dictation() {
        let held = AtomicBool::new(false);
        assert_eq!(decide(&held, true, false), Action::Swallow(Some(true)));
        // Shift pressed while Caps is held: repeat still ours, release still ends it.
        assert_eq!(decide(&held, true, true), Action::Swallow(None));
        assert_eq!(decide(&held, false, true), Action::Swallow(Some(false)));
    }
}
