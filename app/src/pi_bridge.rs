//! One interactive pi process. PTY bytes are display-only; the extension owns app integration.
use crate::{pi_control::Control, state::{AppState, PiStatus}, terminal::{Frame, Terminal}};
use portable_pty::CommandBuilder;
use serde_json::json;
use std::{path::{Path, PathBuf}, sync::{atomic::{AtomicU64, Ordering}, Arc, Mutex}};

pub struct PiBridge {
    session: Mutex<Option<(Arc<Terminal>, Arc<Control>)>>,
    generation: AtomicU64,
    lifecycle: Mutex<()>,
}
impl PiBridge {
    pub fn new() -> Self { Self { session: Mutex::new(None), generation: AtomicU64::new(0), lifecycle: Mutex::new(()) } }
    pub fn ready(&self) -> bool { self.session.lock().unwrap().as_ref().is_some_and(|(_, c)| c.ready()) }
    /// Capture at intent creation, before building context or doing import work.
    pub fn ticket(&self) -> u64 { self.generation.load(Ordering::SeqCst) }
    pub fn prompt(&self, generation: u64, st: &AppState, message: String, _display: Option<String>) -> Result<(), String> {
        let result = (|| {
            if !self.ready() { return Err("pi integration isn't ready — wait or Restart".into()); }
            let c = self.session.lock().unwrap().as_ref().filter(|(t, _)| t.generation == generation).map(|(_, c)| c.clone()).ok_or("pi session/project changed; request cancelled")?;
            c.request("prompt", json!({"message":message})).map(|_| ())
        })();
        if result.is_err() {
            let stuck = {
                let mut s = st.lock();
                if self.ticket() == generation { s.store.release_stuck_evaluations() } else { 0 }
            };
            if stuck > 0 { st.changed(); }
        }
        result
    }
    pub fn abort(&self) -> Result<(), String> {
        let c = self.session.lock().unwrap().as_ref().map(|(_, c)| c.clone()).ok_or("pi offline")?;
        c.request("abort", json!({})).map(|_| ())
    }
    pub fn terminal(&self, generation: Option<u64>) -> Result<Arc<Terminal>, String> {
        let s = self.session.lock().unwrap();
        let (t, _) = s.as_ref().ok_or("pi is offline — use Restart")?;
        if generation.is_some_and(|g| g != t.generation) { return Err("stale terminal generation".into()); }
        Ok(t.clone())
    }
    pub fn read(&self, generation: Option<u64>, after: u64) -> Result<Frame, String> { self.terminal(generation)?.read(after) }
    pub fn context(&self, tab: String) {
        if let Some((_, c)) = self.session.lock().unwrap().as_ref() { *c.tab.lock().unwrap() = tab; }
    }
    pub fn shutdown(&self) {
        let _guard = self.lifecycle.lock().unwrap();
        self.stop();
    }
    fn stop(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        let session = self.session.lock().unwrap().take();
        if let Some((t, c)) = session { c.stop(); t.stop(); }
    }
    pub fn restart(&self, st: &AppState) -> Result<(), String> {
        let _guard = self.lifecycle.lock().unwrap();
        self.stop();
        let released = st.lock().store.release_stuck_evaluations();
        if released > 0 { st.changed(); }
        self.start_locked(st.clone())
    }
    pub fn start(&self, st: &AppState) -> Result<(), String> {
        let _guard = self.lifecycle.lock().unwrap();
        self.start_locked(st.clone())
    }
    fn start_locked(&self, st: AppState) -> Result<(), String> {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        { let mut s = st.lock(); s.pi_status = PiStatus::Starting; s.pi_error = None; s.pi_generation = generation; }
        st.emit();
        let result = self.spawn(&st, generation);
        match result {
            Ok(s) => { *self.session.lock().unwrap() = Some(s); Ok(()) }
            Err(e) => { { let mut s = st.lock(); s.pi_status = PiStatus::Offline; s.pi_error = Some(e.clone()); } st.toast("error", &format!("Couldn't start pi: {e}")); st.emit(); Err(e) }
        }
    }
    fn spawn(&self, st: &AppState, generation: u64) -> Result<(Arc<Terminal>, Arc<Control>), String> {
        let (project, port) = { let s = st.lock(); (s.project_dir.clone(), s.mcp_port) };
        let sessions = project.join(".spide/sessions");
        std::fs::create_dir_all(&sessions).map_err(|e| e.to_string())?;
        write_mcp_config(&project, port).map_err(|e| e.to_string())?;
        let ext = project.join(".spide/terminal-bridge.ts");
        std::fs::write(&ext, include_str!("../../agent/spide-terminal.ts")).map_err(|e| e.to_string())?;
        let control = Control::start(st.clone(), generation)?;
        let mut cmd = CommandBuilder::new(pi_binary());
        cmd.cwd(&project);
        cmd.env("PATH", child_path());
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("SPIDE_CONTROL_SOCKET", &control.path);
        cmd.env("SPIDE_CONTROL_TOKEN", &control.token);
        cmd.env("SPIDE_CONTROL_GENERATION", generation.to_string());
        cmd.args(["--session-dir", &sessions.display().to_string(), "--continue", "--append-system-prompt", include_str!("../../agent/spide-skill.md"), "--tools", "*", "-e", &ext.display().to_string()]);
        match Terminal::spawn(cmd, generation) {
            Ok(t) => Ok((t, control)),
            Err(e) => { control.stop(); Err(e) }
        }
    }
}
fn write_mcp_config(project: &Path, port: u16) -> std::io::Result<()> {
    let path = project.join(".pi/mcp.json");
    std::fs::create_dir_all(path.parent().unwrap())?;
    let mut cfg: serde_json::Value = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(json!({}));
    cfg["mcpServers"]["spide"] = json!({"url":format!("http://127.0.0.1:{port}/mcp"),"transport":"streamable-http","lifecycle":"keep-alive","directTools":true});
    std::fs::write(path, serde_json::to_vec_pretty(&cfg)?)
}
fn pi_binary() -> PathBuf {
    if let Ok(p) = std::env::var("SPIDE_PI") { return PathBuf::from(p); }
    if let Some(p) = which("pi") { return p; }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        for relative in [".pi/agent/bin/pi", ".local/bin/pi"] {
            let p = home.join(relative);
            if p.is_file() { return p; }
        }
        let nvm = home.join(".nvm/versions/node");
        if let Ok(entries) = std::fs::read_dir(nvm) {
            let mut paths: Vec<_> = entries.filter_map(Result::ok).map(|e| e.path().join("bin/pi")).filter(|p| p.exists()).collect();
            paths.sort();
            if let Some(p) = paths.pop() { return p; }
        }
    }
    PathBuf::from("pi")
}
fn which(bin: &str) -> Option<PathBuf> { std::env::split_paths(&std::env::var_os("PATH")?).map(|p| p.join(bin)).find(|p| p.is_file()) }
fn child_path() -> String {
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
    if let Some(bin) = pi_binary().parent() { if !bin.as_os_str().is_empty() { dirs.insert(0, bin.to_owned()); } }
    // Finder does not source .zshrc (where nvm usually lives). The managed pi launcher
    // still needs a Node interpreter even when no nvm version has a `bin/pi` symlink.
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        let managed = home.join(".local/share/pi-node/current/bin");
        if managed.join("node").is_file() { dirs.push(managed); }
        if let Ok(entries) = std::fs::read_dir(home.join(".nvm/versions/node")) {
            let mut bins: Vec<_> = entries.filter_map(Result::ok).map(|e| e.path().join("bin")).filter(|p| p.join("node").is_file()).collect();
            bins.sort(); bins.reverse(); dirs.extend(bins);
        }
    }
    for p in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"] { dirs.push(PathBuf::from(p)); }
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    if let Ok(o) = std::process::Command::new(shell).args(["-lc", "printf '%s' \"$PATH\""]).output() {
        if o.status.success() { dirs.extend(std::env::split_paths(&String::from_utf8_lossy(&o.stdout).trim())); }
    }
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|p| seen.insert(p.clone()));
    std::env::join_paths(dirs).unwrap_or_default().to_string_lossy().into_owned()
}
