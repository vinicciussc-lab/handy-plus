//! The typing hint: after the user types a few words in any program and
//! pauses, a small pill where Handy's overlay pill lives offers the correction
//! shortcut. At most once per window. The same count, without the pause,
//! starts loading the local model in the background (also once per window),
//! so it is ready when the shortcut comes. Keys are only counted; which keys
//! were typed is never kept.

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

/// Counts typing keys in the foreground window and says when to show the hint
/// and when to preload the correction model.
#[derive(Default)]
pub struct HintTracker {
    /// Window the current count belongs to.
    window: Option<isize>,
    count: u32,
    last_key_ms: u64,
    /// Windows that already got the hint.
    hinted: HashSet<isize>,
    /// Keys toward the typing preload, counted apart from the hint's (the
    /// hint resets its own count when it shows).
    preload_keys: u32,
    /// Windows whose typing already started a preload.
    preloaded: HashSet<isize>,
}

impl HintTracker {
    /// A typing key went down in `hwnd` (the foreground window).
    pub fn on_typing_key(&mut self, hwnd: isize, now_ms: u64) {
        if self.window != Some(hwnd) {
            self.window = Some(hwnd);
            self.count = 0;
            self.preload_keys = 0;
        }
        self.count = self.count.saturating_add(1);
        self.preload_keys = self.preload_keys.saturating_add(1);
        self.last_key_ms = now_ms;
    }

    /// The window whose typing should start a preload now, if nothing blocks
    /// it: enough keys there (no pause needed) and none started for it yet.
    pub fn preload_due(&self) -> Option<isize> {
        self.window
            .filter(|w| self.preload_keys >= HINT_MIN_KEYS && !self.preloaded.contains(w))
    }

    /// The preload for `hwnd` was checked. The count starts over, so a blocked
    /// check is tried again only after more typing; a started one marks the
    /// window so it never preloads again.
    pub fn preload_checked(&mut self, hwnd: isize, started: bool) {
        if self.window == Some(hwnd) {
            self.preload_keys = 0;
        }
        if started {
            self.preloaded.insert(hwnd);
        }
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

/// What decides whether typing starts loading the local model, read when the
/// preload is due.
#[derive(Clone, Debug)]
pub struct PreloadCheck {
    /// `preload_on_typing` is on.
    pub enabled: bool,
    /// The provider is the local model.
    pub local_provider: bool,
    /// A local model is chosen and its file exists.
    pub model_exists: bool,
    /// The engine already serves that model, so there is nothing to load.
    pub engine_ready: bool,
    /// A typing preload is already loading.
    pub in_flight: bool,
    /// The model unloads as soon as it is idle, so a preload would be wasted.
    pub unload_immediately: bool,
    pub fullscreen: bool,
    pub dictation_busy: bool,
    pub session_active: bool,
    /// The keys went to Handy Plus itself.
    pub own_window: bool,
    pub free_vram: Option<u64>,
    pub model_bytes: u64,
}

/// Every condition but GPU room allows the preload (low VRAM is told apart
/// so it can be logged).
pub fn preload_wanted(c: &PreloadCheck) -> bool {
    c.enabled
        && c.local_provider
        && c.model_exists
        && !c.engine_ready
        && !c.in_flight
        && !c.unload_immediately
        && !c.fullscreen
        && !c.dictation_busy
        && !c.session_active
        && !c.own_window
}

/// Typing should start loading the local model now.
pub fn should_preload(c: &PreloadCheck) -> bool {
    preload_wanted(c) && super::gpu_guard::has_room(c.free_vram, c.model_bytes)
}

/// A session opening must check free VRAM before loading only when the model
/// is neither served nor already being loaded by a typing preload (that load
/// is what is using the VRAM; the session waits for it on the engine's load
/// lock and then reuses the server).
pub fn session_needs_room_check(served: bool, preloading_same_model: bool) -> bool {
    !served && !preloading_same_model
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
            // Nothing would show, so the window must not count as hinted.
            || settings::get_settings(app).overlay_style == settings::OverlayStyle::None
            || gpu_guard::fullscreen_app_active()
            || session::dictation_busy(app)
            || session::session_active(app)
            || overlay::overlay_in_use()
    }

    /// Starts loading the correction model once enough keys were typed in a
    /// window (see [`super::should_preload`]); once per window.
    fn preload(app: &AppHandle, hint: &Mutex<Hint>) {
        let Some(window) = lock(hint).tracker.preload_due() else {
            return;
        };
        // The user left the window without typing there: try again later.
        let started = foreground_window() == Some(window)
            && session::try_typing_preload(app, is_own_window(window));
        lock(hint).tracker.preload_checked(window, started);
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
                preload(&app, &hint);
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
    fn preload_is_due_at_twelve_keys_without_a_pause() {
        let mut t = HintTracker::default();
        type_keys(&mut t, W, HINT_MIN_KEYS - 1, 0);
        assert_eq!(t.preload_due(), None);
        t.on_typing_key(W, 5_000);
        // No pause needed, unlike the hint.
        assert_eq!(t.preload_due(), Some(W));
    }

    #[test]
    fn preload_once_per_window_only_when_it_started() {
        let mut t = HintTracker::default();
        let last = type_keys(&mut t, W, HINT_MIN_KEYS, 0);
        // Blocked: the window is not marked, the count starts over.
        t.preload_checked(W, false);
        assert_eq!(t.preload_due(), None);
        let last = type_keys(&mut t, W, HINT_MIN_KEYS, last + 100);
        assert_eq!(t.preload_due(), Some(W));
        t.preload_checked(W, true);
        let last = type_keys(&mut t, W, 40, last + 100);
        assert_eq!(t.preload_due(), None, "already preloaded for this window");
        // Another window can still preload.
        type_keys(&mut t, OTHER, HINT_MIN_KEYS, last + 100);
        assert_eq!(t.preload_due(), Some(OTHER));
    }

    #[test]
    fn preload_count_is_separate_from_the_hint() {
        let mut t = HintTracker::default();
        let last = type_keys(&mut t, W, 8, 0);
        let last = type_keys(&mut t, OTHER, 8, last + 100);
        assert_eq!(
            t.preload_due(),
            None,
            "switching windows restarts the count"
        );
        // The hint showing (and resetting its count) does not reset the preload's.
        let last = type_keys(&mut t, OTHER, 2, last + 100);
        assert_eq!(t.poll(last + HINT_IDLE_MS, false), None);
        type_keys(&mut t, OTHER, 2, last + 3000);
        assert_eq!(t.preload_due(), Some(OTHER));
        // A check for a window the user already left changes nothing.
        t.preload_checked(W, true);
        assert_eq!(t.preload_due(), Some(OTHER));
    }

    fn ready_check() -> PreloadCheck {
        PreloadCheck {
            enabled: true,
            local_provider: true,
            model_exists: true,
            engine_ready: false,
            in_flight: false,
            unload_immediately: false,
            fullscreen: false,
            dictation_busy: false,
            session_active: false,
            own_window: false,
            free_vram: Some(16 * GB),
            model_bytes: 5 * GB,
        }
    }

    const GB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn preloads_when_nothing_blocks() {
        assert!(should_preload(&ready_check()));
        // Unknown free VRAM (no NVML) never blocks, as for a session.
        let c = PreloadCheck {
            free_vram: None,
            ..ready_check()
        };
        assert!(should_preload(&c));
    }

    #[test]
    fn each_condition_blocks_the_preload() {
        type Block = fn(&mut PreloadCheck);
        let blocked: [(&str, Block); 12] = [
            ("disabled", |c| c.enabled = false),
            ("cloud provider", |c| c.local_provider = false),
            ("no model file", |c| c.model_exists = false),
            ("already loaded", |c| c.engine_ready = true),
            ("preload in flight", |c| c.in_flight = true),
            ("unloads immediately", |c| c.unload_immediately = true),
            ("fullscreen", |c| c.fullscreen = true),
            ("dictation", |c| c.dictation_busy = true),
            ("session open", |c| c.session_active = true),
            ("own window", |c| c.own_window = true),
            ("low VRAM", |c| c.free_vram = Some(6 * GB)),
            ("no VRAM", |c| c.free_vram = Some(0)),
        ];
        for (why, block) in blocked {
            let mut c = ready_check();
            block(&mut c);
            assert!(!should_preload(&c), "{why} must block the preload");
        }
    }

    #[test]
    fn low_vram_is_the_only_room_reason() {
        let c = PreloadCheck {
            free_vram: Some(GB),
            ..ready_check()
        };
        // Everything else allows it: this is the case logged as "low VRAM".
        assert!(preload_wanted(&c));
        assert!(!should_preload(&c));
        let c = PreloadCheck {
            fullscreen: true,
            ..ready_check()
        };
        assert!(!preload_wanted(&c));
    }

    #[test]
    fn no_second_load_while_one_runs() {
        // A typing preload never starts over a load in flight or a ready server,
        // so with the engine's load lock at most one server ever loads.
        for (in_flight, engine_ready) in [(true, false), (false, true), (true, true)] {
            let c = PreloadCheck {
                in_flight,
                engine_ready,
                ..ready_check()
            };
            assert!(!should_preload(&c));
        }
        // A session opening mid-preload skips the VRAM check (the preload is
        // already using it) and joins the same load instead of falling back.
        assert!(!session_needs_room_check(false, true));
        assert!(!session_needs_room_check(true, false));
        assert!(session_needs_room_check(false, false));
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
