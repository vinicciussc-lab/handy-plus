//! One text correction, from the `correct_selection` (or
//! `translate_selection`) shortcut to the paste.
//!
//! The decisions (which key does what in which phase, which keys to listen
//! to) are pure functions at the top. The session itself runs as an async
//! task that copies the selection, opens the panel with the original text
//! and the options (styles, then adjustments) while the model loads, streams
//! each pick into the overlay panel via [`CorrectionUpdateEvent`] and waits
//! for Enter, a digit or Esc, which arrive through temporary global shortcuts.
//! A style processes the original; an adjustment works on the current result,
//! so adjustments chain.

use super::input_hooks::{self, binding_vk, click_outside, HookEvent};
use super::local_engine::{EngineError, LocalEngine};
use super::selection::{self, CaptureError};
use super::settings::{
    default_tones, get_correction_settings, write_correction_settings, CloudModel,
    CorrectionProvider, CorrectionSettings, CorrectionTone, ToneKind,
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
use std::path::PathBuf;
use std::pin::Pin;
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
/// The "copied" pill after the user asked for the copy.
const COPY_HOLD: Duration = Duration::from_secs(2);
/// Pause after bringing the original window back, before pasting.
const SETTLE_DELAY: Duration = Duration::from_millis(80);
/// A preview nobody answers is dropped, so a failed key registration can't
/// leave the session (and the shortcut) stuck forever.
const CHOICE_TIMEOUT: Duration = Duration::from_secs(300);
/// The first request after a load is very slow, so a tiny one goes first.
const WARMUP_SYSTEM: &str = "Responda só OK.";
const WARMUP_USER: &str = "oi";

/// Id of the built-in translation adjustment.
const TRANSLATE_ID: &str = "traduzir";

const CANCEL_ID: &str = "correction_cancel";
const APPLY_ID: &str = "correction_apply";
const TONE_PREFIX: &str = "correction_tone_";
const VIEW_ID: &str = "correction_view";
const SCROLL_UP_ID: &str = "correction_scroll_up";
const SCROLL_DOWN_ID: &str = "correction_scroll_down";
const COPY_ID: &str = "correction_copy";
/// Temporary shortcuts, registered only while a session needs them. They are
/// not in `AppSettings.bindings`, so they never show on the shortcuts screen
/// and startup/suspend/resume never touch them.
pub const TEMPORARY_BINDING_IDS: &[&str] = &[
    CANCEL_ID,
    APPLY_ID,
    VIEW_ID,
    SCROLL_UP_ID,
    SCROLL_DOWN_ID,
    COPY_ID,
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
    /// The panel shows the original text and the options, waiting for a pick.
    /// Nothing is generated yet; the local model may be loading meanwhile.
    Choosing,
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

/// Which text the preview shows; Tab cycles through them.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Type, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionView {
    /// The corrected text, clean.
    #[default]
    Corrected,
    /// The corrected text with the changes marked.
    Changes,
    /// The text as it was selected.
    Original,
}

impl CorrectionView {
    pub fn next(self) -> Self {
        match self {
            Self::Corrected => Self::Changes,
            Self::Changes => Self::Original,
            Self::Original => Self::Corrected,
        }
    }
}

/// Everything the correction panel shows. Sent whenever it changes.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct CorrectionUpdateEvent {
    pub phase: CorrectionPhase,
    pub original: String,
    pub text: String,
    pub load_progress: Option<f32>,
    pub gen_progress: Option<f32>,
    /// Names of the styles, then of the adjustments: one numbered list,
    /// styles first (number = position + 1; only 1 to 9 have a key).
    pub styles: Vec<String>,
    pub adjusts: Vec<String>,
    /// Index in `styles` of the style the current text came from.
    pub current_style: Option<u32>,
    /// Indexes in `adjusts` applied on top of it, in order.
    pub applied_adjusts: Vec<u32>,
    /// The local model is loading in the background while the user chooses.
    pub preloading: bool,
    pub model_label: String,
    pub elapsed_ms: Option<u64>,
    pub error: Option<CorrectionErrorCode>,
    pub fallback_options: Vec<FallbackOption>,
    pub view: CorrectionView,
    /// Counts the scroll key presses; the panel scrolls once per new value.
    pub scroll_nonce: u32,
    /// Direction of the last scroll: -1 up, 1 down, 0 none yet.
    pub scroll_dir: i8,
}

impl CorrectionUpdateEvent {
    fn hidden() -> Self {
        Self {
            phase: CorrectionPhase::Hidden,
            original: String::new(),
            text: String::new(),
            load_progress: None,
            gen_progress: None,
            styles: Vec::new(),
            adjusts: Vec::new(),
            current_style: None,
            applied_adjusts: Vec::new(),
            preloading: false,
            model_label: String::new(),
            elapsed_ms: None,
            error: None,
            fallback_options: Vec::new(),
            view: CorrectionView::Corrected,
            scroll_nonce: 0,
            scroll_dir: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionInput {
    Escape,
    Enter,
    Digit(u8),
    Tab,
    ScrollUp,
    ScrollDown,
    /// Ctrl+C in the preview.
    CopyKey,
    /// The correction shortcut pressed again while a session is open.
    Shortcut,
    /// A click in the panel.
    Ui(UiAction),
    /// A click outside the panel, or the user typing again.
    Dismiss,
}

/// What a click in the panel asks for.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum UiAction {
    Apply,
    Copy,
    Cancel,
    /// 1-based, like the digit keys.
    Tone(u8),
    View(CorrectionView),
}

#[derive(Clone, Debug, PartialEq)]
pub enum SessionCommand {
    Cancel,
    Apply,
    /// Process the original with this style (index in the styles); the
    /// adjustments applied so far are dropped.
    RunStyle(usize),
    /// Apply this adjustment (index in the adjustments) to the current text,
    /// or to the original when no style ran yet.
    RunAdjust(usize),
    UseFallback(FallbackOption),
    NextView,
    SetView(CorrectionView),
    /// Put the correction on the clipboard and close.
    Copy,
    /// -1 up, 1 down.
    Scroll(i8),
    Ignore,
}

/// The option a 1-based number picks in the single list `styles ++ adjusts`.
pub fn option_for_number(n: usize, styles: usize, adjusts: usize) -> Option<SessionCommand> {
    let i = n.checked_sub(1)?;
    if i < styles {
        Some(SessionCommand::RunStyle(i))
    } else if i < styles + adjusts {
        Some(SessionCommand::RunAdjust(i - styles))
    } else {
        None
    }
}

/// What a key means in a phase. In the choice and the preview a number picks
/// a style or an adjustment (digits 1 to 9; a click reaches any option). In
/// the fallback prompt it picks an option.
pub fn decide(
    phase: &CorrectionPhase,
    input: SessionInput,
    styles: usize,
    adjusts: usize,
    fallback: &[FallbackOption],
) -> SessionCommand {
    use CorrectionPhase as P;
    let pick = |n: usize| option_for_number(n, styles, adjusts).unwrap_or(SessionCommand::Ignore);
    match (phase, input) {
        (
            P::Choosing | P::Loading | P::Generating | P::Preview | P::NeedsFallback,
            SessionInput::Escape
            | SessionInput::Shortcut
            | SessionInput::Dismiss
            | SessionInput::Ui(UiAction::Cancel),
        ) => SessionCommand::Cancel,
        (P::Choosing, SessionInput::Enter) if styles > 0 => SessionCommand::RunStyle(0),
        (P::Preview, SessionInput::Enter | SessionInput::Ui(UiAction::Apply)) => {
            SessionCommand::Apply
        }
        (P::Preview, SessionInput::CopyKey | SessionInput::Ui(UiAction::Copy)) => {
            SessionCommand::Copy
        }
        (P::Preview, SessionInput::Ui(UiAction::View(v))) => SessionCommand::SetView(v),
        (P::Preview, SessionInput::Tab) => SessionCommand::NextView,
        (P::Choosing | P::Generating | P::Preview, SessionInput::ScrollUp) => {
            SessionCommand::Scroll(-1)
        }
        (P::Choosing | P::Generating | P::Preview, SessionInput::ScrollDown) => {
            SessionCommand::Scroll(1)
        }
        (P::Choosing | P::Preview, SessionInput::Digit(d)) if (1..=9).contains(&d) => {
            pick(usize::from(d))
        }
        (P::Choosing | P::Preview, SessionInput::Ui(UiAction::Tone(n))) => pick(usize::from(n)),
        (P::NeedsFallback, SessionInput::Enter) => {
            fallback.first().map_or(SessionCommand::Ignore, |o| {
                SessionCommand::UseFallback(o.clone())
            })
        }
        (P::NeedsFallback, SessionInput::Digit(d) | SessionInput::Ui(UiAction::Tone(d)))
            if d >= 1 =>
        {
            fallback
                .get(usize::from(d) - 1)
                .map_or(SessionCommand::Ignore, |o| {
                    SessionCommand::UseFallback(o.clone())
                })
        }
        _ => SessionCommand::Ignore,
    }
}

/// The text a pick works on: a style always starts from the original; an
/// adjustment works on the current result, or on the original before any.
pub fn run_source<'a>(
    cmd: &SessionCommand,
    original: &'a str,
    current: Option<&'a str>,
) -> &'a str {
    match cmd {
        SessionCommand::RunAdjust(_) => current.unwrap_or(original),
        _ => original,
    }
}

/// Records a pick in the panel: a style replaces the chain, an adjustment
/// joins it.
fn mark_choice(panel: &mut CorrectionUpdateEvent, cmd: &SessionCommand) {
    match cmd {
        SessionCommand::RunStyle(i) => {
            panel.current_style = Some(*i as u32);
            panel.applied_adjusts.clear();
        }
        SessionCommand::RunAdjust(i) => panel.applied_adjusts.push(*i as u32),
        _ => {}
    }
}

/// The shortcut is ignored while a dictation is anywhere in its pipeline
/// (recording, transcribing, post-processing or pasting) and while a
/// correction runs.
pub fn should_start(dictation_busy: bool, session_active: bool) -> bool {
    !dictation_busy && !session_active
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShortcutRoute {
    Start,
    /// Pressed again with a session open: closes it.
    Close,
    Ignore,
}

/// What the correction shortcut does now. An open session takes it as "close";
/// a dictation in progress swallows it.
pub fn shortcut_route(dictation_busy: bool, session_active: bool) -> ShortcutRoute {
    if session_active {
        ShortcutRoute::Close
    } else if should_start(dictation_busy, false) {
        ShortcutRoute::Start
    } else {
        ShortcutRoute::Ignore
    }
}

/// The mouse hook is needed only while the panel is open (a click outside it
/// closes it). Off otherwise: a global mouse hook adds latency to every mouse
/// event, games included.
pub fn wants_mouse_hook(phase: &CorrectionPhase) -> bool {
    use CorrectionPhase as P;
    matches!(
        phase,
        P::Choosing | P::Loading | P::Generating | P::Preview | P::NeedsFallback
    )
}

/// A hook event closes the open panel: a click outside `panel` (its physical
/// screen rect, if known), or a typing key the panel is not bound to
/// (`registered` = temporary binding ids, so a tone digit stays a tone key).
pub fn hook_dismisses(
    ev: HookEvent,
    panel: Option<(i32, i32, i32, i32)>,
    registered: &[String],
) -> bool {
    match ev {
        HookEvent::MouseDown { x, y } => panel.is_some_and(|rect| click_outside(x, y, rect)),
        HookEvent::TypingKey { vk } => !registered
            .iter()
            .filter_map(|id| binding_key(id))
            .filter_map(|key| binding_vk(&key))
            .any(|bound| bound == vk),
    }
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

/// Temporary bindings a phase listens to: Esc, Enter, one digit per option
/// and the scroll keys while choosing; Esc (and the scroll keys while text
/// streams); Esc, Enter, Tab, the scroll keys, Ctrl+C and one digit per
/// option in the preview; Esc, Enter and one digit per option in the
/// fallback prompt; nothing otherwise.
pub fn wanted_bindings(phase: &CorrectionPhase, choices: usize) -> Vec<String> {
    use CorrectionPhase as P;
    match phase {
        P::Choosing => [CANCEL_ID.to_string(), APPLY_ID.to_string()]
            .into_iter()
            .chain((1..=choices.min(9)).map(|n| format!("{TONE_PREFIX}{n}")))
            .chain([SCROLL_UP_ID.to_string(), SCROLL_DOWN_ID.to_string()])
            .collect(),
        P::Loading => vec![CANCEL_ID.to_string()],
        P::Generating => vec![
            CANCEL_ID.to_string(),
            SCROLL_UP_ID.to_string(),
            SCROLL_DOWN_ID.to_string(),
        ],
        P::Preview => [CANCEL_ID.to_string(), APPLY_ID.to_string()]
            .into_iter()
            .chain((1..=choices.min(9)).map(|n| format!("{TONE_PREFIX}{n}")))
            .chain([
                VIEW_ID.to_string(),
                SCROLL_UP_ID.to_string(),
                SCROLL_DOWN_ID.to_string(),
                COPY_ID.to_string(),
            ])
            .collect(),
        P::NeedsFallback => [CANCEL_ID.to_string(), APPLY_ID.to_string()]
            .into_iter()
            .chain((1..=choices.min(9)).map(|n| format!("{TONE_PREFIX}{n}")))
            .collect(),
        _ => Vec::new(),
    }
}

/// Volume of the correction sounds relative to the dictation feedback volume.
pub const CORRECTION_SOUND_VOLUME: f32 = 0.6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorrectionSound {
    Start,
    Ready,
}

impl CorrectionSound {
    fn file(self) -> &'static str {
        match self {
            Self::Start => "resources/correction_start.wav",
            Self::Ready => "resources/correction_ready.wav",
        }
    }
}

/// The start sound plays once per session, when work first begins (loading or
/// generating); the ready sound plays every time the preview appears.
pub fn sound_for_transition(
    _prev: &CorrectionPhase,
    next: &CorrectionPhase,
    start_played: bool,
) -> Option<CorrectionSound> {
    use CorrectionPhase as P;
    match next {
        P::Loading | P::Generating if !start_played => Some(CorrectionSound::Start),
        P::Preview => Some(CorrectionSound::Ready),
        _ => None,
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
        VIEW_ID => Some("tab".to_string()),
        SCROLL_UP_ID => Some("up".to_string()),
        SCROLL_DOWN_ID => Some("down".to_string()),
        COPY_ID => Some("ctrl+c".to_string()),
        _ => tone_digit(id).map(|d| d.to_string()),
    }
}

pub fn input_for_binding(id: &str) -> Option<SessionInput> {
    match id {
        CANCEL_ID => Some(SessionInput::Escape),
        APPLY_ID => Some(SessionInput::Enter),
        VIEW_ID => Some(SessionInput::Tab),
        SCROLL_UP_ID => Some(SessionInput::ScrollUp),
        SCROLL_DOWN_ID => Some(SessionInput::ScrollDown),
        COPY_ID => Some(SessionInput::CopyKey),
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

/// The styles and the adjustments, each in its saved order.
fn split_tones(tones: &[CorrectionTone]) -> (Vec<CorrectionTone>, Vec<CorrectionTone>) {
    tones
        .iter()
        .cloned()
        .partition(|t| t.kind == ToneKind::Style)
}

/// The translation the translate shortcut runs: the `traduzir` adjustment
/// (and its index), or the built-in Traduzir goal if the user deleted it.
fn translate_goal(adjusts: &[CorrectionTone]) -> (Option<usize>, String) {
    if let Some(i) = adjusts.iter().position(|t| t.id == TRANSLATE_ID) {
        return (Some(i), adjusts[i].goal.clone());
    }
    let goal = default_tones()
        .into_iter()
        .find(|t| t.id == TRANSLATE_ID)
        .map(|t| t.goal)
        .unwrap_or_default();
    (None, goal)
}

/// Runs `work` until it finishes, calling `on_tick` every [`TICK`] meanwhile.
/// Esc drops `work` (aborting a stream or a load) and returns `None`; the
/// scroll keys call `on_scroll`; other keys are ignored.
async fn until_cancelled<F: Future>(
    work: F,
    inputs: &mut UnboundedReceiver<SessionInput>,
    phase: &CorrectionPhase,
    mut on_tick: impl FnMut(),
    mut on_scroll: impl FnMut(i8),
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
                match decide(phase, input, 0, 0, &[]) {
                    SessionCommand::Cancel => return None,
                    SessionCommand::Scroll(d) => on_scroll(d),
                    _ => {}
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
    /// Bindings just unregistered, and when: a tone key re-binds the panel
    /// while the key is still down, so it must not then count as typing.
    recently_unbound: Mutex<Vec<(String, Instant)>>,
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

/// How long a just-unbound panel key still does not count as typing.
const UNBOUND_GRACE: Duration = Duration::from_millis(1000);

/// Closes the open panel on a click outside it or when the user types again
/// (the key still reaches the program). Installs the keyboard hook; the
/// mouse hook is installed only while a panel is open (see [`wants_mouse_hook`]).
pub fn watch_input_hooks(app: &AppHandle) {
    let app = app.clone();
    input_hooks::start_hooks(move |ev| {
        let Some(state) = app.try_state::<CorrectionSessionState>() else {
            return;
        };
        // Cheap exit while no correction runs: dictation and games untouched.
        if !state.active.load(Ordering::SeqCst) {
            return;
        }
        let panel = match ev {
            HookEvent::MouseDown { .. } => overlay_rect(&app),
            HookEvent::TypingKey { .. } => None,
        };
        let mut bound = lock(&state.registered).clone();
        {
            let mut recent = lock(&state.recently_unbound);
            recent.retain(|(_, at)| at.elapsed() < UNBOUND_GRACE);
            bound.extend(recent.iter().map(|(id, _)| id.clone()));
        }
        if hook_dismisses(ev, panel, &bound) {
            let why = match ev {
                HookEvent::MouseDown { .. } => "a click outside the panel",
                HookEvent::TypingKey { .. } => "typing",
            };
            debug!("Correction panel dismissed by {why}");
            input(&app, SessionInput::Dismiss);
        }
    });
}

/// The overlay window's rect in physical screen pixels, while it is shown.
#[cfg(windows)]
fn overlay_rect(app: &AppHandle) -> Option<(i32, i32, i32, i32)> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsWindowVisible};
    let hwnd = app.get_webview_window("recording_overlay")?.hwnd().ok()?;
    let mut r = RECT::default();
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            return None;
        }
        GetWindowRect(hwnd, &mut r).ok()?;
    }
    Some((r.left, r.top, r.right, r.bottom))
}

#[cfg(not(windows))]
fn overlay_rect(_app: &AppHandle) -> Option<(i32, i32, i32, i32)> {
    None
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

/// `translate_selection`: starts a session that translates right away, when
/// the shortcut is released.
struct TranslateSelectionAction;

impl ShortcutAction for TranslateSelectionAction {
    fn start(&self, _app: &AppHandle, _binding_id: &str, _shortcut_str: &str) {}

    fn stop(&self, app: &AppHandle, _binding_id: &str, _shortcut_str: &str) {
        start_session(app, SessionMode::Translate);
    }
}

/// Esc, Enter, Tab, the arrows and the digits while a session listens for them.
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
    map.insert(
        "translate_selection".to_string(),
        Arc::new(TranslateSelectionAction),
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

/// A click in the correction panel.
#[tauri::command]
#[specta::specta]
pub fn correction_ui_action(app: AppHandle, action: UiAction) {
    input(&app, SessionInput::Ui(action));
}

/// A dictation is recording, transcribing, post-processing or pasting. The
/// tray state is set on every dictation start, finish and cancel (even with no
/// tray icon) and stays busy until the paste is done.
pub fn dictation_busy(app: &AppHandle) -> bool {
    crate::tray::dictation_in_progress(app)
        || app
            .try_state::<Arc<AudioRecordingManager>>()
            .is_some_and(|m| m.is_recording())
}

/// A correction session is running (from the shortcut to the panel closing).
pub fn session_active(app: &AppHandle) -> bool {
    app.try_state::<CorrectionSessionState>()
        .is_some_and(|s| s.active.load(Ordering::SeqCst))
}

/// The typing hint was clicked: the same path as the shortcut. The hint pill
/// never takes focus, so the user's program still has it.
#[tauri::command]
#[specta::specta]
pub fn correction_hint_clicked(app: AppHandle) {
    overlay::hide_hint_overlay(&app);
    begin(&app);
}

/// The correction shortcut: opens the panel to choose, or closes the open one.
/// Ignored while a dictation is recording or being processed.
pub fn begin(app: &AppHandle) {
    start_session(app, SessionMode::Correct);
}

/// Starts a session in `mode`, or closes the open one (either shortcut).
fn start_session(app: &AppHandle, mode: SessionMode) {
    let Some(state) = app.try_state::<CorrectionSessionState>() else {
        return;
    };
    match shortcut_route(dictation_busy(app), state.active.load(Ordering::SeqCst)) {
        ShortcutRoute::Close => {
            input(app, SessionInput::Shortcut);
            return;
        }
        ShortcutRoute::Ignore => {
            debug!("Correction shortcut ignored (dictation running)");
            return;
        }
        ShortcutRoute::Start => {}
    }
    if state
        .active
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        debug!("Correction shortcut ignored (another correction just started)");
        return;
    }
    let (tx, rx) = unbounded_channel();
    *lock(&state.inputs) = Some(tx);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Run in its own task so a panic is caught here and cleanup still runs.
        let work = tauri::async_runtime::spawn(run(app.clone(), rx, mode));
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
    input_hooks::set_mouse_hook(false);
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
    lock(&state.recently_unbound).retain(|(_, at)| at.elapsed() < UNBOUND_GRACE);
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
        lock(&state.recently_unbound).push((id.clone(), Instant::now()));
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

/// A local model load, with its warm-up after a cold load. Built when the
/// panel opens so it can run while the user chooses; dropping it cancels the
/// load (the engine's load guard kills a half-loaded server).
type LoadFuture = Pin<Box<dyn Future<Output = Result<ChatTarget, CorrectionErrorCode>> + Send>>;

/// The model as the user picks: still loading, ready (or failed), or waiting
/// for the user to choose a way out because the GPU has no room.
enum Preload {
    Running {
        work: LoadFuture,
        started: Instant,
        /// A server has to start (the loading bar shows).
        cold: bool,
        expected_ms: Option<u64>,
    },
    Done(Result<ChatTarget, CorrectionErrorCode>),
    NoRoom {
        model: PathBuf,
        key: String,
        expected_ms: Option<u64>,
    },
}

/// What opened the session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionMode {
    /// The correction shortcut: the panel waits for a style or an adjustment.
    Correct,
    /// The translate shortcut: runs the translation right away.
    Translate,
}

/// Opens the local server for `model` and warms it up after a cold load (the
/// first request after a load is very slow), recording how long it all took.
fn local_load(
    app: AppHandle,
    engine: Arc<LocalEngine>,
    model: PathBuf,
    gpu: bool,
    key: String,
    expected_ms: Option<u64>,
    label: String,
) -> LoadFuture {
    Box::pin(async move {
        let started = Instant::now();
        let (base_url, load_ms) = engine
            .ensure_loaded(&model, gpu, expected_ms, |_| {})
            .await
            .map_err(|e| {
                warn!("Correction model failed to load: {e}");
                engine_error_code(e)
            })?;
        let target = ChatTarget {
            base_url,
            api_key: String::new(),
            model: label,
            format: PostProcessApiFormat::OpenAi,
            local: true,
        };
        if load_ms > 0 {
            if let Err(e) = stream_chat(&target, WARMUP_SYSTEM, WARMUP_USER, |_| {}).await {
                warn!("Correction warm-up failed: {e}");
            }
            let total = started.elapsed().as_millis() as u64;
            debug!("Correction model ready in {total} ms (load {load_ms} ms)");
            record_load_ms(&app, &key, total);
        }
        Ok(target)
    })
}

async fn run(app: AppHandle, inputs: UnboundedReceiver<SessionInput>, mode: SessionMode) {
    let mut session = Session {
        panel: CorrectionUpdateEvent::hidden(),
        app,
        inputs,
        started: Instant::now(),
        start_played: false,
    };
    if let Err(code) = session.correct(mode).await {
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
    /// Start of the current attempt (the last pick).
    started: Instant,
    /// The start sound already played in this session.
    start_played: bool,
}

type Step<T> = Result<Option<T>, CorrectionErrorCode>;

fn goal_for(cmd: &SessionCommand, styles: &[CorrectionTone], adjusts: &[CorrectionTone]) -> String {
    match cmd {
        SessionCommand::RunStyle(i) => styles.get(*i),
        SessionCommand::RunAdjust(i) => adjusts.get(*i),
        _ => None,
    }
    .map(|t| t.goal.clone())
    .unwrap_or_default()
}

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
        input_hooks::set_mouse_hook(wants_mouse_hook(&phase));
        self.play_sound_for(&phase);
        self.panel.phase = phase.clone();
        self.show(phase.clone());
        set_bindings(&self.app, wanted_bindings(&phase, choices)).await;
    }

    fn play_sound_for(&mut self, next: &CorrectionPhase) {
        let sound = sound_for_transition(&self.panel.phase, next, self.start_played);
        let Some(sound) = sound else { return };
        if sound == CorrectionSound::Start {
            self.start_played = true;
        }
        if get_correction_settings(&self.app).sounds {
            crate::audio_feedback::play_resource_sound(
                &self.app,
                sound.file(),
                CORRECTION_SOUND_VOLUME,
            );
        }
    }

    async fn hold(&mut self, phase: CorrectionPhase, how_long: Duration) {
        input_hooks::set_mouse_hook(wants_mouse_hook(&phase));
        self.panel.phase = phase.clone();
        self.show(phase);
        tokio::time::sleep(how_long).await;
    }

    /// Handles a key in the current phase: views and scrolling are done here;
    /// a command the caller acts on is returned.
    fn handle_input(
        &mut self,
        input: SessionInput,
        fallback: &[FallbackOption],
    ) -> Option<SessionCommand> {
        let (styles, adjusts) = (self.panel.styles.len(), self.panel.adjusts.len());
        match decide(&self.panel.phase, input, styles, adjusts, fallback) {
            SessionCommand::Ignore => None,
            SessionCommand::NextView => {
                self.panel.view = self.panel.view.next();
                emit(&self.app, &self.panel);
                None
            }
            SessionCommand::SetView(v) => {
                self.panel.view = v;
                emit(&self.app, &self.panel);
                None
            }
            SessionCommand::Scroll(d) => {
                self.panel.scroll_nonce = self.panel.scroll_nonce.wrapping_add(1);
                self.panel.scroll_dir = d;
                emit(&self.app, &self.panel);
                None
            }
            cmd => Some(cmd),
        }
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
            if let Some(cmd) = self.handle_input(input, fallback) {
                return cmd;
            }
        }
    }

    /// Waits for the user's pick in the choice panel while the model keeps
    /// loading; the loading bar moves meanwhile.
    async fn wait_choice(&mut self, preload: &mut Preload) -> SessionCommand {
        enum Woke {
            Loaded(Result<ChatTarget, CorrectionErrorCode>),
            Input(Option<SessionInput>),
            Tick,
        }
        loop {
            let Preload::Running {
                work,
                started,
                cold,
                expected_ms,
            } = preload
            else {
                return self.next_command(&[]).await;
            };
            let (started, cold, expected_ms) = (*started, *cold, *expected_ms);
            let woke = {
                let recv = std::pin::pin!(self.inputs.recv());
                let tick = std::pin::pin!(tokio::time::sleep(TICK));
                match select(work.as_mut(), select(recv, tick)).await {
                    Either::Left((loaded, _)) => Woke::Loaded(loaded),
                    Either::Right((Either::Left((input, _)), _)) => Woke::Input(input),
                    Either::Right((Either::Right(_), _)) => Woke::Tick,
                }
            };
            match woke {
                Woke::Loaded(loaded) => {
                    self.panel.preloading = false;
                    self.panel.load_progress = (cold && loaded.is_ok()).then_some(1.0);
                    *preload = Preload::Done(loaded);
                    emit(&self.app, &self.panel);
                }
                Woke::Tick => {
                    if cold {
                        let elapsed = started.elapsed().as_millis() as u64;
                        self.panel.load_progress = load_progress(elapsed, expected_ms);
                        emit(&self.app, &self.panel);
                    }
                }
                Woke::Input(None) => return SessionCommand::Cancel,
                Woke::Input(Some(input)) => {
                    if let Some(cmd) = self.handle_input(input, &[]) {
                        return cmd;
                    }
                }
            }
        }
    }

    async fn correct(&mut self, mode: SessionMode) -> Result<(), CorrectionErrorCode> {
        let app = self.app.clone();
        let captured =
            tauri::async_runtime::spawn_blocking(move || selection::capture_selection(&app))
                .await
                .map_err(|_| CorrectionErrorCode::ClipboardFailed)?
                .map_err(capture_error_code)?;
        check_text(&captured.text)?;

        let settings = get_correction_settings(&self.app);
        let (styles, adjusts) = split_tones(&session_tones(&settings));
        let choices = styles.len() + adjusts.len();
        self.panel.original = captured.text;
        self.panel.styles = styles.iter().map(|t| t.name.clone()).collect();
        self.panel.adjusts = adjusts.iter().map(|t| t.name.clone()).collect();

        let mut preload = self.prepare(&settings)?;
        // The first run: what the user picks, or the translation. An unknown
        // translation adjustment (deleted by the user) runs on the original
        // without being marked in the list.
        let (mut cmd, mut goal) = match mode {
            SessionMode::Correct => {
                self.enter(CorrectionPhase::Choosing, choices).await;
                let cmd = self.wait_choice(&mut preload).await;
                if !matches!(
                    cmd,
                    SessionCommand::RunStyle(_) | SessionCommand::RunAdjust(_)
                ) {
                    return Ok(());
                }
                let goal = goal_for(&cmd, &styles, &adjusts);
                (cmd, goal)
            }
            SessionMode::Translate => {
                let (index, goal) = translate_goal(&adjusts);
                (
                    index.map_or(SessionCommand::Ignore, SessionCommand::RunAdjust),
                    goal,
                )
            }
        };
        self.started = Instant::now();
        let Some(target) = self.resolve_target(preload, &settings).await? else {
            return Ok(());
        };

        let mut current: Option<String> = None;
        loop {
            let source = run_source(&cmd, &self.panel.original, current.as_deref()).to_string();
            mark_choice(&mut self.panel, &cmd);
            self.panel.view = CorrectionView::Corrected;
            let Some(text) = self
                .generate(&target, &settings.style_rules, &goal, &source)
                .await?
            else {
                return Ok(());
            };
            current = Some(text.clone());
            self.panel.text = text;
            self.panel.gen_progress = Some(1.0);
            self.panel.elapsed_ms = Some(self.started.elapsed().as_millis() as u64);
            self.enter(CorrectionPhase::Preview, choices).await;
            match self.next_command(&[]).await {
                SessionCommand::Apply => {
                    let text = self.panel.text.clone();
                    return self.apply(captured.hwnd, text).await;
                }
                SessionCommand::Copy => {
                    let text = self.panel.text.clone();
                    return self.copy(text).await;
                }
                next @ (SessionCommand::RunStyle(_) | SessionCommand::RunAdjust(_)) => {
                    goal = goal_for(&next, &styles, &adjusts);
                    cmd = next;
                    self.started = Instant::now();
                }
                _ => return Ok(()),
            }
        }
    }

    /// Decides where the requests go when the panel opens, and starts loading
    /// the local model if the GPU has room for it.
    fn prepare(&mut self, s: &CorrectionSettings) -> Result<Preload, CorrectionErrorCode> {
        let path = match &s.provider {
            CorrectionProvider::Cloud(m) => {
                self.panel.model_label = m.model.clone();
                return cloud_target(&self.app, m).map(|t| Preload::Done(Ok(t)));
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
        let expected_ms = s.last_load_ms.get(&path).copied();
        let engine = self.engine();
        let served = engine.serves(&model, true);
        if !served && !gpu_guard::has_room(gpu_guard::free_vram_bytes(), size) {
            return Ok(Preload::NoRoom {
                model,
                key: path,
                expected_ms,
            });
        }
        let cold = !served;
        if cold {
            self.panel.preloading = true;
            self.panel.load_progress = load_progress(0, expected_ms);
        }
        let work = local_load(
            self.app.clone(),
            engine,
            model,
            true,
            path,
            expected_ms,
            self.panel.model_label.clone(),
        );
        Ok(Preload::Running {
            work,
            started: Instant::now(),
            cold,
            expected_ms,
        })
    }

    /// The target once the user picked: the finished preload, the rest of a
    /// load still running (with the loading bar), or the GPU fallback prompt.
    /// `None` when the user cancelled.
    async fn resolve_target(
        &mut self,
        preload: Preload,
        s: &CorrectionSettings,
    ) -> Step<ChatTarget> {
        self.panel.preloading = false;
        match preload {
            Preload::Done(done) => done.map(Some),
            Preload::Running {
                work,
                started,
                cold,
                expected_ms,
            } => self.finish_load(work, started, cold, expected_ms).await,
            Preload::NoRoom {
                model,
                key,
                expected_ms,
            } => {
                let options = fallback_options(s.fallback.is_some());
                self.panel.fallback_options = options.clone();
                self.enter(CorrectionPhase::NeedsFallback, options.len())
                    .await;
                match self.next_command(&options).await {
                    SessionCommand::UseFallback(FallbackOption::Cloud) => {
                        let m = s.fallback.as_ref().expect("offered only when set");
                        self.panel.model_label = m.model.clone();
                        cloud_target(&self.app, m).map(Some)
                    }
                    SessionCommand::UseFallback(FallbackOption::Cpu) => {
                        let engine = self.engine();
                        let cold = !engine.serves(&model, false);
                        let work = local_load(
                            self.app.clone(),
                            engine,
                            model,
                            false,
                            key,
                            expected_ms,
                            self.panel.model_label.clone(),
                        );
                        self.finish_load(work, Instant::now(), cold, expected_ms)
                            .await
                    }
                    _ => Ok(None),
                }
            }
        }
    }

    /// Waits for a load to finish, showing the loading bar when a server has
    /// to start. Esc drops the load.
    async fn finish_load(
        &mut self,
        work: LoadFuture,
        started: Instant,
        cold: bool,
        expected_ms: Option<u64>,
    ) -> Step<ChatTarget> {
        let elapsed = || started.elapsed().as_millis() as u64;
        if cold {
            self.panel.load_progress = load_progress(elapsed(), expected_ms);
            self.enter(CorrectionPhase::Loading, 0).await;
        }
        let mut panel = self.panel.clone();
        let app = self.app.clone();
        let tick = move || {
            if cold {
                panel.load_progress = load_progress(elapsed(), expected_ms);
                emit(&app, &panel);
            }
        };
        let phase = CorrectionPhase::Loading;
        let Some(loaded) = until_cancelled(work, &mut self.inputs, &phase, tick, |_| {}).await
        else {
            return Ok(None);
        };
        let target = loaded?;
        self.panel.load_progress = cold.then_some(1.0);
        Ok(Some(target))
    }

    /// Streams one run over `source` (the original for a style, the current
    /// result for an adjustment) into the panel. `None` when the user
    /// pressed Esc.
    async fn generate(
        &mut self,
        target: &ChatTarget,
        rules: &str,
        goal: &str,
        source: &str,
    ) -> Step<String> {
        self.panel.text.clear();
        self.panel.gen_progress = Some(0.0);
        self.panel.elapsed_ms = None;
        self.enter(CorrectionPhase::Generating, 0).await;

        let source = source.to_string();
        let (system, user) = build_messages(rules, goal, &source);
        let input_chars = source.chars().count();
        // Shared with the scroll handler so a scroll event carries the text so far.
        let shared = Arc::new(Mutex::new(self.panel.clone()));
        let app = self.app.clone();
        let mut raw = String::new();
        let mut last_emit: Option<Instant> = None;
        let on_delta = {
            let (shared, app, source) = (shared.clone(), app.clone(), source.clone());
            move |delta: &str| {
                raw.push_str(delta);
                if last_emit.is_some_and(|t| t.elapsed() < EMIT_INTERVAL) {
                    return;
                }
                last_emit = Some(Instant::now());
                let mut panel = lock(&shared);
                panel.text = clean_output(&source, &raw);
                panel.gen_progress = Some(generation_progress(raw.chars().count(), input_chars));
                emit(&app, &panel);
            }
        };
        let on_scroll = {
            let (shared, app) = (shared.clone(), app.clone());
            move |dir: i8| {
                let mut panel = lock(&shared);
                panel.scroll_nonce = panel.scroll_nonce.wrapping_add(1);
                panel.scroll_dir = dir;
                emit(&app, &panel);
            }
        };
        let phase = CorrectionPhase::Generating;
        let streaming = stream_chat(target, &system, &user, on_delta);
        let result = until_cancelled(streaming, &mut self.inputs, &phase, || {}, on_scroll).await;
        // Keep the scroll count going into the preview.
        self.panel.scroll_nonce = lock(&shared).scroll_nonce;
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
        let text = clean_output(&source, &raw);
        if text.is_empty() {
            warn!("Correction reply was empty after cleanup");
            return Err(failed);
        }
        Ok(Some(text))
    }

    /// Leaves the correction on the clipboard (on purpose) and closes.
    async fn copy(&mut self, text: String) -> Result<(), CorrectionErrorCode> {
        set_bindings(&self.app, Vec::new()).await;
        overlay::hide_recording_overlay(&self.app);
        let app = self.app.clone();
        // Under the paste lock, like the "copied" fallback of `apply`.
        let done = tauri::async_runtime::spawn_blocking(move || {
            crate::clipboard::with_paste_lock(|| {
                crate::clipboard::write_text_to_clipboard(&app, &text)
            })
        })
        .await;
        match done {
            Ok(Ok(())) => {
                self.hold(CorrectionPhase::Copied, COPY_HOLD).await;
                Ok(())
            }
            Ok(Err(e)) => {
                warn!("Could not copy the correction: {e}");
                Err(CorrectionErrorCode::ClipboardFailed)
            }
            Err(e) => {
                warn!("Copy task failed: {e}");
                Err(CorrectionErrorCode::ClipboardFailed)
            }
        }
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
            Ok(Ok(true)) => {
                info!("Correction applied: pasted");
                self.hold(CorrectionPhase::Applied, APPLIED_HOLD).await
            }
            Ok(Ok(false)) => {
                info!("Correction applied: refocus failed, left on clipboard");
                self.hold(CorrectionPhase::Copied, COPIED_HOLD).await
            }
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
                decide(&p, SessionInput::Escape, 4, 0, &[]),
                SessionCommand::Cancel
            );
        }
    }

    #[test]
    fn preview_enter_applies_and_digits_pick_tones() {
        let p = CorrectionPhase::Preview;
        assert_eq!(
            decide(&p, SessionInput::Enter, 4, 0, &[]),
            SessionCommand::Apply
        );
        assert_eq!(
            decide(&p, SessionInput::Digit(2), 4, 0, &[]),
            SessionCommand::RunStyle(1)
        );
        assert_eq!(
            decide(&p, SessionInput::Digit(5), 4, 0, &[]),
            SessionCommand::Ignore
        );
    }

    #[test]
    fn fallback_digits_pick_options_and_enter_takes_the_first() {
        let opts = [FallbackOption::Cloud, FallbackOption::Cpu];
        let p = CorrectionPhase::NeedsFallback;
        assert_eq!(
            decide(&p, SessionInput::Digit(2), 4, 0, &opts),
            SessionCommand::UseFallback(FallbackOption::Cpu)
        );
        assert_eq!(
            decide(&p, SessionInput::Enter, 4, 0, &opts),
            SessionCommand::UseFallback(FallbackOption::Cloud)
        );
        assert_eq!(
            decide(&p, SessionInput::Escape, 4, 0, &opts),
            SessionCommand::Cancel
        );
        assert_eq!(
            decide(&p, SessionInput::Digit(3), 4, 0, &opts),
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
                    decide(&p, input, 4, 0, &[FallbackOption::Cpu]),
                    SessionCommand::Ignore
                );
            }
        }
    }

    #[test]
    fn working_phases_ignore_enter_and_digits() {
        for p in [CorrectionPhase::Loading, CorrectionPhase::Generating] {
            assert_eq!(
                decide(&p, SessionInput::Enter, 4, 0, &[]),
                SessionCommand::Ignore
            );
            assert_eq!(
                decide(&p, SessionInput::Digit(1), 4, 0, &[]),
                SessionCommand::Ignore
            );
        }
    }

    #[test]
    fn digit_zero_and_tones_past_nine_are_ignored() {
        let p = CorrectionPhase::Preview;
        assert_eq!(
            decide(&p, SessionInput::Digit(0), 12, 0, &[]),
            SessionCommand::Ignore
        );
        assert_eq!(
            decide(&p, SessionInput::Digit(9), 12, 0, &[]),
            SessionCommand::RunStyle(8)
        );
    }

    #[test]
    fn ui_actions_map_to_commands_in_preview() {
        let p = CorrectionPhase::Preview;
        assert_eq!(
            decide(&p, SessionInput::Ui(UiAction::Apply), 6, 0, &[]),
            SessionCommand::Apply
        );
        assert_eq!(
            decide(&p, SessionInput::Ui(UiAction::Copy), 6, 0, &[]),
            SessionCommand::Copy
        );
        assert_eq!(
            decide(&p, SessionInput::CopyKey, 6, 0, &[]),
            SessionCommand::Copy
        );
        assert_eq!(
            decide(&p, SessionInput::Ui(UiAction::Tone(3)), 6, 0, &[]),
            SessionCommand::RunStyle(2)
        );
        assert_eq!(
            decide(&p, SessionInput::Ui(UiAction::Tone(9)), 6, 0, &[]),
            SessionCommand::Ignore
        );
        assert_eq!(
            decide(
                &p,
                SessionInput::Ui(UiAction::View(CorrectionView::Original)),
                6,
                0,
                &[]
            ),
            SessionCommand::SetView(CorrectionView::Original)
        );
    }

    #[test]
    fn copy_apply_and_view_buttons_do_nothing_outside_the_preview() {
        for p in [
            CorrectionPhase::Loading,
            CorrectionPhase::Generating,
            CorrectionPhase::NeedsFallback,
            CorrectionPhase::Applied,
        ] {
            for i in [
                SessionInput::CopyKey,
                SessionInput::Ui(UiAction::Copy),
                SessionInput::Ui(UiAction::Apply),
                SessionInput::Ui(UiAction::View(CorrectionView::Changes)),
            ] {
                assert_eq!(
                    decide(&p, i, 6, 0, &[FallbackOption::Cpu]),
                    SessionCommand::Ignore
                );
            }
        }
    }

    #[test]
    fn tone_click_picks_the_fallback_option_like_the_digit() {
        let opts = [FallbackOption::Cloud, FallbackOption::Cpu];
        let p = CorrectionPhase::NeedsFallback;
        assert_eq!(
            decide(&p, SessionInput::Ui(UiAction::Tone(2)), 2, 0, &opts),
            SessionCommand::UseFallback(FallbackOption::Cpu)
        );
        assert_eq!(
            decide(&p, SessionInput::Ui(UiAction::Tone(3)), 2, 0, &opts),
            SessionCommand::Ignore
        );
    }

    #[test]
    fn shortcut_again_or_cancel_button_closes_any_open_phase() {
        for p in [
            CorrectionPhase::Loading,
            CorrectionPhase::Generating,
            CorrectionPhase::Preview,
            CorrectionPhase::NeedsFallback,
        ] {
            assert_eq!(
                decide(&p, SessionInput::Shortcut, 6, 0, &[FallbackOption::Cpu]),
                SessionCommand::Cancel
            );
            assert_eq!(
                decide(
                    &p,
                    SessionInput::Ui(UiAction::Cancel),
                    6,
                    0,
                    &[FallbackOption::Cpu]
                ),
                SessionCommand::Cancel
            );
        }
    }

    #[test]
    fn copy_key_only_bound_in_preview() {
        for p in [
            CorrectionPhase::Loading,
            CorrectionPhase::Generating,
            CorrectionPhase::NeedsFallback,
            CorrectionPhase::Applied,
            CorrectionPhase::Copied,
            CorrectionPhase::Error,
            CorrectionPhase::Hidden,
        ] {
            assert!(!wanted_bindings(&p, 4)
                .iter()
                .any(|b| b == "correction_copy"));
        }
        assert!(wanted_bindings(&CorrectionPhase::Preview, 4)
            .iter()
            .any(|b| b == "correction_copy"));
        assert_eq!(binding_key("correction_copy").as_deref(), Some("ctrl+c"));
        assert_eq!(
            input_for_binding("correction_copy"),
            Some(SessionInput::CopyKey)
        );
        assert!(TEMPORARY_BINDING_IDS.contains(&"correction_copy"));
    }

    #[test]
    fn shortcut_pressed_with_an_open_session_closes_it() {
        assert_eq!(shortcut_route(false, true), ShortcutRoute::Close);
        assert_eq!(shortcut_route(true, true), ShortcutRoute::Close);
        assert_eq!(shortcut_route(false, false), ShortcutRoute::Start);
        assert_eq!(shortcut_route(true, false), ShortcutRoute::Ignore);
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
            [
                "correction_cancel",
                "correction_scroll_up",
                "correction_scroll_down"
            ]
        );
        assert_eq!(
            wanted_bindings(&CorrectionPhase::Preview, 2),
            [
                "correction_cancel",
                "correction_apply",
                "correction_tone_1",
                "correction_tone_2",
                "correction_view",
                "correction_scroll_up",
                "correction_scroll_down",
                "correction_copy"
            ]
        );
        assert_eq!(
            wanted_bindings(&CorrectionPhase::Preview, 12).len(),
            2 + 9 + 3 + 1
        );
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
    fn view_cycles_corrected_changes_original() {
        assert_eq!(CorrectionView::Corrected.next(), CorrectionView::Changes);
        assert_eq!(CorrectionView::Changes.next(), CorrectionView::Original);
        assert_eq!(CorrectionView::Original.next(), CorrectionView::Corrected);
        assert_eq!(CorrectionView::default(), CorrectionView::Corrected);
    }

    #[test]
    fn tab_switches_view_only_in_preview() {
        assert_eq!(
            decide(&CorrectionPhase::Preview, SessionInput::Tab, 4, 0, &[]),
            SessionCommand::NextView
        );
        for p in [
            CorrectionPhase::Loading,
            CorrectionPhase::Generating,
            CorrectionPhase::NeedsFallback,
            CorrectionPhase::Applied,
        ] {
            assert_eq!(
                decide(&p, SessionInput::Tab, 4, 0, &[]),
                SessionCommand::Ignore
            );
        }
    }

    #[test]
    fn arrows_scroll_while_generating_or_previewing() {
        for p in [CorrectionPhase::Generating, CorrectionPhase::Preview] {
            assert_eq!(
                decide(&p, SessionInput::ScrollDown, 4, 0, &[]),
                SessionCommand::Scroll(1)
            );
            assert_eq!(
                decide(&p, SessionInput::ScrollUp, 4, 0, &[]),
                SessionCommand::Scroll(-1)
            );
        }
        for p in [
            CorrectionPhase::Loading,
            CorrectionPhase::NeedsFallback,
            CorrectionPhase::Error,
        ] {
            assert_eq!(
                decide(&p, SessionInput::ScrollDown, 4, 0, &[]),
                SessionCommand::Ignore
            );
        }
    }

    #[test]
    fn view_and_scroll_keys_are_listened_to_while_generating_and_previewing() {
        let scroll = ["correction_scroll_up", "correction_scroll_down"];
        let gen = wanted_bindings(&CorrectionPhase::Generating, 4);
        assert!(scroll.iter().all(|k| gen.iter().any(|g| g == k)));
        assert!(!gen.iter().any(|g| g == "correction_view"));
        let prev = wanted_bindings(&CorrectionPhase::Preview, 4);
        assert!(scroll.iter().all(|k| prev.iter().any(|g| g == k)));
        assert!(prev.iter().any(|g| g == "correction_view"));
        let fb = wanted_bindings(&CorrectionPhase::NeedsFallback, 2);
        assert!(!fb
            .iter()
            .any(|g| g.starts_with("correction_s") || g == "correction_view"));
    }

    #[test]
    fn tab_and_arrow_bindings_use_key_names_both_backends_parse() {
        assert_eq!(binding_key("correction_view").as_deref(), Some("tab"));
        assert_eq!(binding_key("correction_scroll_up").as_deref(), Some("up"));
        assert_eq!(
            binding_key("correction_scroll_down").as_deref(),
            Some("down")
        );
        assert_eq!(
            input_for_binding("correction_view"),
            Some(SessionInput::Tab)
        );
        assert_eq!(
            input_for_binding("correction_scroll_up"),
            Some(SessionInput::ScrollUp)
        );
        assert_eq!(
            input_for_binding("correction_scroll_down"),
            Some(SessionInput::ScrollDown)
        );
        for id in [
            "correction_view",
            "correction_scroll_up",
            "correction_scroll_down",
        ] {
            assert!(
                TEMPORARY_BINDING_IDS.contains(&id),
                "{id} must be registered as an action"
            );
        }
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
            kind: Default::default(),
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
            |_| {},
        ));
        assert!(out.is_none());
        assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
        assert!(rx.try_recv().is_err(), "every queued key was consumed");
    }

    #[test]
    fn scroll_keys_reach_the_handler_without_stopping_the_work() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        tx.send(SessionInput::ScrollDown).unwrap();
        tx.send(SessionInput::ScrollUp).unwrap();
        tx.send(SessionInput::Tab).unwrap();
        let mut scrolls = Vec::new();
        let out = tauri::async_runtime::block_on(until_cancelled(
            async {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                5
            },
            &mut rx,
            &CorrectionPhase::Generating,
            || {},
            |d| scrolls.push(d),
        ));
        assert_eq!(out, Some(5));
        assert_eq!(scrolls, [1, -1]);
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
            |_| {},
        ));
        assert_eq!(out, Some(7));
        assert!(ticks >= 1, "ticked {ticks} times");
    }

    #[test]
    fn sounds_play_on_start_once_and_on_each_preview() {
        use CorrectionPhase::*;
        assert_eq!(
            sound_for_transition(&Hidden, &Loading, false),
            Some(CorrectionSound::Start)
        );
        assert_eq!(sound_for_transition(&Loading, &Generating, true), None);
        assert_eq!(
            sound_for_transition(&Hidden, &Generating, false),
            Some(CorrectionSound::Start)
        );
        assert_eq!(
            sound_for_transition(&Generating, &Preview, true),
            Some(CorrectionSound::Ready)
        );
        assert_eq!(sound_for_transition(&Preview, &Generating, true), None);
        assert_eq!(sound_for_transition(&Generating, &Error, true), None);
    }

    #[test]
    fn dismiss_closes_open_phases_only() {
        for p in [
            CorrectionPhase::Loading,
            CorrectionPhase::Generating,
            CorrectionPhase::Preview,
            CorrectionPhase::NeedsFallback,
        ] {
            assert_eq!(
                decide(&p, SessionInput::Dismiss, 6, 0, &[FallbackOption::Cpu]),
                SessionCommand::Cancel
            );
        }
        for p in [
            CorrectionPhase::Applied,
            CorrectionPhase::Copied,
            CorrectionPhase::Error,
            CorrectionPhase::Hidden,
        ] {
            assert_eq!(
                decide(&p, SessionInput::Dismiss, 6, 0, &[]),
                SessionCommand::Ignore
            );
        }
    }

    #[test]
    fn hook_events_dismiss_on_outside_click_or_typing() {
        let rect = Some((100, 100, 860, 560));
        let none: Vec<String> = Vec::new();
        assert!(!hook_dismisses(
            HookEvent::MouseDown { x: 300, y: 300 },
            rect,
            &none
        ));
        assert!(hook_dismisses(
            HookEvent::MouseDown { x: 50, y: 300 },
            rect,
            &none
        ));
        // Unknown panel position: never guess.
        assert!(!hook_dismisses(
            HookEvent::MouseDown { x: 50, y: 300 },
            None,
            &none
        ));
        assert!(hook_dismisses(
            HookEvent::TypingKey { vk: 0x41 },
            rect,
            &none
        ));
        // A digit the panel listens to is a tone key, not typing...
        let preview = wanted_bindings(&CorrectionPhase::Preview, 4);
        assert!(!hook_dismisses(
            HookEvent::TypingKey { vk: 0x32 },
            rect,
            &preview
        ));
        // ...but a digit with no tone, or any digit while loading, is typing.
        assert!(hook_dismisses(
            HookEvent::TypingKey { vk: 0x37 },
            rect,
            &preview
        ));
        let loading = wanted_bindings(&CorrectionPhase::Loading, 4);
        assert!(hook_dismisses(
            HookEvent::TypingKey { vk: 0x32 },
            rect,
            &loading
        ));
    }

    #[test]
    fn mouse_hook_only_while_the_panel_is_open() {
        for p in [
            CorrectionPhase::Loading,
            CorrectionPhase::Generating,
            CorrectionPhase::Preview,
            CorrectionPhase::NeedsFallback,
        ] {
            assert!(wants_mouse_hook(&p), "{p:?}");
        }
        for p in [
            CorrectionPhase::Applied,
            CorrectionPhase::Copied,
            CorrectionPhase::Error,
            CorrectionPhase::Hidden,
        ] {
            assert!(!wants_mouse_hook(&p), "{p:?}");
        }
    }

    #[test]
    fn choosing_numbers_pick_a_style_or_an_adjustment() {
        let p = CorrectionPhase::Choosing;
        let cases = [
            (1, SessionCommand::RunStyle(0)),
            (4, SessionCommand::RunStyle(3)),
            (5, SessionCommand::RunAdjust(0)),
            (8, SessionCommand::RunAdjust(3)),
            (9, SessionCommand::Ignore),
            (0, SessionCommand::Ignore),
        ];
        for (d, want) in cases {
            assert_eq!(decide(&p, SessionInput::Digit(d), 4, 4, &[]), want, "{d}");
        }
        assert_eq!(
            decide(&p, SessionInput::Ui(UiAction::Tone(6)), 4, 4, &[]),
            SessionCommand::RunAdjust(1)
        );
    }

    #[test]
    fn choosing_enter_runs_the_first_style() {
        let p = CorrectionPhase::Choosing;
        assert_eq!(
            decide(&p, SessionInput::Enter, 4, 4, &[]),
            SessionCommand::RunStyle(0)
        );
        // No style to run: Enter does nothing.
        assert_eq!(
            decide(&p, SessionInput::Enter, 0, 4, &[]),
            SessionCommand::Ignore
        );
    }

    #[test]
    fn choosing_closes_on_escape_shortcut_click_outside_or_cancel() {
        let p = CorrectionPhase::Choosing;
        for i in [
            SessionInput::Escape,
            SessionInput::Shortcut,
            SessionInput::Dismiss,
            SessionInput::Ui(UiAction::Cancel),
        ] {
            assert_eq!(decide(&p, i, 4, 4, &[]), SessionCommand::Cancel, "{i:?}");
        }
    }

    #[test]
    fn choosing_has_nothing_to_apply_copy_or_switch_view() {
        let p = CorrectionPhase::Choosing;
        for i in [
            SessionInput::Ui(UiAction::Apply),
            SessionInput::Ui(UiAction::Copy),
            SessionInput::CopyKey,
            SessionInput::Tab,
            SessionInput::Ui(UiAction::View(CorrectionView::Changes)),
        ] {
            assert_eq!(decide(&p, i, 4, 4, &[]), SessionCommand::Ignore, "{i:?}");
        }
        assert_eq!(
            decide(&p, SessionInput::ScrollDown, 4, 4, &[]),
            SessionCommand::Scroll(1)
        );
    }

    #[test]
    fn preview_style_restarts_and_adjustment_chains() {
        let p = CorrectionPhase::Preview;
        assert_eq!(
            decide(&p, SessionInput::Digit(2), 4, 4, &[]),
            SessionCommand::RunStyle(1)
        );
        assert_eq!(
            decide(&p, SessionInput::Digit(7), 4, 4, &[]),
            SessionCommand::RunAdjust(2)
        );
        assert_eq!(
            decide(&p, SessionInput::Ui(UiAction::Tone(5)), 4, 4, &[]),
            SessionCommand::RunAdjust(0)
        );
        assert_eq!(
            decide(&p, SessionInput::Enter, 4, 4, &[]),
            SessionCommand::Apply
        );
        assert_eq!(
            decide(&p, SessionInput::Digit(9), 4, 4, &[]),
            SessionCommand::Ignore
        );
    }

    #[test]
    fn numbers_past_nine_are_reached_only_by_click() {
        // 7 styles + 6 adjustments = 13 options.
        assert_eq!(
            option_for_number(9, 7, 6),
            Some(SessionCommand::RunAdjust(1))
        );
        assert_eq!(
            option_for_number(12, 7, 6),
            Some(SessionCommand::RunAdjust(4))
        );
        assert_eq!(
            option_for_number(13, 7, 6),
            Some(SessionCommand::RunAdjust(5))
        );
        assert_eq!(option_for_number(14, 7, 6), None);
        assert_eq!(option_for_number(0, 7, 6), None);
        assert_eq!(
            option_for_number(7, 7, 6),
            Some(SessionCommand::RunStyle(6))
        );
        for p in [CorrectionPhase::Choosing, CorrectionPhase::Preview] {
            assert_eq!(
                decide(&p, SessionInput::Digit(10), 7, 6, &[]),
                SessionCommand::Ignore
            );
            assert_eq!(
                decide(&p, SessionInput::Ui(UiAction::Tone(12)), 7, 6, &[]),
                SessionCommand::RunAdjust(4)
            );
            assert_eq!(
                decide(&p, SessionInput::Ui(UiAction::Tone(14)), 7, 6, &[]),
                SessionCommand::Ignore
            );
        }
    }

    #[test]
    fn choosing_listens_to_enter_escape_the_option_digits_and_arrows() {
        assert_eq!(
            wanted_bindings(&CorrectionPhase::Choosing, 3),
            [
                "correction_cancel",
                "correction_apply",
                "correction_tone_1",
                "correction_tone_2",
                "correction_tone_3",
                "correction_scroll_up",
                "correction_scroll_down"
            ]
        );
        assert_eq!(
            wanted_bindings(&CorrectionPhase::Choosing, 13).len(),
            2 + 9 + 2
        );
    }

    #[test]
    fn choosing_is_an_open_panel_without_a_sound() {
        let p = CorrectionPhase::Choosing;
        assert!(wants_mouse_hook(&p));
        assert_eq!(
            sound_for_transition(&CorrectionPhase::Hidden, &p, false),
            None
        );
        // The start sound waits for the choice.
        assert_eq!(
            sound_for_transition(&p, &CorrectionPhase::Loading, false),
            Some(CorrectionSound::Start)
        );
        assert_eq!(
            sound_for_transition(&p, &CorrectionPhase::Generating, false),
            Some(CorrectionSound::Start)
        );
        assert_eq!(
            decide(&p, SessionInput::Dismiss, 4, 4, &[]),
            SessionCommand::Cancel
        );
        // A bound option digit is not typing; a letter is.
        let bound = wanted_bindings(&p, 8);
        assert!(!hook_dismisses(
            HookEvent::TypingKey { vk: 0x35 },
            None,
            &bound
        ));
        assert!(hook_dismisses(
            HookEvent::TypingKey { vk: 0x41 },
            None,
            &bound
        ));
    }

    fn tone(id: &str, kind: crate::correction::settings::ToneKind) -> CorrectionTone {
        CorrectionTone {
            id: id.into(),
            name: id.to_uppercase(),
            goal: format!("goal {id}"),
            kind,
        }
    }

    #[test]
    fn tones_split_into_styles_then_adjustments() {
        use crate::correction::settings::ToneKind::{Adjust, Style};
        let tones = [
            tone("a", Adjust),
            tone("s1", Style),
            tone("b", Adjust),
            tone("s2", Style),
        ];
        let (styles, adjusts) = split_tones(&tones);
        let ids = |v: &[CorrectionTone]| v.iter().map(|t| t.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&styles), ["s1", "s2"]);
        assert_eq!(ids(&adjusts), ["a", "b"]);
    }

    #[test]
    fn translation_uses_the_traduzir_adjustment_or_the_default_goal() {
        use crate::correction::settings::ToneKind::Adjust;
        let mut adjusts = vec![tone("encurtar", Adjust), tone("traduzir", Adjust)];
        assert_eq!(
            translate_goal(&adjusts),
            (Some(1), "goal traduzir".to_string())
        );
        adjusts.retain(|t| t.id != "traduzir");
        let (index, goal) = translate_goal(&adjusts);
        assert_eq!(index, None);
        let default = crate::correction::settings::default_tones()
            .into_iter()
            .find(|t| t.id == "traduzir")
            .unwrap();
        assert_eq!(goal, default.goal);
    }

    #[test]
    fn a_style_starts_from_the_original_and_an_adjustment_from_the_result() {
        let original = "orig";
        assert_eq!(
            run_source(&SessionCommand::RunStyle(1), original, Some("now")),
            "orig"
        );
        assert_eq!(
            run_source(&SessionCommand::RunAdjust(0), original, Some("now")),
            "now"
        );
        // Picked before any style: the adjustment works on the original.
        assert_eq!(
            run_source(&SessionCommand::RunAdjust(0), original, None),
            "orig"
        );
    }

    #[test]
    fn a_style_resets_the_adjustments_and_adjustments_chain() {
        let mut panel = CorrectionUpdateEvent::hidden();
        mark_choice(&mut panel, &SessionCommand::RunStyle(1));
        assert_eq!(panel.current_style, Some(1));
        mark_choice(&mut panel, &SessionCommand::RunAdjust(0));
        mark_choice(&mut panel, &SessionCommand::RunAdjust(2));
        assert_eq!(panel.applied_adjusts, [0, 2]);
        mark_choice(&mut panel, &SessionCommand::RunStyle(3));
        assert_eq!(panel.current_style, Some(3));
        assert!(panel.applied_adjusts.is_empty());
    }

    #[test]
    fn translate_shortcut_is_a_correction_action_not_a_temporary_key() {
        assert!(!TEMPORARY_BINDING_IDS.contains(&"translate_selection"));
        assert!(crate::settings::get_default_settings()
            .bindings
            .contains_key("translate_selection"));
    }
}
