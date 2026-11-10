//! The typing hint: after the user types a few words in any program and
//! pauses, a small pill where Handy's overlay pill lives offers the correction
//! shortcut. At most once per window. Keys are only counted; which keys were
//! typed is never kept.

use std::collections::HashSet;

#[cfg(windows)]
pub use runtime::start;

/// No hooks outside Windows, so no hint.
#[cfg(not(windows))]
pub fn start(_app: &tauri::AppHandle) {}

/// Typing keys needed in one window before the hint can show.
pub const HINT_MIN_KEYS: u32 = 12;
/// Pause after the last key before the hint shows.
pub const HINT_IDLE_MS: u64 = 1500;
/// How long the hint stays up.
pub const HINT_SHOW_MS: u64 = 4000;

/// Counts typing keys in the foreground window and says when to show the hint.
#[derive(Default)]
pub struct HintTracker {
    /// Window the current count belongs to.
    window: Option<isize>,
    count: u32,
    last_key_ms: u64,
    /// Windows that already got the hint.
    hinted: HashSet<isize>,
}

impl HintTracker {
    /// A typing key went down in `hwnd` (the foreground window).
    pub fn on_typing_key(&mut self, hwnd: isize, now_ms: u64) {
        if self.window != Some(hwnd) {
            self.window = Some(hwnd);
            self.count = 0;
        }
        self.count = self.count.saturating_add(1);
        self.last_key_ms = now_ms;
    }

    /// Window the keys are being counted in.
    pub fn window(&self) -> Option<isize> {
        self.window
    }

    /// The hint would show now if nothing blocks it.
    pub fn due(&self, now_ms: u64) -> bool {
        self.window.is_some_and(|w| {
            self.count >= HINT_MIN_KEYS
                && now_ms.saturating_sub(self.last_key_ms) >= HINT_IDLE_MS
                && !self.hinted.contains(&w)
        })
    }

    /// Called every ~250 ms; returns Some(hwnd) when the hint should show now.
    /// A blocked hint is dropped (the count restarts) without marking the window.
    pub fn poll(&mut self, now_ms: u64, blocked: bool) -> Option<isize> {
        if !self.due(now_ms) {
            return None;
        }
        self.count = 0;
        if blocked {
            return None;
        }
        let window = self.window?;
        self.hinted.insert(window);
        Some(window)
    }
}

#[cfg(windows)]
mod runtime {
    use super::{HintTracker, HINT_SHOW_MS};
    use crate::correction::input_hooks::{self, HookEvent};
    use crate::correction::settings::get_correction_settings;
    use crate::correction::{gpu_guard, session};
    use crate::{overlay, settings};
    use log::{debug, error};
    use std::sync::{Arc, Mutex, MutexGuard};
    use std::time::{Duration, Instant};
    use tauri::AppHandle;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    const POLL_INTERVAL: Duration = Duration::from_millis(250);

    #[derive(Default)]
    struct Hint {
        tracker: HintTracker,
        /// When the hint went up, while it is up.
        shown_at: Option<u64>,
    }

    fn lock(m: &Mutex<Hint>) -> MutexGuard<'_, Hint> {
        m.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn foreground_window() -> Option<isize> {
        let hwnd = unsafe { GetForegroundWindow() };
        (!hwnd.0.is_null()).then_some(hwnd.0 as isize)
    }

    /// The window belongs to Handy Plus itself.
    fn is_own_window(hwnd: isize) -> bool {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(HWND(hwnd as *mut _), Some(&mut pid)) };
        pid == std::process::id()
    }

    /// The correction shortcut as stored ("ctrl+section"); the overlay formats it.
    fn shortcut_binding(app: &AppHandle) -> Option<String> {
        settings::get_settings(app)
            .bindings
            .get("correct_selection")
            .map(|b| b.current_binding.clone())
            .filter(|b| !b.trim().is_empty())
    }

    /// Something keeps the hint from showing for the keys typed in `window`.
    fn blocked(app: &AppHandle, window: Option<isize>) -> bool {
        let foreground = foreground_window();
        // The user moved to another window without typing there.
        foreground.is_none()
            || foreground != window
            || foreground.is_some_and(is_own_window)
            || !get_correction_settings(app).typing_hint
            || gpu_guard::fullscreen_app_active()
            || session::dictation_busy(app)
            || session::session_active(app)
            || overlay::overlay_in_use()
    }

    /// Counts typing keys (never which ones) and shows the hint after a pause.
    pub fn start(app: &AppHandle) {
        let hint = Arc::new(Mutex::new(Hint::default()));
        let clock = Instant::now();
        let now_ms = move || clock.elapsed().as_millis() as u64;

        let (keys_app, keys_hint) = (app.clone(), hint.clone());
        input_hooks::start_hooks(move |ev| {
            let HookEvent::TypingKey { .. } = ev else {
                return;
            };
            // Read here, on the dispatcher thread, never in the hook itself.
            let Some(hwnd) = foreground_window() else {
                return;
            };
            let mut h = lock(&keys_hint);
            h.tracker.on_typing_key(hwnd, now_ms());
            // Typing again puts the hint away.
            if h.shown_at.take().is_some() {
                overlay::hide_hint_overlay(&keys_app);
            }
        });

        let app = app.clone();
        let poller = std::thread::Builder::new()
            .name("typing-hint".into())
            .spawn(move || loop {
                std::thread::sleep(POLL_INTERVAL);
                let now = now_ms();
                let (due, window) = {
                    let mut h = lock(&hint);
                    if h.shown_at
                        .is_some_and(|at| now.saturating_sub(at) >= HINT_SHOW_MS)
                    {
                        h.shown_at = None;
                        overlay::hide_hint_overlay(&app);
                    }
                    (h.tracker.due(now), h.tracker.window())
                };
                if !due {
                    continue;
                }
                // Settings and system checks only when the hint is due.
                let binding = shortcut_binding(&app);
                let blocked = binding.is_none() || blocked(&app, window);
                let mut h = lock(&hint);
                if h.tracker.poll(now, blocked).is_some() {
                    if let Some(binding) = binding {
                        debug!("Typing hint shown");
                        overlay::show_hint_overlay(&app, binding);
                        h.shown_at = Some(now);
                    }
                }
            });
        if let Err(e) = poller {
            error!("Could not start the typing hint: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: isize = 0x1001;
    const OTHER: isize = 0x2002;

    /// Types `n` keys in `hwnd`, 100 ms apart, starting at `start`; returns the
    /// time of the last key.
    fn type_keys(t: &mut HintTracker, hwnd: isize, n: u32, start: u64) -> u64 {
        let mut now = start;
        for i in 0..n {
            now = start + u64::from(i) * 100;
            t.on_typing_key(hwnd, now);
        }
        now
    }

    #[test]
    fn shows_after_enough_keys_and_a_pause() {
        let mut t = HintTracker::default();
        let last = type_keys(&mut t, W, HINT_MIN_KEYS, 0);
        assert!(t.due(last + HINT_IDLE_MS));
        assert_eq!(t.poll(last + HINT_IDLE_MS, false), Some(W));
    }

    #[test]
    fn not_before_twelve_keys() {
        let mut t = HintTracker::default();
        let last = type_keys(&mut t, W, HINT_MIN_KEYS - 1, 0);
        assert!(!t.due(last + 10_000));
        assert_eq!(t.poll(last + 10_000, false), None);
    }

    #[test]
    fn not_before_the_pause() {
        let mut t = HintTracker::default();
        let last = type_keys(&mut t, W, 30, 0);
        assert_eq!(t.poll(last + HINT_IDLE_MS - 1, false), None);
        assert_eq!(t.poll(last + HINT_IDLE_MS, false), Some(W));
    }

    #[test]
    fn once_per_window() {
        let mut t = HintTracker::default();
        let last = type_keys(&mut t, W, 20, 0);
        assert_eq!(t.poll(last + 2000, false), Some(W));
        let last = type_keys(&mut t, W, 40, last + 3000);
        assert_eq!(t.poll(last + 2000, false), None);
        assert!(!t.due(last + 2000));
        // Another window still gets it.
        let last = type_keys(&mut t, OTHER, 20, last + 3000);
        assert_eq!(t.poll(last + 2000, false), Some(OTHER));
    }

    #[test]
    fn switching_windows_restarts_the_count() {
        let mut t = HintTracker::default();
        let last = type_keys(&mut t, W, 8, 0);
        let last = type_keys(&mut t, OTHER, 8, last + 100);
        assert_eq!(t.window(), Some(OTHER));
        assert_eq!(t.poll(last + 2000, false), None);
        // Back to W: the earlier 8 keys there do not count either.
        let last = type_keys(&mut t, W, 8, last + 3000);
        assert_eq!(t.poll(last + 2000, false), None);
        let last = type_keys(&mut t, W, 4, last + 100);
        assert_eq!(t.poll(last + 2000, false), Some(W));
    }

    #[test]
    fn blocked_does_not_show_or_mark_the_window() {
        let mut t = HintTracker::default();
        let last = type_keys(&mut t, W, 20, 0);
        assert_eq!(t.poll(last + 2000, true), None);
        // The blocked attempt is dropped, not shown later by itself.
        assert_eq!(t.poll(last + 2300, false), None);
        // The window was not marked: typing again there can show it.
        let last = type_keys(&mut t, W, 12, last + 5000);
        assert_eq!(t.poll(last + 2000, false), Some(W));
    }

    #[test]
    fn a_new_key_postpones_the_hint() {
        let mut t = HintTracker::default();
        let last = type_keys(&mut t, W, 12, 0);
        t.on_typing_key(W, last + 1400);
        assert_eq!(t.poll(last + 1500, false), None);
        assert_eq!(t.poll(last + 1400 + HINT_IDLE_MS, false), Some(W));
    }
}
