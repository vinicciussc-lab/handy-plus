//! Runs `llama-server.exe` (llama.cpp) as a helper process for local correction.
//!
//! The server is started on a free loopback port, is considered ready once
//! `GET /health` answers 200 (503 while the model loads), and is killed on
//! `unload`, on `Drop` and when the app exits.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::correction::text::load_progress;

const POLL_INTERVAL: Duration = Duration::from_millis(200);
const LOAD_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug)]
pub enum EngineError {
    BinaryMissing,
    ModelMissing(String),
    StartFailed(String),
    Crashed,
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BinaryMissing => write!(f, "llama-server.exe not found"),
            Self::ModelMissing(p) => write!(f, "model file not found: {p}"),
            Self::StartFailed(e) => write!(f, "could not start llama-server: {e}"),
            Self::Crashed => write!(f, "llama-server stopped unexpectedly"),
        }
    }
}

impl std::error::Error for EngineError {}

struct Running {
    child: Child,
    port: u16,
    model_path: PathBuf,
    gpu: bool,
    /// True once `/health` answered 200.
    ready: bool,
    last_used: Instant,
}

impl Running {
    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn has_exited(&mut self) -> bool {
        !matches!(self.child.try_wait(), Ok(None))
    }
}

pub struct LocalEngine {
    binary: PathBuf,
    state: Mutex<Option<Running>>,
    /// Serializes `ensure_loaded` so two callers never start two servers.
    load_lock: tokio::sync::Mutex<()>,
    /// Children are assigned to this job so Windows kills them if the app dies.
    #[cfg(windows)]
    job: Option<job::KillOnCloseJob>,
    #[cfg(test)]
    args_override: Option<Vec<String>>,
    #[cfg(test)]
    last_pid: std::sync::atomic::AtomicU32,
}

impl LocalEngine {
    pub fn new(binary: PathBuf) -> Self {
        Self {
            binary,
            state: Mutex::new(None),
            load_lock: tokio::sync::Mutex::new(()),
            #[cfg(windows)]
            job: job::KillOnCloseJob::new(),
            #[cfg(test)]
            args_override: None,
            #[cfg(test)]
            last_pid: std::sync::atomic::AtomicU32::new(0),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Running>> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Opens (or reuses) the server for `model`. `on_progress` receives
    /// `load_progress(...)` every 200 ms while loading.
    pub async fn ensure_loaded(
        &self,
        model: &Path,
        gpu: bool,
        expected_ms: Option<u64>,
        on_progress: impl Fn(Option<f32>) + Send,
    ) -> Result<(String, u64), EngineError> {
        if !self.binary.is_file() {
            return Err(EngineError::BinaryMissing);
        }
        if !model.is_file() {
            return Err(EngineError::ModelMissing(model.display().to_string()));
        }
        let _serial = self.load_lock.lock().await;

        // Reuse a live server for the same model and device; otherwise drop it.
        {
            let mut guard = self.lock();
            if let Some(r) = guard.as_mut() {
                if r.ready && !r.has_exited() && r.model_path == model && r.gpu == gpu {
                    r.last_used = Instant::now();
                    return Ok((base_url(r.port), 0));
                }
            }
            if let Some(mut old) = guard.take() {
                old.kill();
            }
        }

        let port = free_port().map_err(|e| EngineError::StartFailed(e.to_string()))?;
        let mut cmd = Command::new(&self.binary);
        #[cfg(test)]
        let args = self
            .args_override
            .clone()
            .unwrap_or_else(|| server_args(model, port, gpu));
        #[cfg(not(test))]
        let args = server_args(model, port, gpu);
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(dir) = self.binary.parent() {
            cmd.current_dir(dir);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let child = cmd
            .spawn()
            .map_err(|e| EngineError::StartFailed(e.to_string()))?;
        #[cfg(test)]
        self.last_pid
            .store(child.id(), std::sync::atomic::Ordering::Relaxed);
        #[cfg(windows)]
        if let Some(job) = &self.job {
            if let Err(e) = job.assign(&child) {
                log::warn!("could not add llama-server to the kill-on-close job: {e}");
            }
        }
        *self.lock() = Some(Running {
            child,
            port,
            model_path: model.to_path_buf(),
            gpu,
            ready: false,
            last_used: Instant::now(),
        });

        // Cancelled mid-load (future dropped): kill the half-loaded server.
        let mut guard_on_drop = LoadGuard {
            engine: self,
            port,
            armed: true,
        };
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .map_err(|e| EngineError::StartFailed(e.to_string()))?;
        let health = format!("{}/health", base_url(port));
        let started = Instant::now();
        loop {
            {
                // Killed by `unload`, or died on its own?
                let mut guard = self.lock();
                match guard.as_mut() {
                    Some(r) if r.port == port => {
                        if r.has_exited() {
                            *guard = None;
                            return Err(EngineError::Crashed);
                        }
                    }
                    _ => {
                        return Err(EngineError::StartFailed("unloaded while loading".into()));
                    }
                }
            }
            let elapsed = started.elapsed();
            if elapsed >= LOAD_TIMEOUT {
                self.kill_port(port);
                return Err(EngineError::StartFailed(format!(
                    "timed out after {} s",
                    LOAD_TIMEOUT.as_secs()
                )));
            }
            if let Ok(resp) = client.get(&health).send().await {
                if resp.status().is_success() {
                    let mut guard = self.lock();
                    if let Some(r) = guard.as_mut().filter(|r| r.port == port) {
                        r.ready = true;
                        r.last_used = Instant::now();
                    }
                    drop(guard);
                    guard_on_drop.armed = false;
                    on_progress(Some(1.0));
                    return Ok((base_url(port), started.elapsed().as_millis() as u64));
                }
            }
            on_progress(load_progress(elapsed.as_millis() as u64, expected_ms));
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    fn kill_port(&self, port: u16) {
        let mut guard = self.lock();
        if guard.as_ref().is_some_and(|r| r.port == port) {
            if let Some(mut r) = guard.take() {
                r.kill();
            }
        }
    }

    pub fn unload(&self) {
        if let Some(mut r) = self.lock().take() {
            r.kill();
        }
    }

    /// True only for a ready server whose process is still alive.
    pub fn is_loaded(&self) -> bool {
        let mut guard = self.lock();
        let Some(r) = guard.as_mut() else {
            return false;
        };
        if r.has_exited() {
            *guard = None;
            return false;
        }
        r.ready
    }

    /// True when a ready, live server already runs `model` on the same device,
    /// so `ensure_loaded` would reuse it without loading anything.
    pub fn serves(&self, model: &Path, gpu: bool) -> bool {
        let mut guard = self.lock();
        let Some(r) = guard.as_mut() else {
            return false;
        };
        if r.has_exited() {
            *guard = None;
            return false;
        }
        r.ready && r.model_path == model && r.gpu == gpu
    }

    /// Kills the server unless `busy` is set. `busy` is read (SeqCst) while
    /// holding the same state lock `ensure_loaded` uses, so a session that sets
    /// its flag before calling `ensure_loaded` is never unloaded under it:
    /// either this sees the flag and skips, or it unloads first and
    /// `ensure_loaded` reloads. The caller must also `touch()` when the session
    /// ends, before clearing the flag. Returns whether it unloaded.
    pub fn unload_unless_busy(&self, busy: &std::sync::atomic::AtomicBool) -> bool {
        let mut guard = self.lock();
        if busy.load(std::sync::atomic::Ordering::SeqCst) {
            return false;
        }
        match guard.take() {
            Some(mut r) => {
                r.kill();
                true
            }
            None => false,
        }
    }

    /// Marks use and restarts the idle clock.
    pub fn touch(&self) {
        if let Some(r) = self.lock().as_mut() {
            r.last_used = Instant::now();
        }
    }

    pub fn idle_for(&self) -> Option<Duration> {
        self.lock()
            .as_ref()
            .filter(|r| r.ready)
            .map(|r| r.last_used.elapsed())
    }

    /// Test helper: force-kills the child without clearing state.
    #[cfg(test)]
    fn kill_child_for_test(&self) {
        if let Some(r) = self.lock().as_mut() {
            let _ = r.child.kill();
            let _ = r.child.wait();
        }
    }
}

struct LoadGuard<'a> {
    engine: &'a LocalEngine,
    port: u16,
    armed: bool,
}

impl Drop for LoadGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.engine.kill_port(self.port);
        }
    }
}

#[cfg(windows)]
mod job {
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    /// A job object that kills every process in it when its last handle closes.
    pub struct KillOnCloseJob(HANDLE);

    // The handle is only used through thread-safe Win32 calls.
    unsafe impl Send for KillOnCloseJob {}
    unsafe impl Sync for KillOnCloseJob {}

    impl KillOnCloseJob {
        pub fn new() -> Option<Self> {
            unsafe {
                let handle = match CreateJobObjectW(None, None) {
                    Ok(h) => h,
                    Err(e) => {
                        log::warn!("CreateJobObjectW failed: {e}");
                        return None;
                    }
                };
                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                if let Err(e) = SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                ) {
                    log::warn!("SetInformationJobObject failed: {e}");
                    let _ = CloseHandle(handle);
                    return None;
                }
                Some(Self(handle))
            }
        }

        pub fn assign(&self, child: &Child) -> windows::core::Result<()> {
            unsafe { AssignProcessToJobObject(self.0, HANDLE(child.as_raw_handle())) }
        }
    }

    impl Drop for KillOnCloseJob {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

impl Drop for LocalEngine {
    fn drop(&mut self) {
        self.unload();
    }
}

fn base_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

fn free_port() -> std::io::Result<u16> {
    Ok(TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

pub fn server_args(model: &Path, port: u16, gpu: bool) -> Vec<String> {
    [
        "-m",
        &model.to_string_lossy(),
        "--host",
        "127.0.0.1",
        "--port",
        &port.to_string(),
        "-ngl",
        if gpu { "999" } else { "0" },
        "--jinja",
        "--reasoning-budget",
        "0",
        "-c",
        "4096",
        "--no-webui",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_args_offload_all_layers_and_disable_thinking() {
        let a = server_args(Path::new(r"D:\m\g.gguf"), 8099, true);
        for want in [
            "-m",
            r"D:\m\g.gguf",
            "--host",
            "127.0.0.1",
            "--port",
            "8099",
            "-ngl",
            "999",
            "--jinja",
            "--reasoning-budget",
            "0",
            "-c",
            "4096",
            "--no-webui",
        ] {
            assert!(a.contains(&want.to_string()), "missing {want}");
        }
    }

    #[test]
    fn cpu_args_offload_nothing() {
        let a = server_args(Path::new("g.gguf"), 1, false);
        let i = a.iter().position(|x| x == "-ngl").unwrap();
        assert_eq!(a[i + 1], "0");
    }

    #[tokio::test]
    async fn missing_binary_is_reported() {
        let e = LocalEngine::new(PathBuf::from(r"C:\nao\existe.exe"));
        let r = e
            .ensure_loaded(Path::new("x.gguf"), true, None, |_| {})
            .await;
        assert!(matches!(r, Err(EngineError::BinaryMissing)));
        assert!(!e.is_loaded());
    }

    #[tokio::test]
    async fn missing_model_is_reported() {
        let bin = std::env::current_exe().unwrap(); // any existing file
        let e = LocalEngine::new(bin);
        let r = e
            .ensure_loaded(Path::new(r"C:\nao\existe.gguf"), true, None, |_| {})
            .await;
        assert!(matches!(r, Err(EngineError::ModelMissing(_))));
    }

    /// Manual: HANDY_PLUS_TEST_GGUF=caminho\modelo.gguf cargo test -- --ignored local_engine
    #[tokio::test]
    #[ignore]
    async fn loads_real_model_and_recovers_after_kill() {
        let model =
            PathBuf::from(std::env::var("HANDY_PLUS_TEST_GGUF").expect("set HANDY_PLUS_TEST_GGUF"));
        let bin = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources/llama-server/llama-server.exe");
        let e = LocalEngine::new(bin);

        let ticks = std::sync::atomic::AtomicU32::new(0);
        let (url, load_ms) = e
            .ensure_loaded(&model, true, Some(30_000), |_| {
                ticks.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            })
            .await
            .expect("first load");
        println!(
            "loaded in {load_ms} ms at {url}, progress ticks: {:?}",
            ticks
        );
        let status = reqwest::get(format!("{url}/health"))
            .await
            .unwrap()
            .status();
        assert_eq!(status.as_u16(), 200);
        assert!(e.is_loaded());
        assert!(e.idle_for().is_some());

        // Reuse: same server, no reload.
        let (url2, ms2) = e.ensure_loaded(&model, true, None, |_| {}).await.unwrap();
        assert_eq!((url2.as_str(), ms2), (url.as_str(), 0));

        // Kill the child hard: the next check notices.
        e.kill_child_for_test();
        assert!(!e.is_loaded());

        let (url3, load_ms3) = e
            .ensure_loaded(&model, true, None, |_| {})
            .await
            .expect("reload");
        println!("reloaded in {load_ms3} ms at {url3}");
        assert_eq!(
            reqwest::get(format!("{url3}/health"))
                .await
                .unwrap()
                .status()
                .as_u16(),
            200
        );
        e.unload();
        assert!(!e.is_loaded());
    }

    /// Dropping `ensure_loaded` mid-load must kill the half-started server.
    #[cfg(windows)]
    #[tokio::test]
    async fn cancelled_load_kills_child_and_clears_state() {
        let ping = PathBuf::from(r"C:\Windows\System32\PING.EXE");
        let mut e = LocalEngine::new(ping);
        e.args_override = Some(vec!["-n".into(), "60".into(), "127.0.0.1".into()]);
        let model = std::env::current_exe().unwrap(); // any existing file
        let r = tokio::time::timeout(
            Duration::from_millis(800),
            e.ensure_loaded(&model, false, None, |_| {}),
        )
        .await;
        assert!(r.is_err(), "load should still be pending");
        let pid = e.last_pid.load(std::sync::atomic::Ordering::Relaxed);
        assert_ne!(pid, 0);
        assert!(!e.is_loaded());
        assert!(e.idle_for().is_none());
        assert!(e.lock().is_none(), "state must be cleared");
        let out = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .unwrap();
        assert!(
            !String::from_utf8_lossy(&out.stdout).contains(&pid.to_string()),
            "child {pid} still running"
        );
    }

    #[cfg(windows)]
    #[test]
    fn unload_unless_busy_respects_the_flag() {
        use std::sync::atomic::AtomicBool;
        let e = LocalEngine::new(PathBuf::from("unused"));
        let child = std::process::Command::new(r"C:\Windows\System32\PING.EXE")
            .args(["-n", "60", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        *e.lock() = Some(Running {
            child,
            port: 1,
            model_path: PathBuf::new(),
            gpu: false,
            ready: true,
            last_used: Instant::now(),
        });
        let busy = AtomicBool::new(true);
        assert!(!e.unload_unless_busy(&busy));
        assert!(e.is_loaded(), "busy session must keep the server");
        busy.store(false, std::sync::atomic::Ordering::SeqCst);
        assert!(e.unload_unless_busy(&busy));
        assert!(!e.is_loaded());
        assert!(!e.unload_unless_busy(&busy), "nothing left to unload");
    }

    #[cfg(windows)]
    #[test]
    fn serves_only_the_same_ready_model_on_the_same_device() {
        let e = LocalEngine::new(PathBuf::from("unused"));
        let model = PathBuf::from(r"D:\m\g.gguf");
        assert!(!e.serves(&model, true), "nothing loaded");
        let child = std::process::Command::new(r"C:\Windows\System32\PING.EXE")
            .args(["-n", "60", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        *e.lock() = Some(Running {
            child,
            port: 1,
            model_path: model.clone(),
            gpu: true,
            ready: false,
            last_used: Instant::now(),
        });
        assert!(!e.serves(&model, true), "still loading");
        e.lock().as_mut().unwrap().ready = true;
        assert!(e.serves(&model, true));
        assert!(!e.serves(&model, false), "other device");
        assert!(!e.serves(Path::new("outro.gguf"), true), "other model");
        e.kill_child_for_test();
        assert!(!e.serves(&model, true), "dead server");
        assert!(e.lock().is_none(), "dead server is cleared");
    }
}
