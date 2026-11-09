//! Copies the user's selection out of the focused app and brings focus back to
//! it for the paste. The user's clipboard is restored after the copy.

use crate::clipboard::{with_enigo, with_paste_lock, write_text_to_clipboard};
use crate::input;
use log::{debug, warn};
use std::time::{Duration, Instant};
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

const MODIFIER_RELEASE_TIMEOUT: Duration = Duration::from_millis(600);
const COPY_POLL_INTERVAL: Duration = Duration::from_millis(50);
const COPY_TIMEOUT: Duration = Duration::from_millis(1000);
const LATE_WRITE_GRACE: Duration = Duration::from_millis(300);
const COPY_HOLD_MS: u64 = 50;

/// The selected text and the window it came from.
pub struct Captured {
    pub text: String,
    /// Foreground window at capture time; 0 when unknown (non-Windows).
    pub hwnd: isize,
}

#[derive(Debug)]
pub enum CaptureError {
    NothingSelected,
    /// Ctrl/Shift/Alt/Win were still held after the wait, so no copy was sent.
    KeysHeld,
    Clipboard(String),
}

/// A selection with only whitespace is the same as no selection.
pub fn is_blank_selection(text: &str) -> bool {
    text.trim().is_empty()
}

#[cfg(windows)]
fn foreground_window() -> isize {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    // SAFETY: no arguments; returns a possibly-null handle.
    unsafe { GetForegroundWindow() }.0 as isize
}

#[cfg(not(windows))]
fn foreground_window() -> isize {
    0
}

/// Polls `any_down` until it reports false or `timeout` passes. Returns whether
/// the keys were released.
fn wait_until_released(
    mut any_down: impl FnMut() -> bool,
    timeout: Duration,
    step: Duration,
) -> bool {
    let start = Instant::now();
    while any_down() {
        if start.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(step);
    }
    true
}

#[cfg(windows)]
fn modifiers_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };
    [VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN]
        .iter()
        // SAFETY: plain query of global key state. The high bit means down.
        .any(|k| unsafe { GetAsyncKeyState(k.0 as i32) } < 0)
}

#[cfg(not(windows))]
fn modifiers_down() -> bool {
    false
}

/// Sequence number that changes on every clipboard write by any app.
#[cfg(windows)]
fn clipboard_sequence() -> Option<u32> {
    use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
    // SAFETY: no arguments; 0 means no access to the window station.
    match unsafe { GetClipboardSequenceNumber() } {
        0 => None,
        n => Some(n),
    }
}

#[cfg(not(windows))]
fn clipboard_sequence() -> Option<u32> {
    None
}

/// True when someone wrote to the clipboard after our restore, so the restore
/// has to be redone.
fn should_rerestore(after_restore: Option<u32>, now: Option<u32>) -> bool {
    matches!((after_restore, now), (Some(a), Some(n)) if a != n)
}

/// Copies the current selection via Ctrl+C and returns it with the window it
/// came from. Call from a worker thread: it sleeps and polls. The user's
/// clipboard (text, or an image when there is no text) is restored on every path.
pub fn capture_selection(app: &AppHandle) -> Result<Captured, CaptureError> {
    let hwnd = foreground_window();
    if !wait_until_released(
        modifiers_down,
        MODIFIER_RELEASE_TIMEOUT,
        Duration::from_millis(10),
    ) {
        warn!("Modifier keys still held after {MODIFIER_RELEASE_TIMEOUT:?}; not copying");
        return Err(CaptureError::KeysHeld);
    }

    // A dictation paste uses the clipboard the same way; never interleave.
    let text = with_paste_lock(|| copy_and_restore(app))?;
    if is_blank_selection(&text) {
        return Err(CaptureError::NothingSelected);
    }
    debug!("Captured {} chars of selection", text.chars().count());
    Ok(Captured { text, hwnd })
}

/// Saves the clipboard, copies the selection and restores the clipboard.
/// Returns an empty string when nothing was copied.
fn copy_and_restore(app: &AppHandle) -> Result<String, CaptureError> {
    let clipboard = app.clipboard();
    let saved_text = clipboard.read_text().ok().filter(|t| !t.is_empty());
    // Same as paste_via_clipboard: decoding an image is only worth it when
    // there is no text to restore.
    let saved_image = if saved_text.is_none() {
        clipboard.read_image().ok().map(|image| image.to_owned())
    } else {
        None
    };

    let restore = || {
        if let Some(text) = &saved_text {
            if let Err(e) = write_text_to_clipboard(app, text) {
                warn!("Failed to restore the clipboard text: {e}");
            }
        } else if let Some(image) = &saved_image {
            if let Err(e) = clipboard.write_image(image) {
                warn!("Failed to restore the clipboard image: {e}");
            }
        } else if let Err(e) = clipboard.clear() {
            warn!("Failed to clear the clipboard: {e}");
        }
    };

    let copied = copy_selection(app);
    restore();

    let text = copied.map_err(CaptureError::Clipboard)?;
    if text.is_empty() {
        // Timed out: a slow app may still write the selection after our restore.
        let after_restore = clipboard_sequence();
        std::thread::sleep(LATE_WRITE_GRACE);
        if should_rerestore(after_restore, clipboard_sequence()) {
            warn!("Clipboard changed after the copy timed out; restoring it again");
            restore();
        }
    }
    Ok(text)
}

/// Clears the clipboard, sends Ctrl+C and polls for the copied text. Returns an
/// empty string when nothing arrived. Does not restore the clipboard.
fn copy_selection(app: &AppHandle) -> Result<String, String> {
    let clipboard = app.clipboard();
    clipboard
        .clear()
        .map_err(|e| format!("Failed to clear the clipboard: {e}"))?;

    with_enigo(app, |enigo| input::send_copy_ctrl_c(enigo, COPY_HOLD_MS))?;

    let start = Instant::now();
    loop {
        std::thread::sleep(COPY_POLL_INTERVAL);
        if let Ok(text) = clipboard.read_text() {
            if !text.is_empty() {
                return Ok(text);
            }
        }
        if start.elapsed() >= COPY_TIMEOUT {
            return Ok(String::new());
        }
    }
}

/// Brings the window the selection came from back to the foreground. Returns
/// whether it is the foreground window afterwards; callers must not paste if not.
#[cfg(windows)]
pub fn refocus(hwnd: isize) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, IsIconic, SetForegroundWindow, ShowWindow,
        SW_RESTORE,
    };
    if hwnd == 0 {
        return false;
    }
    let target = HWND(hwnd as *mut std::ffi::c_void);
    // SAFETY: only window handles are passed; a stale handle makes the calls fail.
    unsafe {
        if GetForegroundWindow() == target {
            return true;
        }
        if IsIconic(target).as_bool() {
            let _ = ShowWindow(target, SW_RESTORE);
        }
        if !SetForegroundWindow(target).as_bool() {
            // Windows only lets the foreground thread hand off focus: attach to
            // its input queue for the call.
            let fg_thread = GetWindowThreadProcessId(GetForegroundWindow(), None);
            let this_thread = GetCurrentThreadId();
            let attached = fg_thread != 0
                && fg_thread != this_thread
                && AttachThreadInput(this_thread, fg_thread, true).as_bool();
            let _ = SetForegroundWindow(target);
            if attached {
                let _ = AttachThreadInput(this_thread, fg_thread, false);
            }
        }
        std::thread::sleep(Duration::from_millis(50));
        let ok = GetForegroundWindow() == target;
        if !ok {
            warn!("Could not bring the original window back to the foreground");
        }
        ok
    }
}

#[cfg(not(windows))]
pub fn refocus(_hwnd: isize) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitespace_only_selection_counts_as_nothing() {
        assert!(is_blank_selection("  \n\t "));
        assert!(!is_blank_selection(" oi "));
    }

    #[test]
    fn rerestores_only_when_the_clipboard_changed_after_our_restore() {
        assert!(should_rerestore(Some(5), Some(6)));
        assert!(!should_rerestore(Some(5), Some(5)));
        assert!(!should_rerestore(None, Some(6)));
        assert!(!should_rerestore(Some(5), None));
    }

    #[test]
    fn modifier_wait_returns_true_once_released_and_false_on_timeout() {
        let mut polls = 0;
        let released = wait_until_released(
            || {
                polls += 1;
                polls < 3
            },
            Duration::from_secs(5),
            Duration::from_millis(1),
        );
        assert!(released);
        assert_eq!(polls, 3);
        assert!(!wait_until_released(
            || true,
            Duration::from_millis(20),
            Duration::from_millis(1)
        ));
    }
}
