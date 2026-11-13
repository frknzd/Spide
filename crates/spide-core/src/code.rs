//! The implementation judged like the design: health of the imported code, real clones, and where
//! the code contradicts what the design says a class is for.

use crate::metrics::{compute, Issue, Metrics};
use crate::model::*;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Disagreement {
    pub class: String,
    pub method: Option<String>,
    /// What the design says (role, concern).
    pub design: String,
    /// What the code does.
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct CodeFindings {
    pub design_health: u8,
    pub code_health: u8,
    /// Issues found in the code (the same checks as the design, plus code-only ones).
    pub issues: Vec<Issue>,
    /// Method pairs whose normalised implementations match.
    pub clones: Vec<crate::overlap::MethodPair>,
    pub disagreements: Vec<Disagreement>,
}

/// The imported code with the design's rules and intent, judged like the design.
pub fn judged_reality(intended: &Design, reality: &Design) -> (Design, Metrics) {
    let r = crate::drift::with_design_rules(intended, reality);
    let m = compute(&r, false);
    (r, m)
}

pub fn analyse(intended: &Design, reality: &Design) -> CodeFindings {
    let dm = compute(intended, false);
    let (_, cm) = judged_reality(intended, reality);
    let mut out = CodeFindings { design_health: dm.health, code_health: cm.health, ..Default::default() };
    out.clones = cm.overlap.pairs.iter().filter(|p| p.code >= crate::overlap::DUPLICATE).cloned().collect();
    // design vs code: same class, the code does a different kind of work than the design says
    for (cls, cr) in &dm.responsibility.classes {
        let Some(code_cls) = cm.responsibility.classes.get(cls) else { continue };
        let role = cr.role.as_ref().map(|r| r.role.clone()).unwrap_or_default();
        for mi in cm.responsibility.methods.iter().filter(|m| &m.class == cls && m.source == "code") {
            let designed = dm.responsibility.method(cls, &mi.method).map(|d| d.concern.clone());
            if designed.as_deref() == Some(mi.concern.as_str()) {
                continue;
            }
            if crate::responsibility::misfit(&role, &mi.concern).is_none() {
                continue;
            }
            let what = match designed {
                Some(c) => format!("{cls} is {role}; {}() is designed as {}", mi.method, c.replace('_', " ")),
                None => format!("{cls} is {role}; {}() isn't in the design", mi.method),
            };
            out.disagreements.push(Disagreement {
                class: cls.clone(),
                method: Some(mi.method.clone()),
                design: what,
                code: format!("{}() {} → {}", mi.method, mi.reason.trim_start_matches("code: "), mi.concern.replace('_', " ")),
            });
        }
        let _ = code_cls;
    }
    // issues only the code has (by message, so the same finding in both isn't repeated)
    let known: std::collections::BTreeSet<&String> = dm.issues.iter().map(|i| &i.message).collect();
    out.issues = cm.issues.into_iter().filter(|i| !known.contains(&i.message)).collect();
    out
}

/// Source of a class's methods (or one method) from the imported code, read-only and size-capped,
/// with the facts Spide extracted, for the agent to judge the real implementation.
pub fn snippet(reality: &Design, code_dir: &std::path::Path, class: &str, method: Option<&str>) -> Result<String, String> {
    let c = reality.class(class).ok_or(format!("{class} is not in the imported code (/import first, or check the name)"))?;
    let ms: Vec<&Method> = c.methods.iter().filter(|m| method.is_none_or(|n| m.name == n)).collect();
    if ms.is_empty() {
        return Err(format!("{class}.{}() is not in the imported code", method.unwrap_or("")));
    }
    let mut out = String::new();
    let mut budget = 12_000usize;
    for m in ms {
        let Some(f) = &m.code else { continue };
        let path = code_dir.join(&f.file);
        // never read outside the imported folder
        let canon = path.canonicalize().map_err(|e| format!("{}: {e}", f.file))?;
        let root = code_dir.canonicalize().map_err(|e| e.to_string())?;
        if !canon.starts_with(&root) {
            return Err("refusing to read outside the imported folder".into());
        }
        let text = std::fs::read_to_string(&canon).map_err(|e| e.to_string())?;
        let lines: Vec<&str> = text.lines().collect();
        let (a, b) = (f.start_line.saturating_sub(1) as usize, (f.end_line as usize).min(lines.len()));
        let body = lines[a.min(b)..b].join("\n");
        let facts = format!(
            "// {}:{}-{} · {} loc · chain {} · switches {} · loops {} · creates [{}] · ui [{}]",
            f.file,
            f.start_line,
            f.end_line,
            f.loc,
            f.max_chain,
            f.switches,
            f.loops,
            f.creates.join(", "),
            f.ui_api.join(", ")
        );
        let piece = format!("{facts}\n{body}\n\n");
        if piece.len() > budget {
            out.push_str(&format!("// … {}() and further methods omitted (size cap); ask for one method at a time\n", m.name));
            break;
        }
        budget -= piece.len();
        out.push_str(&piece);
    }
    Ok(out)
}
