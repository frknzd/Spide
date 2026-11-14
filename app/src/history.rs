//! Persistent design history: one model snapshot per revision in `.spide/history.jsonl`
//! (append-only, deduplicated, capped). Powers the timeline, restore and fork.

use serde::{Deserialize, Serialize};
use spide_core::Design;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

pub const CAP: usize = 5000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub rev: u64,
    pub at: u64,
    pub actor: String,
    pub summary: String,
}

#[derive(Serialize, Deserialize)]
struct Line {
    #[serde(flatten)]
    entry: Entry,
    model: Design,
}

pub fn path(project: &Path) -> PathBuf {
    project.join(".spide").join("history.jsonl")
}

/// The part of a design that the timeline shows (no log, feedback, counters or settings).
pub fn model_of(d: &Design) -> Design {
    let mut m = d.clone();
    m.log.clear();
    m.feedback.clear();
    m.proposals.clear();
    m.rev = 0;
    m.model_rev = 0;
    m.counter = 0;
    m.settings = Default::default();
    // keep only the class layout (replays look like the canvas)
    m.layout.retain(|k, _| k == "class");
    m
}

/// Fingerprint used for dedup: the model without layout.
fn fingerprint(model: &Design) -> String {
    let mut m = model.clone();
    m.layout.clear();
    serde_json::to_string(&m).unwrap_or_default()
}

/// Append a snapshot unless the model is unchanged. `last` caches the previous fingerprint.
pub fn append(project: &Path, d: &Design, last: &mut Option<String>) -> std::io::Result<()> {
    let model = model_of(d);
    let fp = fingerprint(&model);
    if last.is_none() {
        *last = read_all(project).last().map(|l| fingerprint(&l.model));
    }
    if last.as_deref() == Some(fp.as_str()) {
        return Ok(());
    }
    let le = d.log.last();
    let line = Line {
        entry: Entry {
            rev: d.rev,
            at: le.map_or(0, |l| l.at),
            actor: le.map(|l| l.actor.clone()).unwrap_or_default(),
            summary: le.map(|l| l.summary.clone()).unwrap_or_else(|| "opened".into()),
        },
        model,
    };
    let p = path(project);
    std::fs::create_dir_all(p.parent().unwrap())?;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&p)?;
    writeln!(f, "{}", serde_json::to_string(&line)?)?;
    *last = Some(fp);
    rotate_if_needed(project)
}

fn rotate_if_needed(project: &Path) -> std::io::Result<()> {
    let p = path(project);
    let n = BufReader::new(std::fs::File::open(&p)?).lines().count();
    if n <= CAP {
        return Ok(());
    }
    let keep: Vec<String> = BufReader::new(std::fs::File::open(&p)?).lines().map_while(Result::ok).skip(n - CAP * 4 / 5).collect();
    crate::state::write_atomic(&p, &(keep.join("\n") + "\n"))
}

fn read_all(project: &Path) -> Vec<Line> {
    let Ok(f) = std::fs::File::open(path(project)) else { return vec![] };
    BufReader::new(f).lines().map_while(Result::ok).filter_map(|l| serde_json::from_str(&l).ok()).collect()
}

/// Timeline entries (oldest first), without the models.
pub fn list(project: &Path) -> Vec<Entry> {
    read_all(project).into_iter().map(|l| l.entry).collect()
}

/// The design as it was at `rev` (the latest snapshot at or before it).
pub fn at(project: &Path, rev: u64) -> Option<Design> {
    read_all(project).into_iter().filter(|l| l.entry.rev <= rev).last().map(|l| l.model)
}

#[cfg(test)]
mod tests {
    use super::*;
    use spide_core::{Op, Store};

    #[test]
    fn appends_dedups_and_reads_back() {
        let dir = std::env::temp_dir().join(format!("spide-hist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = Store::default();
        let mut last = None;
        append(&dir, &s.design, &mut last).unwrap();
        let ops: Vec<Op> = serde_json::from_str(r#"[{"op":"add_class","name":"A"}]"#).unwrap();
        s.apply_user(ops).unwrap();
        append(&dir, &s.design, &mut last).unwrap();
        // layout-only change → no new entry
        s.set_layout("class", [("A".to_string(), spide_core::model::Pos { x: 1.0, y: 2.0, pinned: false })].into());
        append(&dir, &s.design, &mut last).unwrap();
        let ops: Vec<Op> = serde_json::from_str(r#"[{"op":"add_class","name":"B"}]"#).unwrap();
        s.apply_user(ops).unwrap();
        let mut fresh = None; // a new session re-reads the last fingerprint
        append(&dir, &s.design, &mut fresh).unwrap();
        append(&dir, &s.design, &mut fresh).unwrap();
        let l = list(&dir);
        assert_eq!(l.len(), 3, "{l:?}");
        assert_eq!(l[1].actor, "user");
        let at1 = at(&dir, l[1].rev).unwrap();
        assert_eq!(at1.classes.len(), 1);
        assert_eq!(at(&dir, l[2].rev).unwrap().classes.len(), 2);
        assert!(at(&dir, 0).is_some_and(|d| d.classes.is_empty()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
