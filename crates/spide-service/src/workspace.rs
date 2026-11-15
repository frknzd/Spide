//! A workspace: one project folder, its design store and the single-writer lock.
//!
//! Exactly one process (desktop or headless) owns a workspace for writing. The lock is
//! `.spide/lock` containing the owner's pid and a session token; a lock whose pid is gone is stale
//! and is taken over. Other processes must attach to the owner (see `discovery`) instead of writing.

use spide_core::{Design, Store};
use std::path::{Path, PathBuf};

pub const LOCK: &str = ".spide/lock";

#[derive(Debug)]
pub struct WorkspaceLock {
    path: PathBuf,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LockConflict {
    pub pid: u32,
    pub path: PathBuf,
}

impl std::fmt::Display for LockConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "workspace is owned by process {} ({}); attach to it instead of opening a second writer", self.pid, self.path.display())
    }
}

fn alive(pid: u32) -> bool {
    if pid == std::process::id() {
        return true;
    }
    // signal 0: existence check only
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM) }
}

/// Random-enough hex token (no crypto dependency): pid, time and address entropy through FNV.
pub fn token() -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let local = 0u8;
    let seed = format!("{}:{t}:{:p}:{:?}", std::process::id(), &local, std::thread::current().id());
    let mut extra = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = f.read_exact(&mut extra);
    }
    let hex: String = extra.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}{hex}", spide_core::assess::fnv(&seed))
}

impl WorkspaceLock {
    pub fn acquire(root: &Path) -> Result<WorkspaceLock, LockConflict> {
        let path = root.join(LOCK);
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        for _ in 0..2 {
            let token = token();
            let body = format!("{}\n{token}\n", std::process::id());
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut f) => {
                    use std::io::Write;
                    let _ = f.write_all(body.as_bytes());
                    restrict(&path);
                    return Ok(WorkspaceLock { path, token });
                }
                Err(_) => {
                    let text = std::fs::read_to_string(&path).unwrap_or_default();
                    let pid: u32 = text.lines().next().and_then(|l| l.trim().parse().ok()).unwrap_or(0);
                    if pid != 0 && alive(pid) {
                        return Err(LockConflict { pid, path });
                    }
                    let _ = std::fs::remove_file(&path); // stale
                }
            }
        }
        Err(LockConflict { pid: 0, path })
    }
}

impl Drop for WorkspaceLock {
    fn drop(&mut self) {
        let mine = std::fs::read_to_string(&self.path).is_ok_and(|t| t.contains(&self.token));
        if mine {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Owner-only permissions for local secrets/metadata.
pub fn restrict(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

pub fn write_atomic(path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".{}.tmp", std::process::id()));
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)
}

/// `spide/design.json` in a Spide Project, `design.json` otherwise.
pub fn design_path(root: &Path) -> PathBuf {
    if spide_core::project::is_project(root) {
        root.join(spide_core::project::DESIGN)
    } else {
        root.join("design.json")
    }
}

pub struct Workspace {
    pub root: PathBuf,
    pub store: Store,
    pub last_written: Option<String>,
    pub lock: Option<WorkspaceLock>,
    pub last_history: Option<String>,
}

impl Workspace {
    /// Open for writing (takes the lock).
    pub fn open(dir: &Path) -> Result<Workspace, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let root = spide_core::project::resolve_root(dir);
        let lock = WorkspaceLock::acquire(&root).map_err(|e| e.to_string())?;
        let mut w = Self::load(&root)?;
        w.lock = Some(lock);
        Ok(w)
    }

    /// Load without the lock (read-only inspection, tests).
    pub fn load(root: &Path) -> Result<Workspace, String> {
        let path = design_path(root);
        let mut store = Store::default();
        let mut last_written = None;
        match std::fs::read_to_string(&path) {
            Ok(s) => {
                let d = Design::from_json(&s).map_err(|e| format!("{}: {e}", path.display()))?;
                store.load(d);
                store.release_stuck_evaluations();
                last_written = Some(s);
            }
            Err(_) => {
                if let Some(n) = root.file_name().and_then(|n| n.to_str()) {
                    store.design.name = n.to_string();
                }
            }
        }
        Ok(Workspace { root: root.to_path_buf(), store, last_written, lock: None, last_history: None })
    }

    pub fn design_path(&self) -> PathBuf {
        design_path(&self.root)
    }

    /// Persist if changed. Refuses without the lock.
    pub fn save(&mut self) -> Result<bool, String> {
        if self.lock.is_none() {
            return Err("workspace opened read-only".into());
        }
        let json = self.store.design.to_json();
        if self.last_written.as_deref() == Some(json.as_str()) {
            return Ok(false);
        }
        write_atomic(&self.design_path(), &json).map_err(|e| e.to_string())?;
        self.last_written = Some(json);
        let root = self.root.clone();
        if let Err(e) = crate::history::append(&root, &self.store.design, &mut self.last_history) {
            eprintln!("spide: history: {e}");
        }
        Ok(true)
    }

    /// Give up the writer role (another process takes over): save, release the lock.
    pub fn release(&mut self) -> Result<(), String> {
        if self.lock.is_some() {
            self.save()?;
        }
        self.lock = None;
        Ok(())
    }

    /// Take the writer role back (the other process is gone) and reload what it saved.
    pub fn reacquire(&mut self) -> Result<(), String> {
        if self.lock.is_none() {
            self.lock = Some(WorkspaceLock::acquire(&self.root).map_err(|e| e.to_string())?);
        }
        self.reload_if_external();
        Ok(())
    }

    /// Re-read the design if another tool changed the file (only when we hold no unsaved changes).
    pub fn reload_if_external(&mut self) -> bool {
        let Ok(text) = std::fs::read_to_string(self.design_path()) else { return false };
        if self.last_written.as_deref() == Some(text.as_str()) {
            return false;
        }
        match Design::from_json(&text) {
            Ok(d) => {
                self.store.load(d);
                self.last_written = Some(text);
                true
            }
            Err(_) => false,
        }
    }
}
