//! One text correction, from the `correct_selection` shortcut to the paste.
//!
//! The decisions (which key does what in which phase, which keys to listen
//! to) are pure functions at the top. The session itself runs as an async
//! task that copies the selection, picks the model, streams the correction
//! into the overlay panel via [`CorrectionUpdateEvent`] and waits for Enter,
//! a digit or Esc, which arrive through temporary global shortcuts.

use super::local_engine::{EngineError, LocalEngine};
use super::selection::{self, CaptureError};
use super::settings::{
    default_tones, get_correction_settings, write_correction_settings, CloudModel,
    CorrectionProvider, CorrectionSettings, CorrectionTone,
};
use super::stream::{stream_chat, ChatTarget};
use super::text::{build_messages, clean_output, generation_progress, load_progress};
use super::{gpu_guard, text::MAX_CORRECTION_CHARS};
use crate::actions::ShortcutAction;
use crate::managers::audio::AudioRecordingManager;
use crate::settings::{get_settings, PostProcessApiFormat, ShortcutBinding};
use crate::{overlay, shortcut};
use futures_util::future::{select, Either};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tauri_specta::Event;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

/// Minimum time between two streaming updates sent to the panel.
const EMIT_INTERVAL: Duration = Duration::from_millis(50);
/// How often the loading bar is refreshed.
const TICK: Duration = Duration::from_millis(200);
const ERROR_HOLD: Duration = Duration::from_secs(3);
const APPLIED_HOLD: Duration = Duration::from_secs(2);
const COPIED_HOLD: Duration = Duration::from_secs(3);
/// Pause after bringing the original window back, before pasting.
const SETTLE_DELAY: Duration = Duration::from_millis(80);
/// A preview nobody answers is dropped, so a failed key registration can't
/// leave the session (and the shortcut) stuck forever.
const CHOICE_TIMEOUT: Duration = Duration::from_secs(300);
/// The first request after a load is very slow, so a tiny one goes first.
const WARMUP_SYSTEM: &str = "Responda só OK.";
const WARMUP_USER: &str = "oi";

const CANCEL_ID: &str = "correction_cancel";
const APPLY_ID: &str = "correction_apply";
const TONE_PREFIX: &str = "correction_tone_";
/// Temporary shortcuts, registered only while a session needs them. They are
/// not in `AppSettings.bindings`, so they never show on the shortcuts screen
/// and startup/suspend/resume never touch them.
pub const TEMPORARY_BINDING_IDS: &[&str] = &[
    CANCEL_ID,
    APPLY_ID,
    "correction_tone_1",
    "correction_tone_2",
    "correction_tone_3",
    "correction_tone_4",
    "correction_tone_5",
    "correction_tone_6",
    "correction_tone_7",
    "correction_tone_8",
    "correction_tone_9",
];

#[derive(Clone, Debug, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionPhase {
    Loading,
    Generating,
    Preview,
    NeedsFallback,
    Applied,
    /// The original window could not be brought back: the correction is on
    /// the clipboard for the user to paste.
    Copied,
    Error,
    Hidden,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionErrorCode {
    NothingSelected,
    TextTooLong,
    NoModelChosen,
    ModelMissing,
    EngineFailed,
    CloudFailed,
    /// Modifier keys were still held, so the selection was not copied.
    KeysHeld,
    ClipboardFailed,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FallbackOption {
    Cloud,
    Cpu,
}

/// Everything the correction panel shows. Sent whenever it changes.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct CorrectionUpdateEvent {
    pub phase: CorrectionPhase,
    pub original: String,
    pub text: String,
    pub load_progress: Option<f32>,
    pub gen_progress: Option<f32>,
    pub tones: Vec<String>,
    pub tone_index: u32,
    pub model_label: String,
    pub elapsed_ms: Option<u64>,
    pub error: Option<CorrectionErrorCode>,
    pub fallback_options: Vec<FallbackOption>,
}

impl CorrectionUpdateEvent {
    fn hidden() -> Self {
        Self {
            phase: CorrectionPhase::Hidden,
            original: String::new(),
            text: String::new(),
            load_progress: None,
            gen_progress: None,
            tones: Vec::new(),
            tone_index: 0,
            model_label: String::new(),
            elapsed_ms: None,
            error: None,
            fallback_options: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionInput {
    Escape,
    Enter,
    Digit(u8),
}

#[derive(Clone, Debug, PartialEq)]
pub enum SessionCommand {
    Cancel,
    Apply,
    Regenerate(usize),
    UseFallback(FallbackOption),
    Ignore,
}

/// What a key means in a phase. Digits pick a tone in the preview (only the
/// first nine tones have one) and an option in the fallback prompt.
pub fn decide(
    phase: &CorrectionPhase,
    input: SessionInput,
    tones_len: usize,
    fallback: &[FallbackOption],
) -> SessionCommand {
    use CorrectionPhase as P;
    match (phase, input) {
        (P::Loading | P::Generating | P::Preview | P::NeedsFallback, SessionInput::Escape) => {
            SessionCommand::Cancel
        }
        (P::Preview, SessionInput::Enter) => SessionCommand::Apply,
        (P::Preview, SessionInput::Digit(d)) if d >= 1 && usize::from(d) <= tones_len.min(9) => {
            SessionCommand::Regenerate(usize::from(d) - 1)
        }
        (P::NeedsFallback, SessionInput::Enter) => {
            fallback.first().map_or(SessionCommand::Ignore, |o| {
                SessionCommand::UseFallback(o.clone())
            })
        }
        (P::NeedsFallback, SessionInput::Digit(d)) if d >= 1 => fallback
            .get(usize::from(d) - 1)
            .map_or(SessionCommand::Ignore, |o| {
                SessionCommand::UseFallback(o.clone())
            }),
        _ => SessionCommand::Ignore,
    }
}

/// The shortcut is ignored while a dictation is anywhere in its pipeline
/// (recording, transcribing, post-processing or pasting) and while a
/// correction runs.
pub fn should_start(dictation_busy: bool, session_active: bool) -> bool {
    !dictation_busy && !session_active
}

pub fn check_text(text: &str) -> Result<(), CorrectionErrorCode> {
    if selection::is_blank_selection(text) {
        return Err(CorrectionErrorCode::NothingSelected);
    }
    if text.chars().count() > MAX_CORRECTION_CHARS {
        return Err(CorrectionErrorCode::TextTooLong);
    }
    Ok(())
}

/// Temporary bindings a phase listens to: Esc while working; Esc, Enter and
/// one digit per choice while waiting for an answer; nothing otherwise.
pub fn wanted_bindings(phase: &CorrectionPhase, choices: usize) -> Vec<String> {
    use CorrectionPhase as P;
    match phase {
        P::Loading | P::Generating => vec![CANCEL_ID.to_string()],
        P::Preview | P::NeedsFallback => [CANCEL_ID.to_string(), APPLY_ID.to_string()]
            .into_iter()
            .chain((1..=choices.min(9)).map(|n| format!("{TONE_PREFIX}{n}")))
            .collect(),
        _ => Vec::new(),
    }
}

fn tone_digit(id: &str) -> Option<u8> {
    let digit: u8 = id.strip_prefix(TONE_PREFIX)?.parse().ok()?;
    (1..=9).contains(&digit).then_some(digit)
}

/// Key of a temporary binding, in the syntax both shortcut backends parse.
pub fn binding_key(id: &str) -> Option<String> {
    match id {
        CANCEL_ID => Some("escape".to_string()),
        APPLY_ID => Some("enter".to_string()),
        _ => tone_digit(id).map(|d| d.to_string()),
    }
}

pub fn input_for_binding(id: &str) -> Option<SessionInput> {
    match id {
        CANCEL_ID => Some(SessionInput::Escape),
        APPLY_ID => Some(SessionInput::Enter),
        _ => tone_digit(id).map(SessionInput::Digit),
    }
}

fn capture_error_code(e: CaptureError) -> CorrectionErrorCode {
    match e {
        CaptureError::NothingSelected => CorrectionErrorCode::NothingSelected,
        CaptureError::KeysHeld => CorrectionErrorCode::KeysHeld,
        CaptureError::Clipboard(e) => {
            warn!("Could not copy the selection: {e}");
            CorrectionErrorCode::ClipboardFailed
        }
    }
}

fn engine_error_code(e: EngineError) -> CorrectionErrorCode {
    match e {
        EngineError::ModelMissing(_) => CorrectionErrorCode::ModelMissing,
        _ => CorrectionErrorCode::EngineFailed,
    }
}

/// Ways out when the GPU has no room: the backup cloud model (if set) and the CPU.
fn fallback_options(cloud_configured: bool) -> Vec<FallbackOption> {
    let cloud = cloud_configured.then_some(FallbackOption::Cloud);
    cloud.into_iter().chain([FallbackOption::Cpu]).collect()
}

fn session_tones(s: &CorrectionSettings) -> Vec<CorrectionTone> {
    if s.tones.is_empty() {
        default_tones()
    } else {
        s.tones.clone()
    }
}

/// Runs `work` until it finishes, calling `on_tick` every [`TICK`] meanwhile.
/// Esc drops `work` (aborting a stream or a load) and returns `None`; other
/// keys are ignored.
async fn until_cancelled<F: Future>(
    work: F,
    inputs: &mut UnboundedReceiver<SessionInput>,
    phase: &CorrectionPhase,
    mut on_tick: impl FnMut(),
) -> Option<F::Output> {
    let mut work = std::pin::pin!(work);
    let mut inputs_open = true;
    loop {
        let (open, rx) = (inputs_open, &mut *inputs);
        let next_input = std::pin::pin!(async move {
            if open {
                rx.recv().await
            } else {
                std::future::pending().await
            }
        });
        let tick = std::pin::pin!(tokio::time::sleep(TICK));
        let wake = select(next_input, tick);
        match select(work.as_mut(), wake).await {
            Either::Left((out, _)) => return Some(out),
            Either::Right((Either::Left((Some(input), _)), _)) => {
                if decide(phase, input, 0, &[]) == SessionCommand::Cancel {
                    return None;
                }
            }
            Either::Right((Either::Left((None, _)), _)) => inputs_open = false,
            Either::Right((Either::Right(_), _)) => on_tick(),
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// App state for corrections. `active` doubles as the GPU watcher's
/// "session in progress" flag.
#[derive(Default)]
pub struct CorrectionSessionState {
    active: Arc<AtomicBool>,
    inputs: Mutex<Option<UnboundedSender<SessionInput>>>,
    /// Temporary bindings currently registered with the shortcut backend.
    registered: Mutex<Vec<String>>,
}

/// Creates the local engine and session state and starts the GPU watcher.
pub fn init(app: &AppHandle) {
    let engine = Arc::new(LocalEngine::new(engine_binary(app)));
    let state = CorrectionSessionState::default();
    let handle = app.clone();
    gpu_guard::spawn_watcher(engine.clone(), state.active.clone(), move || {
        get_correction_settings(&handle).unload_timeout.to_seconds()
    });
    app.manage(engine);
    app.manage(state);
}

/// The bundled llama-server, or the copy in the source tree during development.
fn engine_binary(app: &AppHandle) -> PathBuf {
    const REL: &str = "resources/llama-server/llama-server.exe";
    let bundled = app
        .path()
        .resolve(REL, tauri::path::BaseDirectory::Resource);
    match bundled {
        Ok(path) if path.is_file() => path,
        _ => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(REL),
    }
}

/// Stops the local model (app exit).
pub fn shutdown(app: &AppHandle) {
    if let Some(engine) = app.try_state::<Arc<LocalEngine>>() {
        engine.unload();
    }
}

/// `correct_selection`: starts a session when the shortcut is released.
struct CorrectSelectionAction;

impl ShortcutAction for CorrectSelectionAction {
    fn start(&self, _app: &AppHandle, _binding_id: &str, _shortcut_str: &str) {}

    fn stop(&self, app: &AppHandle, _binding_id: &str, _shortcut_str: &str) {
        begin(app);
    }
}

/// Esc, Enter and the digits while a session listens for them.
struct CorrectionKeyAction;

impl ShortcutAction for CorrectionKeyAction {
    fn start(&self, app: &AppHandle, binding_id: &str, _shortcut_str: &str) {
        if let Some(i) = input_for_binding(binding_id) {
            input(app, i);
        }
    }

    fn stop(&self, _app: &AppHandle, _binding_id: &str, _shortcut_str: &str) {}
}

/// Adds the correction actions to `ACTION_MAP`.
pub fn register_actions(map: &mut HashMap<String, Arc<dyn ShortcutAction>>) {
    map.insert(
        "correct_selection".to_string(),
        Arc::new(CorrectSelectionAction),
    );
    for id in TEMPORARY_BINDING_IDS {
        map.insert(id.to_string(), Arc::new(CorrectionKeyAction));
    }
}

/// Forwards a key to the running session. Never blocks: it runs on the
/// shortcut backend's thread.
pub fn input(app: &AppHandle, input: SessionInput) {
    if let Some(state) = app.try_state::<CorrectionSessionState>() {
        if let Some(tx) = lock(&state.inputs).as_ref() {
            let _ = tx.send(input);
        }
    }
}

/// Starts a correction unless one is running or a dictation is recording.
pub fn begin(app: &AppHandle) {
    let Some(state) = app.try_state::<CorrectionSessionState>() else {
        return;
    };
    // The tray state is set on every dictation start, finish and cancel (even
    // with no tray icon) and stays busy until the paste is done.
    let dictation_busy = crate::tray::dictation_in_progress(app)
        || app
            .try_state::<Arc<AudioRecordingManager>>()
            .is_some_and(|m| m.is_recording());
    if !should_start(dictation_busy, state.active.load(Ordering::SeqCst))
        || state
            .active
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
    {
        debug!("Correction shortcut ignored (dictation or another correction running)");
        return;
    }
    let (tx, rx) = unbounded_channel();
    *lock(&state.inputs) = Some(tx);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Run in its own task so a panic is caught here and cleanup still runs.
        let work = tauri::async_runtime::spawn(run(app.clone(), rx));
        if let Err(e) = work.await {
            error!("Correction session failed: {e}");
        }
        finish(&app).await;
    });
}

/// Ends every session, on every path: temporary keys off, panel hidden, the
/// engine's idle clock restarted, then the session flag cleared (in this
/// order, for the GPU watcher).
async fn finish(app: &AppHandle) {
    let state = app.state::<CorrectionSessionState>();
    *lock(&state.inputs) = None;
    set_bindings(app, Vec::new()).await;
    overlay::hide_recording_overlay(app);
    emit(app, &CorrectionUpdateEvent::hidden());
    if let Some(engine) = app.try_state::<Arc<LocalEngine>>() {
        engine.touch();
    }
    state.active.store(false, Ordering::SeqCst);
}

/// Registers and unregisters temporary bindings so exactly `wanted` are active.
async fn set_bindings(app: &AppHandle, wanted: Vec<String>) {
    let app = app.clone();
    // The backends answer over a channel; keep that off the async workers.
    let _ = tauri::async_runtime::spawn_blocking(move || sync_bindings(&app, &wanted)).await;
}

fn sync_bindings(app: &AppHandle, wanted: &[String]) {
    let state = app.state::<CorrectionSessionState>();
    let mut registered = lock(&state.registered);
    registered.retain(|id| {
        if wanted.contains(id) {
            return true;
        }
        if let Some(b) = temp_binding(id) {
            if let Err(e) = shortcut::unregister_shortcut(app, b) {
                warn!("Could not unregister '{id}': {e}");
            }
        }
        false
    });
    for id in wanted {
        if registered.contains(id) {
            continue;
        }
        match temp_binding(id).map(|b| shortcut::register_shortcut(app, b)) {
            Some(Ok(())) => registered.push(id.clone()),
            Some(Err(e)) => warn!("Could not register '{id}': {e}"),
            None => {}
        }
    }
}

fn temp_binding(id: &str) -> Option<ShortcutBinding> {
    let key = binding_key(id)?;
    Some(ShortcutBinding {
        id: id.to_string(),
        name: id.to_string(),
        description: String::new(),
        default_binding: key.clone(),
        current_binding: key,
    })
}

fn emit(app: &AppHandle, ev: &CorrectionUpdateEvent) {
    if let Err(e) = ev.emit(app) {
        warn!("Failed to emit correction update: {e}");
    }
}

fn cloud_target(app: &AppHandle, m: &CloudModel) -> Result<ChatTarget, CorrectionErrorCode> {
    let s = get_settings(app);
    let Some(provider) = s
        .post_process_providers
        .iter()
        .find(|p| p.id == m.provider_id)
    else {
        warn!("Correction provider '{}' not found", m.provider_id);
        return Err(CorrectionErrorCode::CloudFailed);
    };
    Ok(ChatTarget {
        base_url: provider.base_url.clone(),
        api_key: s
            .post_process_api_keys
            .get(&m.provider_id)
            .cloned()
            .unwrap_or_default(),
        model: m.model.clone(),
        format: provider.api_format,
        local: false,
    })
}

/// Saves how long a cold load (with its warm-up) took, for the next loading bar.
fn record_load_ms(app: &AppHandle, model: &str, ms: u64) {
    let mut s = get_correction_settings(app);
    s.last_load_ms.insert(model.to_string(), ms);
    write_correction_settings(app, &s);
}

async fn run(app: AppHandle, inputs: UnboundedReceiver<SessionInput>) {
    let mut session = Session {
        panel: CorrectionUpdateEvent::hidden(),
        app,
        inputs,
        started: Instant::now(),
    };
    if let Err(code) = session.correct().await {
        info!("Correction ended with {code:?}");
        set_bindings(&session.app, Vec::new()).await;
        session.panel.error = Some(code);
        session.hold(CorrectionPhase::Error, ERROR_HOLD).await;
    }
}

struct Session {
    app: AppHandle,
    inputs: UnboundedReceiver<SessionInput>,
    panel: CorrectionUpdateEvent,
    /// Start of the current attempt (the shortcut, or the last tone change).
    started: Instant,
}

type Step<T> = Result<Option<T>, CorrectionErrorCode>;

impl Session {
    fn engine(&self) -> Arc<LocalEngine> {
        self.app.state::<Arc<LocalEngine>>().inner().clone()
    }

    /// Sends the panel and shows the overlay in the panel's size.
    fn show(&self, phase: CorrectionPhase) {
        let mut panel = self.panel.clone();
        panel.phase = phase;
        emit(&self.app, &panel);
        overlay::show_correction_overlay(&self.app);
    }

    async fn enter(&mut self, phase: CorrectionPhase, choices: usize) {
        self.panel.phase = phase.clone();
        self.show(phase.clone());
        set_bindings(&self.app, wanted_bindings(&phase, choices)).await;
    }

    async fn hold(&mut self, phase: CorrectionPhase, how_long: Duration) {
        self.panel.phase = phase.clone();
        self.show(phase);
        tokio::time::sleep(how_long).await;
    }

    /// Waits for a key that means something in the current phase.
    async fn next_command(&mut self, fallback: &[FallbackOption]) -> SessionCommand {
        loop {
            let input = match tokio::time::timeout(CHOICE_TIMEOUT, self.inputs.recv()).await {
                Ok(Some(input)) => input,
                Ok(None) => return SessionCommand::Cancel,
                Err(_) => {
                    warn!("No answer to the correction panel; cancelling");
                    return SessionCommand::Cancel;
                }
            };
            let tones = self.panel.tones.len();
            match decide(&self.panel.phase, input, tones, fallback) {
                SessionCommand::Ignore => continue,
                cmd => return cmd,
            }
        }
    }

    async fn correct(&mut self) -> Result<(), CorrectionErrorCode> {
        let app = self.app.clone();
        let captured =
            tauri::async_runtime::spawn_blocking(move || selection::capture_selection(&app))
                .await
                .map_err(|_| CorrectionErrorCode::ClipboardFailed)?
                .map_err(capture_error_code)?;
        check_text(&captured.text)?;

        let settings = get_correction_settings(&self.app);
        let tones = session_tones(&settings);
        self.panel.original = captured.text;
        self.panel.tones = tones.iter().map(|t| t.name.clone()).collect();

        let Some(target) = self.choose_target(&settings).await? else {
            return Ok(());
        };
        let mut tone = 0;
        loop {
            self.panel.tone_index = tone as u32;
            let goal = &tones[tone].goal;
            let Some(text) = self.generate(&target, &settings.style_rules, goal).await? else {
                return Ok(());
            };
            self.panel.text = text;
            self.panel.gen_progress = Some(1.0);
            self.panel.elapsed_ms = Some(self.started.elapsed().as_millis() as u64);
            self.enter(CorrectionPhase::Preview, tones.len()).await;
            match self.next_command(&[]).await {
                SessionCommand::Apply => {
                    let text = self.panel.text.clone();
                    return self.apply(captured.hwnd, text).await;
                }
                SessionCommand::Regenerate(i) => {
                    tone = i;
                    self.started = Instant::now();
                }
                _ => return Ok(()),
            }
        }
    }

    /// Where the request goes. `None` when the user cancelled.
    async fn choose_target(&mut self, s: &CorrectionSettings) -> Step<ChatTarget> {
        let path = match &s.provider {
            CorrectionProvider::Cloud(m) => {
                self.panel.model_label = m.model.clone();
                return cloud_target(&self.app, m).map(Some);
            }
            CorrectionProvider::Local => s
                .local_model_path
                .clone()
                .filter(|p| !p.trim().is_empty())
                .ok_or(CorrectionErrorCode::NoModelChosen)?,
        };
        let model = PathBuf::from(&path);
        let size = std::fs::metadata(&model)
            .ok()
            .filter(|m| m.is_file())
            .ok_or(CorrectionErrorCode::ModelMissing)?
            .len();
        self.panel.model_label = model
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        let mut gpu = true;
        if !self.engine().serves(&model, true)
            && !gpu_guard::has_room(gpu_guard::free_vram_bytes(), size)
        {
            let options = fallback_options(s.fallback.is_some());
            self.panel.fallback_options = options.clone();
            self.enter(CorrectionPhase::NeedsFallback, options.len())
                .await;
            match self.next_command(&options).await {
                SessionCommand::UseFallback(FallbackOption::Cloud) => {
                    let m = s.fallback.as_ref().expect("offered only when set");
                    self.panel.model_label = m.model.clone();
                    return cloud_target(&self.app, m).map(Some);
                }
                SessionCommand::UseFallback(FallbackOption::Cpu) => gpu = false,
                _ => return Ok(None),
            }
        }
        let expected = s.last_load_ms.get(&path).copied();
        self.load_local(&model, gpu, &path, expected).await
    }

    /// Opens the local server (skipping the loading bar when it is already up)
    /// and warms it up after a cold load.
    async fn load_local(
        &mut self,
        model: &Path,
        gpu: bool,
        key: &str,
        expected_ms: Option<u64>,
    ) -> Step<ChatTarget> {
        let engine = self.engine();
        let cold = !engine.serves(model, gpu);
        if cold {
            self.panel.load_progress = load_progress(0, expected_ms);
            self.enter(CorrectionPhase::Loading, 0).await;
        }
        let load_started = Instant::now();
        let mut panel = self.panel.clone();
        let app = self.app.clone();
        let mut tick = move || {
            if cold {
                let elapsed = load_started.elapsed().as_millis() as u64;
                panel.load_progress = load_progress(elapsed, expected_ms);
                emit(&app, &panel);
            }
        };
        let phase = CorrectionPhase::Loading;
        let loading = engine.ensure_loaded(model, gpu, expected_ms, |_| {});
        let Some(loaded) = until_cancelled(loading, &mut self.inputs, &phase, &mut tick).await
        else {
            return Ok(None);
        };
        let (base_url, load_ms) = loaded.map_err(|e| {
            warn!("Correction model failed to load: {e}");
            engine_error_code(e)
        })?;
        let target = ChatTarget {
            base_url,
            api_key: String::new(),
            model: self.panel.model_label.clone(),
            format: PostProcessApiFormat::OpenAi,
            local: true,
        };
        if load_ms > 0 {
            // Cold load: the first request is slow, so pay for it here.
            let warmup = stream_chat(&target, WARMUP_SYSTEM, WARMUP_USER, |_| {});
            match until_cancelled(warmup, &mut self.inputs, &phase, &mut tick).await {
                None => return Ok(None),
                Some(Err(e)) => warn!("Correction warm-up failed: {e}"),
                Some(Ok(_)) => {}
            }
            let total = load_started.elapsed().as_millis() as u64;
            debug!("Correction model ready in {total} ms (load {load_ms} ms)");
            record_load_ms(&self.app, key, total);
        }
        self.panel.load_progress = cold.then_some(1.0);
        Ok(Some(target))
    }

    /// Streams one correction of the original text into the panel. `None`
    /// when the user pressed Esc.
    async fn generate(&mut self, target: &ChatTarget, rules: &str, goal: &str) -> Step<String> {
        self.panel.text.clear();
        self.panel.gen_progress = Some(0.0);
        self.panel.elapsed_ms = None;
        self.enter(CorrectionPhase::Generating, 0).await;

        let original = self.panel.original.clone();
        let (system, user) = build_messages(rules, goal, &original);
        let input_chars = original.chars().count();
        let mut panel = self.panel.clone();
        let app = self.app.clone();
        let mut raw = String::new();
        let mut last_emit: Option<Instant> = None;
        let on_delta = move |delta: &str| {
            raw.push_str(delta);
            if last_emit.is_some_and(|t| t.elapsed() < EMIT_INTERVAL) {
                return;
            }
            last_emit = Some(Instant::now());
            panel.text = clean_output(&original, &raw);
            panel.gen_progress = Some(generation_progress(raw.chars().count(), input_chars));
            emit(&app, &panel);
        };
        let phase = CorrectionPhase::Generating;
        let streaming = stream_chat(target, &system, &user, on_delta);
        let result = until_cancelled(streaming, &mut self.inputs, &phase, || {}).await;
        if target.local {
            self.engine().touch();
        }
        let failed = if target.local {
            CorrectionErrorCode::EngineFailed
        } else {
            CorrectionErrorCode::CloudFailed
        };
        let Some(result) = result else {
            return Ok(None);
        };
        let raw = result.map_err(|e| {
            warn!("Correction request failed: {e}");
            failed.clone()
        })?;
        let text = clean_output(&self.panel.original, &raw);
        if text.is_empty() {
            warn!("Correction reply was empty after cleanup");
            return Err(failed);
        }
        Ok(Some(text))
    }

    /// Pastes the correction over the selection in the original window, or
    /// leaves it on the clipboard when that window can't be brought back.
    async fn apply(&mut self, hwnd: isize, text: String) -> Result<(), CorrectionErrorCode> {
        set_bindings(&self.app, Vec::new()).await;
        overlay::hide_recording_overlay(&self.app);
        let app = self.app.clone();
        let pasted = tauri::async_runtime::spawn_blocking(move || {
            if selection::refocus(hwnd) {
                std::thread::sleep(SETTLE_DELAY);
                crate::clipboard::paste_correction(text, app).map(|()| true)
            } else {
                // Under the paste lock: a dictation paste restoring the
                // clipboard must not overwrite the correction left there.
                crate::clipboard::with_paste_lock(|| {
                    crate::clipboard::write_text_to_clipboard(&app, &text)
                })
                .map(|()| false)
            }
        })
        .await;
        match pasted {
            Ok(Ok(true)) => self.hold(CorrectionPhase::Applied, APPLIED_HOLD).await,
            Ok(Ok(false)) => self.hold(CorrectionPhase::Copied, COPIED_HOLD).await,
            Ok(Err(e)) => {
                warn!("Could not paste the correction: {e}");
                return Err(CorrectionErrorCode::ClipboardFailed);
            }
            Err(e) => {
                warn!("Paste task failed: {e}");
                return Err(CorrectionErrorCode::ClipboardFailed);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::correction::text::MAX_CORRECTION_CHARS;

    #[test]
    fn escape_cancels_while_loading_generating_or_previewing() {
        for p in [
            CorrectionPhase::Loading,
            CorrectionPhase::Generating,
            CorrectionPhase::Preview,
        ] {
            assert_eq!(
                decide(&p, SessionInput::Escape, 4, &[]),
                SessionCommand::Cancel
            );
        }
    }

    #[test]
    fn preview_enter_applies_and_digits_pick_tones() {
        let p = CorrectionPhase::Preview;
        assert_eq!(
            decide(&p, SessionInput::Enter, 4, &[]),
            SessionCommand::Apply
        );
        assert_eq!(
            decide(&p, SessionInput::Digit(2), 4, &[]),
            SessionCommand::Regenerate(1)
        );
        assert_eq!(
            decide(&p, SessionInput::Digit(5), 4, &[]),
            SessionCommand::Ignore
        );
    }

    #[test]
    fn fallback_digits_pick_options_and_enter_takes_the_first() {
        let opts = [FallbackOption::Cloud, FallbackOption::Cpu];
        let p = CorrectionPhase::NeedsFallback;
        assert_eq!(
            decide(&p, SessionInput::Digit(2), 4, &opts),
            SessionCommand::UseFallback(FallbackOption::Cpu)
        );
        assert_eq!(
            decide(&p, SessionInput::Enter, 4, &opts),
            SessionCommand::UseFallback(FallbackOption::Cloud)
        );
        assert_eq!(
            decide(&p, SessionInput::Escape, 4, &opts),
            SessionCommand::Cancel
        );
        assert_eq!(
            decide(&p, SessionInput::Digit(3), 4, &opts),
            SessionCommand::Ignore
        );
    }

    #[test]
    fn finished_phases_ignore_every_key() {
        for p in [
            CorrectionPhase::Copied,
            CorrectionPhase::Applied,
            CorrectionPhase::Error,
            CorrectionPhase::Hidden,
        ] {
            for input in [
                SessionInput::Escape,
                SessionInput::Enter,
                SessionInput::Digit(1),
            ] {
                assert_eq!(
                    decide(&p, input, 4, &[FallbackOption::Cpu]),
                    SessionCommand::Ignore
                );
            }
        }
    }

    #[test]
    fn working_phases_ignore_enter_and_digits() {
        for p in [CorrectionPhase::Loading, CorrectionPhase::Generating] {
            assert_eq!(
                decide(&p, SessionInput::Enter, 4, &[]),
                SessionCommand::Ignore
            );
            assert_eq!(
                decide(&p, SessionInput::Digit(1), 4, &[]),
                SessionCommand::Ignore
            );
        }
    }

    #[test]
    fn digit_zero_and_tones_past_nine_are_ignored() {
        let p = CorrectionPhase::Preview;
        assert_eq!(
            decide(&p, SessionInput::Digit(0), 12, &[]),
            SessionCommand::Ignore
        );
        assert_eq!(
            decide(&p, SessionInput::Digit(9), 12, &[]),
            SessionCommand::Regenerate(8)
        );
    }

    #[test]
    fn ignores_shortcut_during_dictation_or_another_correction() {
        assert!(should_start(false, false));
        assert!(!should_start(true, false));
        assert!(!should_start(false, true));
    }

    #[test]
    fn rejects_too_long_text() {
        assert_eq!(
            check_text(&"a".repeat(MAX_CORRECTION_CHARS + 1)),
            Err(CorrectionErrorCode::TextTooLong)
        );
        assert_eq!(check_text("oi"), Ok(()));
    }

    #[test]
    fn accepts_text_of_exactly_the_limit_counted_in_chars() {
        assert_eq!(check_text(&"a".repeat(MAX_CORRECTION_CHARS)), Ok(()));
        // Multi-byte characters count once each.
        assert_eq!(check_text(&"é".repeat(MAX_CORRECTION_CHARS)), Ok(()));
    }

    #[test]
    fn phases_listen_only_to_the_keys_they_use() {
        assert_eq!(
            wanted_bindings(&CorrectionPhase::Loading, 4),
            ["correction_cancel"]
        );
        assert_eq!(
            wanted_bindings(&CorrectionPhase::Generating, 4),
            ["correction_cancel"]
        );
        assert_eq!(
            wanted_bindings(&CorrectionPhase::Preview, 2),
            [
                "correction_cancel",
                "correction_apply",
                "correction_tone_1",
                "correction_tone_2"
            ]
        );
        assert_eq!(wanted_bindings(&CorrectionPhase::Preview, 12).len(), 2 + 9);
        assert_eq!(
            wanted_bindings(&CorrectionPhase::NeedsFallback, 1),
            ["correction_cancel", "correction_apply", "correction_tone_1"]
        );
        for p in [
            CorrectionPhase::Applied,
            CorrectionPhase::Copied,
            CorrectionPhase::Error,
            CorrectionPhase::Hidden,
        ] {
            assert!(wanted_bindings(&p, 4).is_empty());
        }
    }

    #[test]
    fn temporary_bindings_map_to_escape_enter_and_digits() {
        assert_eq!(binding_key("correction_cancel").as_deref(), Some("escape"));
        assert_eq!(binding_key("correction_apply").as_deref(), Some("enter"));
        assert_eq!(binding_key("correction_tone_1").as_deref(), Some("1"));
        assert_eq!(binding_key("correction_tone_9").as_deref(), Some("9"));
        assert_eq!(binding_key("correction_tone_10"), None);
        assert_eq!(binding_key("correction_tone_0"), None);
        assert_eq!(binding_key("transcribe"), None);

        assert_eq!(
            input_for_binding("correction_cancel"),
            Some(SessionInput::Escape)
        );
        assert_eq!(
            input_for_binding("correction_apply"),
            Some(SessionInput::Enter)
        );
        assert_eq!(
            input_for_binding("correction_tone_3"),
            Some(SessionInput::Digit(3))
        );
        assert_eq!(input_for_binding("cancel"), None);
    }

    #[test]
    fn temporary_bindings_are_not_user_bindings() {
        // ACTION_MAP itself is not touched here: referencing it links the
        // window runtime into the test binary, which then fails to load
        // (comctl32 v6 entry points without a manifest).
        let defaults = crate::settings::get_default_settings().bindings;
        for id in TEMPORARY_BINDING_IDS {
            assert!(!defaults.contains_key(*id), "{id} must not be persisted");
            assert!(binding_key(id).is_some(), "{id} needs a key");
            assert!(input_for_binding(id).is_some(), "{id} needs an input");
        }
        assert!(defaults.contains_key("correct_selection"));
    }

    #[test]
    fn capture_errors_map_to_panel_codes() {
        use crate::correction::selection::CaptureError;
        assert_eq!(
            capture_error_code(CaptureError::NothingSelected),
            CorrectionErrorCode::NothingSelected
        );
        assert_eq!(
            capture_error_code(CaptureError::KeysHeld),
            CorrectionErrorCode::KeysHeld
        );
        assert_eq!(
            capture_error_code(CaptureError::Clipboard("x".into())),
            CorrectionErrorCode::ClipboardFailed
        );
    }

    #[test]
    fn engine_errors_map_to_panel_codes() {
        use crate::correction::local_engine::EngineError;
        assert_eq!(
            engine_error_code(EngineError::ModelMissing("m".into())),
            CorrectionErrorCode::ModelMissing
        );
        for e in [
            EngineError::BinaryMissing,
            EngineError::StartFailed("x".into()),
            EngineError::Crashed,
        ] {
            assert_eq!(engine_error_code(e), CorrectionErrorCode::EngineFailed);
        }
    }

    #[test]
    fn fallback_offers_cloud_only_when_configured() {
        assert_eq!(
            fallback_options(true),
            [FallbackOption::Cloud, FallbackOption::Cpu]
        );
        assert_eq!(fallback_options(false), [FallbackOption::Cpu]);
    }

    #[test]
    fn empty_tone_list_uses_the_defaults() {
        let mut s = crate::correction::settings::CorrectionSettings::default();
        s.tones.clear();
        assert_eq!(
            session_tones(&s),
            crate::correction::settings::default_tones()
        );
        s.tones = vec![crate::correction::settings::CorrectionTone {
            id: "a".into(),
            name: "A".into(),
            goal: "g".into(),
        }];
        assert_eq!(session_tones(&s).len(), 1);
    }

    /// Sets a flag when dropped, to observe that a future was abandoned.
    struct DropFlag(std::sync::Arc<std::sync::atomic::AtomicBool>);
    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    #[test]
    fn escape_drops_the_work_in_progress() {
        let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = DropFlag(dropped.clone());
        let work = async move {
            let _flag = flag;
            std::future::pending::<()>().await;
        };
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        tx.send(SessionInput::Enter).unwrap();
        tx.send(SessionInput::Digit(2)).unwrap();
        tx.send(SessionInput::Escape).unwrap();
        let out = tauri::async_runtime::block_on(until_cancelled(
            work,
            &mut rx,
            &CorrectionPhase::Generating,
            || {},
        ));
        assert!(out.is_none());
        assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
        assert!(rx.try_recv().is_err(), "every queued key was consumed");
    }

    #[test]
    fn work_that_finishes_returns_its_output_and_ticks_meanwhile() {
        let (_tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut ticks = 0;
        let out = tauri::async_runtime::block_on(until_cancelled(
            async {
                tokio::time::sleep(std::time::Duration::from_millis(450)).await;
                7
            },
            &mut rx,
            &CorrectionPhase::Loading,
            || ticks += 1,
        ));
        assert_eq!(out, Some(7));
        assert!(ticks >= 1, "ticked {ticks} times");
    }
}
