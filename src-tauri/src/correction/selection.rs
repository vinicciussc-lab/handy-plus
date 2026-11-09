//! Copies the user's selection out of the focused app and brings focus back to
//! it for the paste. The user's clipboard is restored after the copy.

use crate::clipboard::{with_enigo, write_text_to_clipboard};
use crate::input;
use log::{debug, warn};
use std::time::{Duration, Instant};
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

const MODIFIER_RELEASE_TIMEOUT: Duration = Duration::from_millis(600);
const COPY_POLL_INTERVAL: Duration = Duration::from_millis(50);
const COPY_TIMEOUT: Duration = Duration::from_millis(1000);
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

/// Waits (up to 600 ms) for Ctrl/Shift/Alt/Win to be released, so the shortcut
/// the user is still holding does not turn our Ctrl+C into something else.
#[cfg(windows)]
fn wait_for_modifiers_released() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };
    let any_down = || {
        [VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN]
            .iter()
            // SAFETY: plain query of global key state. The high bit means down.
            .any(|k| unsafe { GetAsyncKeyState(k.0 as i32) } < 0)
    };
    let start = Instant::now();
    while any_down() {
        if start.elapsed() >= MODIFIER_RELEASE_TIMEOUT {
            warn!("Modifier keys still held after {MODIFIER_RELEASE_TIMEOUT:?}; copying anyway");
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(not(windows))]
fn wait_for_modifiers_released() {
    let _ = (MODIFIER_RELEASE_TIMEOUT, Instant::now());
}

/// Copies the current selection via Ctrl+C and returns it with the window it
/// came from. Call from a worker thread: it sleeps and polls. The user's
/// clipboard (text, or an image when there is no text) is restored on every path.
pub fn capture_selection(app: &AppHandle) -> Result<Captured, CaptureError> {
    let hwnd = foreground_window();
    wait_for_modifiers_released();

    let clipboard = app.clipboard();
    let saved_text = clipboard.read_text().ok().filter(|t| !t.is_empty());
    // Same as paste_via_clipboard: decoding an image is only worth it when
    // there is no text to restore.
    let saved_image = if saved_text.is_none() {
        clipboard.read_image().ok().map(|image| image.to_owned())
    } else {
        None
    };

    let copied = copy_selection(app);

    if let Some(text) = &saved_text {
        let _ = write_text_to_clipboard(app, text);
    } else if let Some(image) = &saved_image {
        let _ = clipboard.write_image(image);
    } else {
        let _ = clipboard.clear();
    }

    let text = copied.map_err(CaptureError::Clipboard)?;
    if is_blank_selection(&text) {
        return Err(CaptureError::NothingSelected);
    }
    debug!("Captured {} chars of selection", text.chars().count());
    Ok(Captured { text, hwnd })
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

/// Brings the window the selection came from back to the foreground, if it is not already.
#[cfg(windows)]
pub fn refocus(hwnd: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, SetForegroundWindow};
    if hwnd == 0 {
        return;
    }
    let target = HWND(hwnd as *mut std::ffi::c_void);
    // SAFETY: both calls only take/return window handles; a stale handle just fails.
    unsafe {
        if GetForegroundWindow() != target && !SetForegroundWindow(target).as_bool() {
            warn!("SetForegroundWindow refused to focus the original window");
        }
    }
}

#[cfg(not(windows))]
pub fn refocus(_hwnd: isize) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitespace_only_selection_counts_as_nothing() {
        assert!(is_blank_selection("  \n\t "));
        assert!(!is_blank_selection(" oi "));
    }
}
