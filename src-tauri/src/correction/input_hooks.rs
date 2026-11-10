//! Low-level mouse and keyboard hooks (Windows). They only observe: every
//! event goes on to its program untouched. The correction panel uses them to
//! close on a click outside it or when the user starts typing again.

/// What the hooks report. Injected (synthesized) input is never reported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookEvent {
    /// A mouse button went down, in physical screen pixels.
    MouseDown { x: i32, y: i32 },
    /// A key that types text went down (see [`is_typing_key`]).
    TypingKey { vk: u32 },
}

// Virtual-key codes, kept local so the pure helpers build on every platform.
const VK_BACK: u32 = 0x08;
const VK_SPACE: u32 = 0x20;

/// A key that types text: letters, digits (top row and numpad), space,
/// punctuation and Backspace. Not injected input, modifiers, F-keys, arrows,
/// Tab, Enter or Esc, and nothing pressed with Ctrl, Alt or Win held.
pub fn is_typing_key(vk: u32, injected: bool, ctrl_or_alt_down: bool) -> bool {
    if injected || ctrl_or_alt_down {
        return false;
    }
    matches!(vk,
        VK_BACK
        | VK_SPACE
        | 0x30..=0x39 // 0-9
        | 0x41..=0x5A // A-Z
        | 0x60..=0x6F // numpad digits and operators
        | 0xBA..=0xC1 // ; = , - . / ` and ABNT2 /?
        | 0xC2        // ABNT2 numpad .
        | 0xDB..=0xDF // [ \ ] ' and OEM_8
        | 0xE2 // OEM_102 (\| left of Z on ABNT2)
    )
}

/// The click at (`x`, `y`) falls outside `panel` = (left, top, right, bottom),
/// with right and bottom exclusive like a Win32 `RECT`.
pub fn click_outside(x: i32, y: i32, panel: (i32, i32, i32, i32)) -> bool {
    let (left, top, right, bottom) = panel;
    !(x >= left && x < right && y >= top && y < bottom)
}

/// Virtual-key code of a temporary binding's key ("1".."9" today), so a digit
/// the panel listens to is not taken as typing.
pub fn binding_vk(key: &str) -> Option<u32> {
    match key.as_bytes() {
        [d @ b'0'..=b'9'] => Some(u32::from(*d)),
        _ => None,
    }
}

#[cfg(windows)]
mod imp {
    use super::{is_typing_key, HookEvent};
    use log::{error, info, warn};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::mpsc::{channel, Sender};
    use std::sync::{Mutex, OnceLock, RwLock};
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RMENU, VK_RWIN,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
        TranslateMessage, UnhookWindowsHookEx, KBDLLHOOKSTRUCT, LLKHF_INJECTED, LLMHF_INJECTED,
        MSG, MSLLHOOKSTRUCT, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_LBUTTONDOWN,
        WM_MBUTTONDOWN, WM_QUIT, WM_RBUTTONDOWN, WM_SYSKEYDOWN, WM_XBUTTONDOWN,
    };

    type Callback = Box<dyn Fn(HookEvent) + Send + Sync>;

    static SUBSCRIBERS: RwLock<Vec<Callback>> = RwLock::new(Vec::new());
    /// Where the hook procedures drop events; read without blocking.
    static EVENTS: OnceLock<Sender<HookEvent>> = OnceLock::new();
    /// Thread running the hooks' message loop (0 = not running).
    static HOOK_THREAD: AtomicU32 = AtomicU32::new(0);
    static STARTED: Mutex<bool> = Mutex::new(false);

    fn down(vk: u16) -> bool {
        // High bit set = key held now.
        let state = unsafe { GetAsyncKeyState(i32::from(vk)) };
        state < 0
    }

    /// Ctrl, Alt or Win held, except AltGr (Ctrl + right Alt), which types
    /// characters on ABNT2 and other layouts.
    fn ctrl_or_alt_down() -> bool {
        let altgr = down(VK_RMENU.0) && down(VK_CONTROL.0);
        !altgr && (down(VK_CONTROL.0) || down(VK_MENU.0) || down(VK_LWIN.0) || down(VK_RWIN.0))
    }

    fn send(ev: HookEvent) {
        if let Some(tx) = EVENTS.get() {
            let _ = tx.send(ev);
        }
    }

    unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            let msg = wparam.0 as u32;
            if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
                let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
                let injected = info.flags.0 & LLKHF_INJECTED.0 != 0;
                // Cheap checks first: no key state read for injected input.
                if !injected && is_typing_key(info.vkCode, false, ctrl_or_alt_down()) {
                    send(HookEvent::TypingKey { vk: info.vkCode });
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            let msg = wparam.0 as u32;
            if matches!(
                msg,
                WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
            ) {
                let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
                if info.flags & LLMHF_INJECTED == 0 {
                    send(HookEvent::MouseDown {
                        x: info.pt.x,
                        y: info.pt.y,
                    });
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    pub fn start_hooks(on_event: impl Fn(HookEvent) + Send + Sync + 'static) {
        SUBSCRIBERS
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .push(Box::new(on_event));
        let mut started = STARTED.lock().unwrap_or_else(|e| e.into_inner());
        if *started {
            return;
        }
        *started = true;

        let (tx, rx) = channel::<HookEvent>();
        let _ = EVENTS.set(tx);
        // Subscribers run here, never on the hook thread.
        let dispatcher = std::thread::Builder::new()
            .name("input-hooks-dispatch".into())
            .spawn(move || {
                for ev in rx {
                    let subs = SUBSCRIBERS.read().unwrap_or_else(|e| e.into_inner());
                    for cb in subs.iter() {
                        cb(ev);
                    }
                }
            });
        if let Err(e) = dispatcher {
            error!("Could not start the input hook dispatcher: {e}");
            return;
        }

        let hooks = std::thread::Builder::new()
            .name("input-hooks".into())
            .spawn(|| unsafe {
                HOOK_THREAD.store(GetCurrentThreadId(), Ordering::SeqCst);
                let kb = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), None, 0);
                let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), None, 0);
                match (&kb, &mouse) {
                    (Ok(_), Ok(_)) => info!("Input hooks installed (keyboard and mouse)"),
                    _ => {
                        if let Err(e) = &kb {
                            error!("Could not install the keyboard hook: {e}");
                        }
                        if let Err(e) = &mouse {
                            error!("Could not install the mouse hook: {e}");
                        }
                    }
                }
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                for h in [kb, mouse].into_iter().flatten() {
                    if let Err(e) = UnhookWindowsHookEx(h) {
                        warn!("Could not remove an input hook: {e}");
                    }
                }
                HOOK_THREAD.store(0, Ordering::SeqCst);
                info!("Input hooks removed");
            });
        if let Err(e) = hooks {
            error!("Could not start the input hook thread: {e}");
        }
    }

    pub fn stop_hooks() {
        let id = HOOK_THREAD.load(Ordering::SeqCst);
        if id != 0 {
            unsafe {
                let _ = PostThreadMessageW(id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
    }
}

/// Installs the hooks once, on their own thread with a message loop, and adds
/// `on_event` to the callbacks that get every event. Callbacks run on a
/// dispatcher thread, never inside the hook. Call again to add another one.
#[cfg(windows)]
pub fn start_hooks(on_event: impl Fn(HookEvent) + Send + Sync + 'static) {
    imp::start_hooks(on_event);
}

/// Removes the hooks (app exit).
#[cfg(windows)]
pub fn stop_hooks() {
    imp::stop_hooks();
}

#[cfg(not(windows))]
pub fn start_hooks(_on_event: impl Fn(HookEvent) + Send + Sync + 'static) {}

#[cfg(not(windows))]
pub fn stop_hooks() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_keys() {
        for vk in [0x41, 0x5A, 0x31, 0x30, VK_SPACE, VK_BACK, 0xBC, 0xE2, 0x61] {
            assert!(is_typing_key(vk, false, false), "{vk:#x} types");
        }
    }

    #[test]
    fn not_typing_keys() {
        // injected 'A'
        assert!(!is_typing_key(0x41, true, false));
        // Ctrl+A / Alt+A
        assert!(!is_typing_key(0x41, false, true));
        // Shift, LShift, Ctrl, Alt, LWin, F5, F12, arrows, Enter, Esc, Tab,
        // Delete, Home, CapsLock
        for vk in [
            0x10, 0xA0, 0x11, 0x12, 0x5B, 0x74, 0x7B, 0x25, 0x26, 0x27, 0x28, 0x0D, 0x1B, 0x09,
            0x2E, 0x24, 0x14,
        ] {
            assert!(!is_typing_key(vk, false, false), "{vk:#x} does not type");
        }
    }

    #[test]
    fn click_inside_edge_and_outside() {
        let panel = (100, 200, 860, 660);
        assert!(!click_outside(400, 400, panel));
        // Top-left corner is inside, right/bottom edges are outside (RECT).
        assert!(!click_outside(100, 200, panel));
        assert!(!click_outside(859, 659, panel));
        assert!(click_outside(860, 400, panel));
        assert!(click_outside(400, 660, panel));
        assert!(click_outside(99, 400, panel));
        assert!(click_outside(400, 199, panel));
        assert!(click_outside(-5, -5, panel));
    }

    #[test]
    fn binding_keys_map_to_vk() {
        assert_eq!(binding_vk("1"), Some(0x31));
        assert_eq!(binding_vk("9"), Some(0x39));
        assert_eq!(binding_vk("escape"), None);
        assert_eq!(binding_vk("ctrl+c"), None);
    }
}
