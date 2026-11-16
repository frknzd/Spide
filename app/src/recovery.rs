//! Last-known-good copies of the design (`.spide/recovery/`), taken before the design file is
//! replaced. Bounded, and never used to overwrite anything: recovery always produces a new copy.

use spide_core::Design;
use std::path::{Path, PathBuf};

/// Copies kept per workspace.
pub const KEEP: usize = 10;
/// At most one copy per this interval (ms): enough to step back past a bad session.
pub const INTERVAL_MS: u64 = 10 * 60 * 1000;

pub fn dir(project: &Path) -> PathBuf {
    project.join(".spide").join("recovery")
}

fn stamp(p: &Path) -> Option<u64> {
    p.file_stem()?.to_str()?.strip_prefix("design-")?.parse().ok()
}

fn copies(project: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir(project)).map(|r| r.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| stamp(p).is_some()).collect()).unwrap_or_default();
    v.sort_by_key(|p| std::cmp::Reverse(stamp(p)));
    v
}

/// Before `design` is replaced: keep its current content if it is a valid design and the newest
/// copy is old enough. Invalid content is never kept (it is not "known good").
pub fn keep_previous(project: &Path, design: &Path, now: u64) -> std::io::Result<()> {
    let Ok(text) = std::fs::read_to_string(design) else { return Ok(()) };
    if Design::from_json(&text).is_err() {
        return Ok(());
    }
    let existing = copies(project);
    if existing.first().and_then(|p| stamp(p)).is_some_and(|t| now.saturating_sub(t) < INTERVAL_MS) {
        return Ok(());
    }
    crate::state::write_atomic(&dir(project).join(format!("design-{now}.json")), &text)?;
    for old in copies(project).into_iter().skip(KEEP) {
        std::fs::remove_file(old)?;
    }
    Ok(())
}

/// Recoverable designs, most recent revision first. On equal revisions a full recovery copy wins
/// over the history snapshot (which lacks layout and pending plans).
pub fn candidates(project: &Path) -> Vec<(String, Design)> {
    let mut out: Vec<(u64, u8, String, Design)> = copies(project)
        .into_iter()
        .filter_map(|p| {
            let d = Design::from_json(&std::fs::read_to_string(&p).ok()?).ok()?;
            Some((d.rev, 1, format!("recovery copy {} (revision {})", crate::ui::format_time(stamp(&p)?), d.rev), d))
        })
        .collect();
    if let Some(e) = crate::history::list(project).last() {
        if let Some(mut d) = crate::history::at(project, e.rev) {
            d.rev = e.rev;
            out.push((e.rev, 0, format!("history revision {} (layout and pending plans not included)", e.rev), d));
        }
    }
    out.sort_by(|a, b| (b.0, b.1).cmp(&(a.0, a.1)));
    out.into_iter().map(|(_, _, l, d)| (l, d)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_bounded_valid_copies_and_skips_invalid_content() {
        let dir = tempfile::tempdir().unwrap();
        let design = dir.path().join("design.json");
        std::fs::write(&design, "corrupt").unwrap();
        keep_previous(dir.path(), &design, 1).unwrap();
        assert!(copies(dir.path()).is_empty(), "corrupt content is not known-good");
        std::fs::write(&design, Design { name: "Good".into(), ..Default::default() }.to_json()).unwrap();
        keep_previous(dir.path(), &design, INTERVAL_MS).unwrap();
        keep_previous(dir.path(), &design, INTERVAL_MS + 1).unwrap();
        assert_eq!(copies(dir.path()).len(), 1, "one copy per interval");
        for i in 2..30 {
            keep_previous(dir.path(), &design, i * INTERVAL_MS).unwrap();
        }
        assert_eq!(copies(dir.path()).len(), KEEP);
        assert_eq!(candidates(dir.path())[0].1.name, "Good");
    }
}
