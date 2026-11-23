//! Architecture Decision Records (MADR style) from accepted / overruled decisions.

use crate::model::*;
use std::fmt::Write;

pub fn slug(s: &str) -> String {
    let s: String = s.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let s = s.split('-').filter(|p| !p.is_empty()).take(8).collect::<Vec<_>>().join("-");
    if s.is_empty() { "decision".into() } else { s }
}

fn date(ms: Option<u64>) -> Option<String> {
    let ms = ms?;
    // civil date from days since epoch (Howard Hinnant's algorithm)
    let z = (ms / 86_400_000) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

/// `(file name, markdown)` per decided decision, numbered in decision order.
pub fn adrs(d: &Design) -> Vec<(String, String)> {
    d.decisions
        .iter()
        .filter(|x| matches!(x.state, DecisionState::Accepted | DecisionState::Overruled))
        .enumerate()
        .map(|(i, x)| (format!("{:04}-{}.md", i + 1, slug(&x.question)), adr(d, x, i + 1)))
        .collect()
}

pub fn adr(d: &Design, x: &Decision, n: usize) -> String {
    let mut o = String::new();
    let chosen = x.chosen.as_ref().and_then(|c| x.options.iter().find(|o| &o.id == c));
    let chosen_label = chosen.map(|o| o.label.clone()).or_else(|| x.custom.clone()).unwrap_or_else(|| "?".into());
    let last = x.attempts.last();
    let _ = writeln!(o, "# {n}. {}\n", x.question);
    let status = if x.state == DecisionState::Overruled { "accepted (examiner overruled)" } else { "accepted" };
    let _ = writeln!(o, "* Status: {status}");
    let _ = writeln!(o, "* Decision: {} in Spide design \"{}\"", x.id, d.name);
    if let Some(dt) = date(x.decided_at) {
        let _ = writeln!(o, "* Date: {dt}");
    }

    let _ = writeln!(o, "\n## Context and Problem Statement\n");
    let _ = writeln!(o, "{}", x.question);
    if let Some(c) = &x.context {
        let _ = writeln!(o, "\n{c}");
    }

    // drivers: forces the options trade off, else the must-have forces of the brief
    let mut drivers: Vec<String> = vec![];
    for t in x.options.iter().flat_map(|o| o.tradeoffs.iter()) {
        if !drivers.contains(&t.force) {
            drivers.push(t.force.clone());
        }
    }
    if drivers.is_empty() {
        drivers = d.live_forces().filter(|f| f.priority == 1).map(|f| f.name.clone()).collect();
    }
    if !drivers.is_empty() {
        let _ = writeln!(o, "\n## Decision Drivers\n");
        for name in &drivers {
            match d.force(name) {
                Some(f) => {
                    let desc = f.description.as_ref().map(|s| format!(": {s}")).unwrap_or_default();
                    let _ = writeln!(o, "* {name} ({}){desc}", f.kind.as_str());
                }
                None => {
                    let _ = writeln!(o, "* {name}");
                }
            }
        }
    }

    let _ = writeln!(o, "\n## Considered Options\n");
    for opt in &x.options {
        let _ = writeln!(o, "* {} — {}", opt.id, opt.label);
    }
    if x.chosen.is_none() {
        if let Some(c) = &x.custom {
            let _ = writeln!(o, "* own alternative — {c}");
        }
    }

    let _ = writeln!(o, "\n## Decision Outcome\n");
    let _ = write!(o, "Chosen option: \"{chosen_label}\"");
    match last {
        Some(a) if !a.defense.trim().is_empty() => {
            let _ = writeln!(o, ", because {}", a.defense.trim());
        }
        _ => {
            let _ = writeln!(o, ".");
        }
    }
    if !x.notes.trim().is_empty() {
        let _ = writeln!(o, "\nUser notes: {}", x.notes.trim());
    }
    if let Some(fb) = last.and_then(|a| a.feedback.as_ref()) {
        let who = if x.state == DecisionState::Overruled { "Examiner (overruled)" } else { "Examiner" };
        let _ = writeln!(o, "\n> {who}: {fb}");
    }

    if let Some(im) = &x.consequences {
        let _ = writeln!(o, "\n### Consequences\n");
        let _ = writeln!(o, "* Design preview: {}", im.summary());
        let list = |o: &mut String, label: &str, v: &Vec<String>| {
            if !v.is_empty() {
                let _ = writeln!(o, "* {label}: {}", v.join("; "));
            }
        };
        list(&mut o, "Added", &im.added);
        list(&mut o, "Changed", &im.changed);
        list(&mut o, "Removed", &im.removed);
        list(&mut o, "Good, because it resolved", &im.resolved_issues);
        list(&mut o, "Bad, because it introduced", &im.new_issues);
        if !im.flipped_deps.is_empty() {
            let f: Vec<String> = im.flipped_deps.iter().map(|(a, b)| format!("{a} → {b}")).collect();
            let _ = writeln!(o, "* Dependencies reversed: {}", f.join(", "));
        }
    }

    let _ = writeln!(o, "\n## Pros and Cons of the Options");
    for opt in &x.options {
        match &opt.pattern {
            Some(pat) if !pat.trim().is_empty() => {
                let _ = writeln!(o, "\n### {} — {} ({})\n", opt.id, opt.label, pat.trim());
            }
            _ => {
                let _ = writeln!(o, "\n### {} — {}\n", opt.id, opt.label);
            }
        }
        if let Some(s) = &opt.summary {
            let _ = writeln!(o, "{s}\n");
        }
        if let Some(d) = &opt.design_notes {
            if !d.trim().is_empty() {
                let _ = writeln!(o, "**Design & class organization:** {}\n", d.trim());
            }
        }
        if let Some(pd) = &opt.pros_detail {
            if !pd.trim().is_empty() {
                let _ = writeln!(o, "**Pros:** {}\n", pd.trim());
            }
        }
        for p in &opt.pros {
            let _ = writeln!(o, "* Good, because {p}");
        }
        if let Some(cd) = &opt.cons_detail {
            if !cd.trim().is_empty() {
                let _ = writeln!(o, "\n**Cons:** {}\n", cd.trim());
            }
        }
        for c in &opt.cons {
            let _ = writeln!(o, "* Bad, because {c}");
        }
        for t in &opt.tradeoffs {
            let note = t.note.as_ref().map(|n| format!(" — {n}")).unwrap_or_default();
            let _ = writeln!(o, "* {} {}{note}", if t.helps() { "Helps" } else { "Hurts" }, t.force);
        }
    }

    if x.attempts.len() > 1 {
        let _ = writeln!(o, "\n## Defence History\n");
        for (i, a) in x.attempts.iter().enumerate() {
            let _ = writeln!(o, "{}. \"{}\" — {}{}", i + 1, a.defense.trim(), a.verdict.as_deref().unwrap_or("no verdict"),
                a.feedback.as_ref().map(|f| format!(": {f}")).unwrap_or_default());
        }
    }
    o
}
