//! Watches `<project>/design.json`. External edits become proposals (never applied silently).

use crate::state::AppState;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use spide_core::{diff::diff, Design};
use std::path::Path;
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::Mutex;
use std::time::Duration;

static WATCHER: Mutex<Option<RecommendedWatcher>> = Mutex::new(None);

pub fn watch(st: &AppState, dir: &Path) {
    let (tx, rx) = channel::<()>();
    let target = dir.join("design.json");
    let target2 = target.clone();
    let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res {
            if ev.paths.iter().any(|p| p == &target2 || p.file_name() == target2.file_name()) {
                let _ = tx.send(());
            }
        }
    });
    let mut watcher = match watcher {
        Ok(w) => w,
        Err(e) => {
            eprintln!("spide: watcher: {e}");
            return;
        }
    };
    if let Err(e) = watcher.watch(dir, RecursiveMode::NonRecursive) {
        eprintln!("spide: watch {}: {e}", dir.display());
        return;
    }
    *WATCHER.lock().unwrap() = Some(watcher); // drops the previous watcher → its thread ends

    let st = st.clone();
    std::thread::spawn(move || loop {
        // wait for an event, then debounce
        if rx.recv().is_err() {
            return;
        }
        loop {
            match rx.recv_timeout(Duration::from_millis(300)) {
                Ok(()) => continue,
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        on_change(&st, &target);
    });
}

fn on_change(st: &AppState, path: &Path) {
    let Ok(text) = std::fs::read_to_string(path) else { return };
    let ops = {
        let mut s = st.lock();
        if s.project_dir.join("design.json") != path || s.last_written.as_deref() == Some(text.as_str()) {
            return;
        }
        let ext = match Design::from_json(&text) {
            Ok(d) => d,
            Err(e) => {
                drop(s);
                st.toast("error", format!("design.json was edited but is invalid: {e}"));
                return;
            }
        };
        let ops = diff(&s.store.design, &ext);
        // remember this content so we don't re-propose it
        s.last_written = Some(text);
        // rules are user-owned configuration: an edit in the file is applied directly
        let rules_changed = ext.rules != s.store.design.rules;
        if rules_changed {
            let _ = s.store.set_rules(ext.rules.clone());
        }
        if ops.is_empty() {
            if rules_changed {
                drop(s);
                st.toast("info", "architecture rules updated from design.json");
                st.changed();
            }
            return;
        }
        match s.store.propose_ops(ops.clone(), Some("edited design.json".into()), "file") {
            Ok(_) => ops,
            Err(e) => {
                drop(s);
                st.toast("error", format!("design.json edit could not be applied: {}", e.0));
                return;
            }
        }
    };
    st.toast("info", format!("design.json changed outside Spide → {} proposed change(s)", ops.len()));
    st.changed();
}
