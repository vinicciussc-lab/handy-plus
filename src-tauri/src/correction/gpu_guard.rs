//! Keeps the correction model from competing with games for the GPU.
//!
//! The model lives in a helper llama-server process; this module reads free
//! VRAM (NVML) and whether a game/fullscreen app is active, and a background
//! watcher unloads an idle model when either says the GPU is needed elsewhere.

use super::local_engine::LocalEngine;
use log::{debug, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Free VRAM required on top of the model size before loading it.
pub const LOAD_MARGIN_BYTES: u64 = 1_536 * 1024 * 1024;
/// Below this much free VRAM an idle model is unloaded.
pub const IDLE_MIN_FREE_BYTES: u64 = 1024 * 1024 * 1024;
/// What llama-server needs on top of the model file once running (KV cache
/// for `-c 4096` and compute buffers). Only a typing preload adds it to its
/// start check, so a preload that starts never trips the low VRAM rules by
/// itself.
pub const RUNTIME_OVERHEAD_BYTES: u64 = 1024 * 1024 * 1024;

/// When the GPU was last needed elsewhere (a fullscreen app or low VRAM
/// unloaded or cancelled the model). Typing preloads back off after it.
static GPU_PRESSURE_AT: Mutex<Option<Instant>> = Mutex::new(None);

/// Records that a game, a fullscreen app or low VRAM needed the GPU.
pub fn note_gpu_pressure() {
    *GPU_PRESSURE_AT.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
}

/// How long ago the GPU was last needed elsewhere, if ever.
pub fn since_gpu_pressure() -> Option<Duration> {
    GPU_PRESSURE_AT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .map(|at| at.elapsed())
}

const WATCH_INTERVAL: Duration = Duration::from_secs(2);

/// Free VRAM of the first NVIDIA GPU, or `None` when NVML is unavailable.
#[cfg(windows)]
pub fn free_vram_bytes() -> Option<u64> {
    use nvml_wrapper::Nvml;
    use std::sync::OnceLock;

    static NVML: OnceLock<Option<Nvml>> = OnceLock::new();
    let nvml = NVML
        .get_or_init(|| {
            // nvml.dll is loaded lazily; a missing driver is an Err (or, at
            // worst, a panic we must not let escape).
            match std::panic::catch_unwind(Nvml::init) {
                Ok(Ok(n)) => Some(n),
                Ok(Err(e)) => {
                    debug!("NVML unavailable: {e}");
                    None
                }
                Err(_) => None,
            }
        })
        .as_ref()?;
    nvml.device_by_index(0)
        .and_then(|d| d.memory_info())
        .map(|m| m.free)
        .ok()
}

#[cfg(not(windows))]
pub fn free_vram_bytes() -> Option<u64> {
    None
}

/// True while a game, a fullscreen app or a presentation owns the screen.
#[cfg(windows)]
pub fn fullscreen_app_active() -> bool {
    use windows::Win32::UI::Shell::{
        SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE,
        QUNS_RUNNING_D3D_FULL_SCREEN,
    };

    // SAFETY: no pointers involved; the call only reads shell state.
    match unsafe { SHQueryUserNotificationState() } {
        Ok(state) => {
            state == QUNS_BUSY
                || state == QUNS_RUNNING_D3D_FULL_SCREEN
                || state == QUNS_PRESENTATION_MODE
        }
        Err(_) => false,
    }
}

#[cfg(not(windows))]
pub fn fullscreen_app_active() -> bool {
    false
}

/// Whether the model fits in free VRAM with margin. Unknown free VRAM never blocks.
pub fn has_room(free: Option<u64>, model_bytes: u64) -> bool {
    match free {
        Some(free) => free >= model_bytes.saturating_add(LOAD_MARGIN_BYTES),
        None => true,
    }
}

/// Whether an idle loaded model should be dropped to give the GPU back.
pub fn should_unload(
    loaded: bool,
    session_active: bool,
    fullscreen: bool,
    free: Option<u64>,
) -> bool {
    if !loaded || session_active {
        return false;
    }
    fullscreen || free.is_some_and(|f| f < IDLE_MIN_FREE_BYTES)
}

/// Whether a typing preload still loading should be dropped because a game,
/// a fullscreen app or low VRAM needs the GPU. The watcher only sees ready
/// models, so the preload checks this itself; a session that joined the load
/// keeps it.
pub fn should_abort_preload(session_active: bool, fullscreen: bool, free: Option<u64>) -> bool {
    should_unload(true, session_active, fullscreen, free)
}

/// One watcher pass.
fn tick(
    engine: &LocalEngine,
    session_active: &AtomicBool,
    unload_after_secs: &impl Fn() -> Option<u64>,
) {
    if session_active.load(Ordering::SeqCst) || !engine.is_loaded() {
        return; // nothing to unload, or a correction is in progress
    }
    if should_unload(true, false, fullscreen_app_active(), free_vram_bytes()) {
        note_gpu_pressure();
        if engine.unload_unless_busy(session_active) {
            debug!("GPU guard: unloaded correction model (GPU needed elsewhere)");
        }
    } else if let (Some(idle), Some(limit)) = (engine.idle_for(), unload_after_secs()) {
        if idle >= Duration::from_secs(limit) && engine.unload_unless_busy(session_active) {
            debug!("GPU guard: unloaded idle correction model");
        }
    }
}

/// Starts the watcher thread (lives as long as the app). Every 2 s it unloads
/// the model for fullscreen apps / low VRAM, or after the idle timeout
/// (`None` = never, `0` = immediately). Never touches a model in use.
///
/// Contract for the session owner: set `session_active = true` (SeqCst) BEFORE
/// calling `ensure_loaded`, and call `engine.touch()` when the session ends,
/// before clearing the flag. The watcher unloads via
/// `LocalEngine::unload_unless_busy`, so it either sees the flag and skips, or
/// unloads first and `ensure_loaded` simply reloads.
pub fn spawn_watcher(
    engine: Arc<LocalEngine>,
    session_active: Arc<AtomicBool>,
    unload_after_secs: impl Fn() -> Option<u64> + Send + 'static,
) {
    let spawned = std::thread::Builder::new()
        .name("correction-gpu-guard".into())
        .spawn(move || loop {
            std::thread::sleep(WATCH_INTERVAL);
            let pass = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                tick(&engine, &session_active, &unload_after_secs)
            }));
            if pass.is_err() {
                warn!("GPU guard pass panicked; continuing");
            }
        });
    if let Err(e) = spawned {
        warn!("could not start GPU guard thread: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Manual sanity check: `cargo test gpu_guard_readings -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn gpu_guard_readings() {
        println!("free_vram_bytes = {:?}", free_vram_bytes());
        println!("fullscreen_app_active = {}", fullscreen_app_active());
    }

    #[test]
    fn room_needs_model_plus_margin() {
        let gb = 1024u64.pow(3);
        assert!(has_room(Some(7 * gb), 5 * gb));
        assert!(!has_room(Some(6 * gb), 5 * gb));
        assert!(has_room(None, 5 * gb)); // sem NVML não bloqueia
    }

    #[test]
    fn typing_preload_stops_for_a_game_unless_a_session_joined() {
        let gb = 1024u64.pow(3);
        assert!(should_abort_preload(false, true, Some(8 * gb)));
        assert!(should_abort_preload(false, false, Some(gb / 2)));
        assert!(
            !should_abort_preload(true, true, Some(0)),
            "a session joined"
        );
        assert!(!should_abort_preload(false, false, Some(8 * gb)));
        assert!(!should_abort_preload(false, false, None), "no NVML");
    }

    #[test]
    fn unloads_idle_model_for_fullscreen_or_low_memory_only() {
        let gb = 1024u64.pow(3);
        assert!(should_unload(true, false, true, Some(8 * gb)));
        assert!(should_unload(true, false, false, Some(gb / 2)));
        assert!(!should_unload(true, true, true, Some(0))); // correção em andamento
        assert!(!should_unload(false, false, true, Some(0))); // nada carregado
        assert!(!should_unload(true, false, false, Some(8 * gb)));
    }
}
