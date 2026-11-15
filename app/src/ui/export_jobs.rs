//! Batch export: every diagram of the project × detail levels × settings profiles × formats.
//! Pure job planning (names, counts) plus safe file writing; scene building lives in the app.

use super::export::Format;
use serde::{Deserialize, Serialize};
use spide_diagram::svg::ExportOptions;
use spide_diagram::visuals::DETAIL_LABELS;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// One diagram of the project.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExportView {
    SystemContext,
    SystemContainers,
    Class,
    Domain,
    Package,
    Usecase,
    Sequence(String),
}

impl ExportView {
    /// Stable identifier (settings, file names).
    pub fn key(&self) -> String {
        match self {
            ExportView::SystemContext => "system-context".into(),
            ExportView::SystemContainers => "system-containers".into(),
            ExportView::Class => "class".into(),
            ExportView::Domain => "domain".into(),
            ExportView::Package => "package".into(),
            ExportView::Usecase => "usecase".into(),
            ExportView::Sequence(n) => format!("sequence-{}", slug(n)),
        }
    }
    pub fn title(&self) -> String {
        match self {
            ExportView::SystemContext => "System context (C4)".into(),
            ExportView::SystemContainers => "Containers (C4)".into(),
            ExportView::Class => "Class diagram".into(),
            ExportView::Domain => "Domain model".into(),
            ExportView::Package => "Package diagram".into(),
            ExportView::Usecase => "Use case diagram".into(),
            ExportView::Sequence(n) => format!("Sequence: {n}"),
        }
    }
    /// Class and Domain draw at a detail level (packages … full); the other views have none.
    pub fn has_detail(&self) -> bool {
        matches!(self, ExportView::Class | ExportView::Domain)
    }
    /// The PlantUML exporter's view (one file per view; all sequences / both C4 levels together).
    pub fn plantuml(&self) -> &'static str {
        match self {
            ExportView::SystemContext | ExportView::SystemContainers => "system",
            ExportView::Class => "class",
            ExportView::Domain => "domain",
            ExportView::Package => "package",
            ExportView::Usecase => "usecase",
            ExportView::Sequence(_) => "sequence",
        }
    }
}

/// A named set of image settings (theme, content toggles, margins, PNG scale).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    pub options: ExportOptions,
    pub scale: u8,
}

impl Default for Profile {
    fn default() -> Self {
        Self { name: "current".into(), options: ExportOptions::default(), scale: 2 }
    }
}

impl Profile {
    pub fn summary(&self) -> String {
        let o = &self.options;
        let mut parts = vec![if o.theme == "dark" { "dark" } else { "light" }.to_string(), format!("PNG {}×", self.scale)];
        if o.diagnostics {
            parts.push("diagnostics".into());
        }
        if o.ghosts {
            parts.push("ghosts".into());
        }
        if !o.legend {
            parts.push("no legend".into());
        }
        if o.background == "transparent" {
            parts.push("transparent".into());
        }
        parts.join(" · ")
    }
}

/// Batch selections (persisted per project, separate from the single export's settings).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BatchSettings {
    /// views left out (by key); new views are included by default
    pub excluded: Vec<String>,
    /// detail levels for Class and Domain
    pub levels: Vec<u8>,
    /// image formats (PNG, SVG, PDF)
    pub formats: Vec<Format>,
    /// also one PlantUML file per view (its own layout, full model)
    pub plantuml: bool,
    /// settings profiles; empty: the current export settings
    pub profiles: Vec<Profile>,
}

impl Default for BatchSettings {
    fn default() -> Self {
        Self { excluded: vec![], levels: vec![1, 2, 3, 4], formats: vec![Format::Png], plantuml: false, profiles: vec![] }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub view: ExportView,
    /// detail level (Class/Domain image exports only)
    pub level: Option<u8>,
    /// index into the effective profiles (image exports only)
    pub profile: usize,
    pub format: Format,
    /// file name inside the output folder
    pub file: String,
}

pub fn slug(s: &str) -> String {
    let s: String = s.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let s = s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    if s.is_empty() {
        "diagram".into()
    } else {
        s
    }
}

fn unique(name: String, ext: &str, used: &mut HashSet<String>) -> String {
    let mut file = format!("{name}.{ext}");
    let mut n = 2;
    while !used.insert(file.clone()) {
        file = format!("{name}-{n}.{ext}");
        n += 1;
    }
    file
}

/// Every file of a batch, in a stable order (view, detail, profile, format), with unique names.
pub fn jobs(views: &[ExportView], b: &BatchSettings, profiles: &[Profile]) -> Vec<Job> {
    let mut used = HashSet::new();
    let mut out = vec![];
    let mut levels: Vec<u8> = b.levels.iter().copied().filter(|l| *l <= 4).collect();
    levels.sort();
    levels.dedup();
    let formats: Vec<Format> = [Format::Png, Format::Svg, Format::Pdf].into_iter().filter(|f| b.formats.contains(f)).collect();
    let chosen: Vec<&ExportView> = views.iter().filter(|v| !b.excluded.contains(&v.key())).collect();
    let profile_names: Vec<String> = {
        let mut seen = HashSet::new();
        profiles.iter().map(|p| unique(slug(&p.name), "", &mut seen).trim_end_matches('.').to_string()).collect()
    };
    for v in &chosen {
        let lv: Vec<Option<u8>> = if v.has_detail() { levels.iter().map(|l| Some(*l)).collect() } else { vec![None] };
        for level in lv {
            for (pi, pname) in profile_names.iter().enumerate() {
                for f in &formats {
                    let mut name = v.key();
                    if let Some(l) = level {
                        name = format!("{name}-{}-{}", l, DETAIL_LABELS[l as usize]);
                    }
                    if profiles.len() > 1 {
                        name = format!("{name}-{pname}");
                    }
                    let ext = super::export::info(*f).ext;
                    out.push(Job { view: (*v).clone(), level, profile: pi, format: *f, file: unique(name, ext, &mut used) });
                }
            }
        }
    }
    if b.plantuml {
        let mut done = HashSet::new();
        for v in &chosen {
            let p = v.plantuml();
            if done.insert(p) {
                out.push(Job { view: (*v).clone(), level: None, profile: 0, format: Format::Puml, file: unique(format!("{p}-plantuml"), "puml", &mut used) });
            }
        }
    }
    out
}

/// A fresh output folder inside `parent` (never an existing one).
pub fn output_dir(parent: &Path, design: &str, stamp: &str) -> std::io::Result<PathBuf> {
    let base = format!("{}-diagrams-{stamp}", slug(design));
    let mut dir = parent.join(&base);
    let mut n = 2;
    while dir.exists() {
        dir = parent.join(format!("{base}-{n}"));
        n += 1;
    }
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Write through a temporary file in the same folder, then rename (no half-written files).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension(format!("{}.part", path.extension().and_then(|e| e.to_str()).unwrap_or("tmp")));
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })
}

/// Outcome of one job.
#[derive(Debug, Clone, PartialEq)]
pub enum JobResult {
    Written(String),
    Skipped(String),
    Failed(String),
}

/// A running batch: the plan, how far it got, and what happened.
pub struct BatchRun {
    pub dir: PathBuf,
    pub jobs: Vec<Job>,
    pub profiles: Vec<Profile>,
    pub next: usize,
    pub results: Vec<(String, JobResult)>,
    pub cancelled: bool,
    /// design revision the batch started from: a change aborts it (one consistent snapshot)
    pub rev: u64,
}

impl BatchRun {
    pub fn done(&self) -> bool {
        self.cancelled || self.next >= self.jobs.len()
    }
    pub fn counts(&self) -> (usize, usize, usize) {
        let w = self.results.iter().filter(|r| matches!(r.1, JobResult::Written(_))).count();
        let s = self.results.iter().filter(|r| matches!(r.1, JobResult::Skipped(_))).count();
        let f = self.results.iter().filter(|r| matches!(r.1, JobResult::Failed(_))).count();
        (w, s, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn views() -> Vec<ExportView> {
        vec![ExportView::SystemContext, ExportView::SystemContainers, ExportView::Class, ExportView::Domain, ExportView::Package, ExportView::Usecase, ExportView::Sequence("Borrow a book".into()), ExportView::Sequence("Borrow a book!".into())]
    }

    #[test]
    fn job_count_is_views_by_applicable_detail_by_profile_by_format() {
        let b = BatchSettings { levels: vec![3, 1, 3], formats: vec![Format::Svg, Format::Png], ..Default::default() };
        let profiles = vec![Profile::default(), Profile { name: "dark".into(), ..Default::default() }];
        let js = jobs(&views(), &b, &profiles);
        // 2 detail views × 2 levels + 6 others, × 2 profiles × 2 formats
        assert_eq!(js.len(), (2 * 2 + 6) * 2 * 2);
        assert!(js.iter().all(|j| j.level.is_some() == j.view.has_detail()));
        let names: HashSet<&String> = js.iter().map(|j| &j.file).collect();
        assert_eq!(names.len(), js.len(), "unique file names");
        assert!(js.iter().any(|j| j.file == "class-1-names-dark.png"));
        // the two scenarios with the same slug don't overwrite each other
        assert!(js.iter().any(|j| j.file == "sequence-borrow-a-book-current.svg"));
        assert!(js.iter().any(|j| j.file == "sequence-borrow-a-book-current-2.svg"));
    }

    #[test]
    fn exclusions_plantuml_once_per_view_and_single_profile_names() {
        let b = BatchSettings { excluded: vec!["package".into()], levels: vec![2], formats: vec![Format::Pdf], plantuml: true, profiles: vec![] };
        let js = jobs(&views(), &b, &[Profile::default()]);
        assert!(!js.iter().any(|j| j.view == ExportView::Package));
        let puml: Vec<&String> = js.iter().filter(|j| j.format == Format::Puml).map(|j| &j.file).collect();
        assert_eq!(puml, vec!["system-plantuml.puml", "class-plantuml.puml", "domain-plantuml.puml", "usecase-plantuml.puml", "sequence-plantuml.puml"]);
        assert!(js.iter().any(|j| j.file == "class-2-responsibilities.pdf"));
        assert!(js.iter().all(|j| !j.file.contains("current")));
    }

    #[test]
    fn output_folders_are_new_and_writes_are_atomic() {
        let tmp = tempfile::tempdir().unwrap();
        let a = output_dir(tmp.path(), "My Design", "20250101-1200").unwrap();
        let b = output_dir(tmp.path(), "My Design", "20250101-1200").unwrap();
        assert_ne!(a, b);
        assert!(a.ends_with("my-design-diagrams-20250101-1200"));
        let f = a.join("x.svg");
        write_atomic(&f, b"<svg/>").unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"<svg/>");
        assert_eq!(std::fs::read_dir(&a).unwrap().count(), 1, "no temporary file left");
    }
}
