//! Floating correction button: a small round window at the bottom-right of
//! the focused text box. Clicking it corrects the whole box.
//!
//! A background thread polls the focused field (`text_focus::focused_field`,
//! which must never run on the UI thread) and posts only the changes to the
//! main thread, which moves, shows or hides the window.
//!
//! The window is shown and hidden with Win32 calls that never activate it,
//! instead of tao's `show()` (a plain `SW_SHOW`), so it can't take the focus
//! away from the text box it sits on.

// Focus detection only exists on Windows; elsewhere the window is never made.
#![cfg_attr(not(windows), allow(dead_code))]

use super::session;
use super::text_focus::button_position;
use log::debug;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use tauri::{AppHandle, Manager};

/// Button window size, in logical pixels.
pub const BUTTON_SIZE: f64 = 28.0;
/// Gap between the button and the box's edges, in logical pixels.
const BUTTON_INSET: f64 = 6.0;
pub const BUTTON_WINDOW: &str = "correction_button";

/// Shared by the watcher thread, the click command and app exit.
#[derive(Default)]
pub struct ButtonState {
    /// Where the button is (or is about to be) shown; `None` when hidden.
    shown: Mutex<Option<ButtonTarget>>,
    stop: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Where the button goes: the field's top-level window and its rectangle
/// (physical pixels). Compared tick to tick so only changes reach the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ButtonTarget {
    pub hwnd: isize,
    pub rect: (i32, i32, i32, i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonAction {
    /// Move the button to the target (and show it if hidden).
    Place(ButtonTarget),
    Hide,
}

/// What to do to the window when the wanted target differs from the shown one.
pub fn next_action(
    shown: Option<ButtonTarget>,
    wanted: Option<ButtonTarget>,
) -> Option<ButtonAction> {
    if shown == wanted {
        return None;
    }
    Some(wanted.map_or(ButtonAction::Hide, ButtonAction::Place))
}

/// Physical pixels to logical pixels at a monitor's scale factor.
pub fn to_logical(physical: (i32, i32), scale: f64) -> (f64, f64) {
    (f64::from(physical.0) / scale, f64::from(physical.1) / scale)
}

/// Button window bounds in physical pixels (x, y, size) for a field rect on a
/// monitor with `scale`; `text_scale` is the Windows text size, which WebView2
/// applies as a zoom on top of the display scale.
pub fn button_bounds(rect: (i32, i32, i32, i32), scale: f64, text_scale: f64) -> (i32, i32, i32) {
    let size = (BUTTON_SIZE * scale * text_scale).round().max(1.0) as i32;
    let inset = (BUTTON_INSET * scale).round() as i32;
    let (x, y) = button_position(rect, size, inset);
    (x, y, size)
}

/// Hides the button and corrects the box it was sitting on.
#[tauri::command]
#[specta::specta]
pub fn correction_button_clicked(app: AppHandle) {
    let Some(state) = app.try_state::<Arc<ButtonState>>() else {
        return;
    };
    let target = {
        let mut shown = lock(&state.shown);
        post(&app, ButtonAction::Hide, None);
        shown.take()
    };
    match target {
        Some(target) => session::begin_from_button(&app, target.hwnd),
        None => debug!("Floating button clicked while hidden; ignored"),
    }
}

/// Stops the watcher and hides the button (app exit).
pub fn shutdown(app: &AppHandle) {
    if let Some(state) = app.try_state::<Arc<ButtonState>>() {
        state.stop.store(true, Ordering::SeqCst);
        *lock(&state.shown) = None;
    }
    if let Some(window) = app.get_webview_window(BUTTON_WINDOW) {
        win::hide(&window);
    }
}

/// Sends a window change to the main thread. With `session_active`, a late
/// `Place` that finds a session started meanwhile (by a click) hides instead.
fn post(app: &AppHandle, action: ButtonAction, session_active: Option<Arc<AtomicBool>>) {
    let handle = app.clone();
    let posted = app.run_on_main_thread(move || {
        let Some(window) = handle.get_webview_window(BUTTON_WINDOW) else {
            return;
        };
        let in_session = session_active.is_some_and(|a| a.load(Ordering::SeqCst));
        match action {
            ButtonAction::Place(target) if !in_session => win::place(&handle, &window, target),
            _ => win::hide(&window),
        }
    });
    if let Err(e) = posted {
        debug!("Could not update the floating button: {e}");
    }
}

#[cfg(windows)]
pub use watcher::{create_button_window, spawn_button_watcher};

#[cfg(windows)]
mod watcher {
    use super::{lock, next_action, post, ButtonState, ButtonTarget, BUTTON_SIZE, BUTTON_WINDOW};
    use crate::correction::settings::get_correction_settings;
    use crate::correction::{gpu_guard, text_focus};
    use log::{debug, warn};
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

    const POLL: Duration = Duration::from_millis(250);
    const SETTINGS_REFRESH: Duration = Duration::from_secs(2);
    /// Back-off after a failed tick, so a persistent fault can't spin.
    const PANIC_PAUSE: Duration = Duration::from_secs(5);

    /// Creates the hidden button window: transparent, undecorated, always on
    /// top, out of the taskbar and never focusable.
    pub fn create_button_window(app: &AppHandle) {
        let mut builder = WebviewWindowBuilder::new(
            app,
            BUTTON_WINDOW,
            WebviewUrl::App("src/correction-button/index.html".into()),
        )
        .title("Handy Plus")
        .resizable(false)
        .inner_size(BUTTON_SIZE, BUTTON_SIZE)
        .shadow(false)
        .maximizable(false)
        .minimizable(false)
        .closable(false)
        .accept_first_mouse(true)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .transparent(true)
        .focusable(false)
        .focused(false)
        .visible(false);
        if let Some(data_dir) = crate::portable::data_dir() {
            builder = builder.data_directory(data_dir.join("webview"));
        }
        match builder.build() {
            Ok(_) => debug!("Floating correction button window created (hidden)"),
            Err(e) => warn!("Could not create the floating correction button window: {e}"),
        }
    }

    /// Settings the watcher needs, re-read every [`SETTINGS_REFRESH`].
    #[derive(Default)]
    struct Cached {
        enabled: bool,
        excluded: Vec<String>,
        read_at: Option<Instant>,
    }

    /// Polls the focused field every [`POLL`] on its own thread and keeps the
    /// button on it. Nothing is polled while the button is off, during a
    /// session or in fullscreen.
    pub fn spawn_button_watcher(app: AppHandle, session_active: Arc<AtomicBool>) {
        let state = Arc::new(ButtonState::default());
        app.manage(state.clone());
        let spawned = std::thread::Builder::new()
            .name("correction-button".into())
            .spawn(move || {
                debug!("Floating correction button watcher started");
                let own_pid = std::process::id();
                let mut cached = Cached::default();
                while !state.stop.load(Ordering::SeqCst) {
                    std::thread::sleep(POLL);
                    let ticked = catch_unwind(AssertUnwindSafe(|| {
                        tick(&app, &state, &session_active, &mut cached, own_pid)
                    }));
                    if ticked.is_err() {
                        warn!("Floating button watcher tick failed; pausing");
                        std::thread::sleep(PANIC_PAUSE);
                    }
                }
                debug!("Floating correction button watcher stopped");
            });
        if let Err(e) = spawned {
            warn!("Could not start the floating button watcher: {e}");
        }
    }

    fn tick(
        app: &AppHandle,
        state: &ButtonState,
        session_active: &Arc<AtomicBool>,
        cached: &mut Cached,
        own_pid: u32,
    ) {
        if cached
            .read_at
            .is_none_or(|t| t.elapsed() >= SETTINGS_REFRESH)
        {
            let s = get_correction_settings(app);
            cached.enabled = s.floating_button;
            cached.excluded = s.button_excluded_apps;
            cached.read_at = Some(Instant::now());
        }
        let session = session_active.load(Ordering::SeqCst);
        let fullscreen = cached.enabled && !session && gpu_guard::fullscreen_app_active();
        // UI Automation polling makes Chromium/Electron apps build their full
        // accessibility trees, so it only runs while the button could show.
        let field = if cached.enabled && !session && !fullscreen {
            text_focus::focused_field()
        } else {
            None
        };
        let wanted = field
            .filter(|f| {
                text_focus::should_show_button(
                    Some(f),
                    cached.enabled,
                    session,
                    fullscreen,
                    &cached.excluded,
                    own_pid,
                )
            })
            .map(|f| ButtonTarget {
                hwnd: f.hwnd,
                rect: f.rect,
            });

        let mut shown = lock(&state.shown);
        // Checked under the lock `shutdown` takes, so nothing shows after exit.
        if state.stop.load(Ordering::SeqCst) {
            return;
        }
        if let Some(action) = next_action(*shown, wanted) {
            *shown = wanted;
            post(app, action, Some(session_active.clone()));
        }
    }
}

#[cfg(windows)]
mod win {
    use super::{button_bounds, to_logical, ButtonTarget};
    use log::{debug, warn};
    use tauri::{AppHandle, WebviewWindow};
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, ShowWindow, HWND_TOPMOST, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE,
    };

    /// Scale factor of the monitor that holds the middle of `rect`.
    fn monitor_scale(app: &AppHandle, rect: (i32, i32, i32, i32)) -> f64 {
        let (cx, cy) = (
            (i64::from(rect.0) + i64::from(rect.2)) / 2,
            (i64::from(rect.1) + i64::from(rect.3)) / 2,
        );
        let contains = |m: &tauri::Monitor| {
            let (p, s) = (m.position(), m.size());
            let (x, y) = (i64::from(p.x), i64::from(p.y));
            cx >= x && cy >= y && cx < x + i64::from(s.width) && cy < y + i64::from(s.height)
        };
        app.available_monitors()
            .ok()
            .and_then(|ms| ms.into_iter().find(contains))
            .or_else(|| app.primary_monitor().ok().flatten())
            .map_or(1.0, |m| m.scale_factor())
    }

    /// Moves the button onto `target` and shows it, without activating it.
    /// Bounds are physical pixels for the field's monitor (like the overlay),
    /// set twice: moving onto a monitor with another DPI makes tao rescale the
    /// window, and the second call puts it back.
    pub fn place(app: &AppHandle, window: &WebviewWindow, target: ButtonTarget) {
        let Ok(hwnd) = window.hwnd() else {
            return;
        };
        let scale = monitor_scale(app, target.rect);
        let text_scale = crate::overlay::windows_text_scale_factor();
        let (x, y, size) = button_bounds(target.rect, scale, text_scale);
        let topmost = Some(HWND_TOPMOST);
        // SAFETY: our own window handle; no pointers are passed.
        let placed = unsafe {
            SetWindowPos(hwnd, topmost, x, y, size, size, SWP_NOACTIVATE).and_then(|()| {
                SetWindowPos(
                    hwnd,
                    topmost,
                    x,
                    y,
                    size,
                    size,
                    SWP_NOACTIVATE | SWP_SHOWWINDOW,
                )
            })
        };
        if let Err(e) = placed {
            warn!("Could not place the floating button: {e}");
            return;
        }
        let (lx, ly) = to_logical((x, y), scale);
        debug!("Floating button at ({x}, {y}) px = ({lx:.0}, {ly:.0}) logical, scale {scale}");
    }

    pub fn hide(window: &WebviewWindow) {
        if let Ok(hwnd) = window.hwnd() {
            // SAFETY: our own window handle. The result is the previous
            // visibility, not an error.
            let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
        }
    }
}

#[cfg(not(windows))]
mod win {
    use super::ButtonTarget;
    use tauri::{AppHandle, WebviewWindow};

    pub fn place(_app: &AppHandle, _window: &WebviewWindow, _target: ButtonTarget) {}

    pub fn hide(window: &WebviewWindow) {
        let _ = window.hide();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: ButtonTarget = ButtonTarget {
        hwnd: 1,
        rect: (100, 100, 500, 200),
    };

    #[test]
    fn only_changes_reach_the_window() {
        assert_eq!(next_action(None, None), None);
        assert_eq!(next_action(Some(A), Some(A)), None);
        assert_eq!(next_action(None, Some(A)), Some(ButtonAction::Place(A)));
        assert_eq!(next_action(Some(A), None), Some(ButtonAction::Hide));
        let moved = ButtonTarget {
            rect: (100, 100, 600, 200),
            ..A
        };
        assert_eq!(
            next_action(Some(A), Some(moved)),
            Some(ButtonAction::Place(moved))
        );
        let other_window = ButtonTarget { hwnd: 2, ..A };
        assert_eq!(
            next_action(Some(A), Some(other_window)),
            Some(ButtonAction::Place(other_window))
        );
    }

    #[test]
    fn physical_to_logical_at_125_percent() {
        assert_eq!(to_logical((1250, 500), 1.25), (1000.0, 400.0));
        assert_eq!(to_logical((-2500, 0), 1.25), (-2000.0, 0.0));
        assert_eq!(to_logical((300, 200), 1.0), (300.0, 200.0));
    }

    #[test]
    fn bounds_scale_the_button_with_the_monitor_and_text_size() {
        // 100%: 28 px button, 6 px inset inside the bottom-right corner.
        assert_eq!(
            button_bounds((100, 100, 500, 200), 1.0, 1.0),
            (466, 166, 28)
        );
        // 125%: 35 px button, 8 px inset (7.5 rounded).
        assert_eq!(
            button_bounds((100, 100, 500, 200), 1.25, 1.0),
            (457, 157, 35)
        );
        // 100% display with 150% text: the page zooms, so the window grows too.
        assert_eq!(
            button_bounds((100, 100, 500, 200), 1.0, 1.5),
            (452, 152, 42)
        );
        // A box too small for the button gets it beside, bottom-aligned.
        assert_eq!(button_bounds((100, 100, 130, 120), 1.0, 1.0), (136, 92, 28));
    }
}
