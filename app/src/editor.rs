//! Neovim as Spide's editor, desktop side.
//!
//! * The bundled plugin (`integrations/nvim`) is installed into Spide's data dir at startup, so
//!   both the embedded Neovim and (via the LazyVim spec) the user's own Neovim load the same code.
//! * The embedded Neovim runs the user's config in its own PTY (rendered by the terminal widget,
//!   never parsed for state); it registers with the service like an external session.
//! * Clicks on the diagram open source through the service (`open_source`): the user's external
//!   Neovim is preferred; without one, the embedded editor is started and used.

use crate::events::UiEvent;
use crate::state::AppState;
use crate::terminal::{Frame, Terminal};
use portable_pty::CommandBuilder;
use serde_json::{json, Value};
use spide_service::Service;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const PLUGIN_INIT: &str = include_str!("../../integrations/nvim/lua/spide/init.lua");
const PLUGIN_CMDS: &str = include_str!("../../integrations/nvim/plugin/spide.lua");

/// `<data>/spide/nvim`: the plugin's runtime directory (written once per start, owner-only).
pub fn plugin_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("spide").join("nvim")
}

pub fn install_plugin() -> Result<PathBuf, String> {
    let d = plugin_dir();
    for (rel, text) in [("lua/spide/init.lua", PLUGIN_INIT), ("plugin/spide.lua", PLUGIN_CMDS)] {
        let p = d.join(rel);
        if std::fs::read_to_string(&p).ok().as_deref() != Some(text) {
            spide_service::workspace::write_atomic(&p, text).map_err(|e| format!("{}: {e}", p.display()))?;
        }
    }
    Ok(d)
}

fn nvim_binary() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("SPIDE_NVIM") {
        return Some(PathBuf::from(p));
    }
    spide_lsp::client::which("nvim").or_else(|| ["/opt/homebrew/bin/nvim", "/usr/local/bin/nvim", "/usr/bin/nvim"].iter().map(PathBuf::from).find(|p| p.is_file()))
}

/// The embedded Neovim: one PTY, restartable; generation bumps on every start.
#[derive(Default)]
pub struct EmbeddedNvim {
    term: Mutex<Option<Arc<Terminal>>>,
    generation: AtomicU64,
    root: Mutex<Option<PathBuf>>,
}

static EMBEDDED: OnceLock<Arc<EmbeddedNvim>> = OnceLock::new();

pub fn embedded() -> Arc<EmbeddedNvim> {
    EMBEDDED.get_or_init(|| Arc::new(EmbeddedNvim::default())).clone()
}

impl EmbeddedNvim {
    pub fn running(&self) -> bool {
        self.term.lock().unwrap().as_ref().is_some_and(|t| !t.exited())
    }

    /// Start Neovim (user config + Spide's plugin) in `root`, unless it already runs there.
    pub fn start(&self, root: &Path) -> Result<(), String> {
        if self.running() && self.root.lock().unwrap().as_deref() == Some(root) {
            return Ok(());
        }
        self.stop();
        let exe = nvim_binary().ok_or("Neovim (nvim) not found: install it or set SPIDE_NVIM")?;
        let plugin = install_plugin()?;
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let mut cmd = CommandBuilder::new(exe);
        cmd.cwd(root);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("SPIDE_EMBEDDED", "1");
        if let Some(path) = std::env::var_os("PATH") {
            cmd.env("PATH", path);
        }
        // prepend the plugin; setup() is idempotent if the user's config already loads it
        cmd.args(["--cmd", &format!("set rtp^={}", plugin.display()), "-c", "lua if not package.loaded['spide'] or not require('spide').state.session then require('spide').setup(require('spide').config) require('spide').connect({quiet=true}) end"]);
        let t = Terminal::spawn(cmd, generation)?;
        *self.term.lock().unwrap() = Some(t);
        *self.root.lock().unwrap() = Some(root.to_path_buf());
        Ok(())
    }

    pub fn stop(&self) {
        if let Some(t) = self.term.lock().unwrap().take() {
            t.stop();
        }
    }
}

impl crate::ui::terminal::PtySource for EmbeddedNvim {
    fn read(&self, generation: Option<u64>, after: u64) -> Result<Frame, String> {
        let t = self.terminal(generation)?;
        t.read(after)
    }
    fn terminal(&self, generation: Option<u64>) -> Result<Arc<Terminal>, String> {
        let g = self.term.lock().unwrap();
        let t = g.as_ref().ok_or("Neovim isn't running — use Neovim ▸ to start it")?;
        if generation.is_some_and(|x| x != t.generation) {
            return Err("stale editor generation".into());
        }
        Ok(t.clone())
    }
    fn exit_message(&self) -> String {
        "Neovim exited. Use Neovim ▸ again to start it.".into()
    }
}

pub fn on_reveal(st: &AppState, symbol: &str, _origin: &str) {
    st.events.send(UiEvent::Reveal { symbol: symbol.to_string() });
}

pub fn install_extension(_st: &AppState, _svc: &Arc<Service>) {}

/// Editor sessions connect to us through the MCP endpoint; no separate socket.
pub fn socket_path(_st: &AppState) -> Option<PathBuf> {
    None
}

/// Open a design element's source in Neovim (external preferred; embedded started on demand).
/// Blocking: call off the UI thread. Returns where it opened.
pub fn open_element(st: &AppState, class: &str, method: Option<&str>) -> Result<Value, String> {
    let svc = crate::service::get(st).ok_or("service not ready")?;
    let mut args = json!({"class": class});
    if let Some(m) = method {
        args["method"] = json!(m);
    }
    // Reuse the existing session-preference mechanism; no second editor bridge.
    if st.lock().preferences.embedded_editor {
        let root = st.lock().project_dir.clone();
        embedded().start(&root)?;
        st.events.send(UiEvent::ShowEditor);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(session) = svc.editors.list().into_iter().find(|s| s.kind == "embedded") {
                svc.editors.prefer(Some(session.id))?;
                break;
            }
            if Instant::now() >= deadline { return Err("Embedded Neovim did not connect. Check its pane or choose external Neovim in Settings.".into()); }
            std::thread::sleep(Duration::from_millis(150));
        }
    } else {
        svc.editors.prefer(None)?; // default: most recently active external session
    }
    match svc.call_tool("open_source", &args, "user") {
        Ok(t) => Ok(serde_json::from_str(&t).unwrap_or(Value::Null)),
        Err(e) if e.contains("no Neovim attached") => {
            let root = st.lock().project_dir.clone();
            embedded().start(&root)?;
            st.events.send(UiEvent::ShowEditor);
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                match svc.call_tool("open_source", &args, "user") {
                    Ok(t) => return Ok(serde_json::from_str(&t).unwrap_or(Value::Null)),
                    Err(e) if e.contains("no Neovim attached") && Instant::now() < deadline => std::thread::sleep(Duration::from_millis(150)),
                    Err(e) => return Err(e),
                }
            }
        }
        Err(e) => Err(e),
    }
}

/// Dirty buffers in the embedded Neovim (empty when it isn't running).
pub fn embedded_dirty(st: &AppState) -> Vec<String> {
    match embedded_status(st) {
        EditorStatus::Dirty(d) => d,
        _ => vec![],
    }
}

/// Whether the embedded Neovim may be torn down. A running editor whose buffer
/// state cannot be queried is `Unknown`, which callers must treat as unsafe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorStatus {
    NotRunning,
    Clean,
    Dirty(Vec<String>),
    Unknown(String),
}

pub fn embedded_status(st: &AppState) -> EditorStatus {
    if !embedded().running() {
        return EditorStatus::NotRunning;
    }
    let Some(svc) = crate::service::get(st) else { return EditorStatus::Unknown("the editor service is unavailable".into()) };
    let sessions: Vec<_> = svc.editors.list().into_iter().filter(|s| s.kind == "embedded").collect();
    if sessions.is_empty() {
        return EditorStatus::Unknown("Neovim has not connected to Spide".into());
    }
    let mut dirty = vec![];
    for s in sessions {
        match svc.editors.dirty(&s.id) {
            Ok(d) => dirty.extend(d),
            Err(e) => return EditorStatus::Unknown(e),
        }
    }
    if dirty.is_empty() { EditorStatus::Clean } else { EditorStatus::Dirty(dirty) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::terminal::PtySource;

    #[test]
    fn embedded_neovim_runs_in_a_pty_with_the_plugin() {
        if nvim_binary().is_none() {
            return;
        }
        let cfg = tempfile::tempdir().unwrap();
        // isolated config: the user's Neovim setup is not touched by tests
        std::env::set_var("XDG_CONFIG_HOME", cfg.path());
        let root = tempfile::tempdir().unwrap();
        let e = EmbeddedNvim::default();
        e.start(root.path()).unwrap();
        assert!(e.running());
        let mut after = 0;
        let mut seen = vec![];
        for _ in 0..100 {
            let f = e.read(None, after).unwrap();
            after = f.next;
            seen.extend(f.bytes);
            if seen.len() > 200 {
                break;
            }
        }
        assert!(!seen.is_empty(), "Neovim drew its UI");
        // the plugin loaded: :SpideConnect exists
        e.terminal(None).unwrap().write(b":lua io.stderr:write(vim.fn.exists(':SpideConnect'))\r").unwrap();
        e.stop();
        assert!(!e.running());
        assert!(plugin_dir().join("lua/spide/init.lua").is_file());
    }
}
