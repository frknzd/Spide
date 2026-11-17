//! Sequence diagrams ↔ Lucid sequence markup (PlantUML-style; Lucid stores and returns it verbatim).
//!
//! Lucid draws what the markup says, so the writer spells out what Spide's canvas derives: the
//! activations and the implicit replies (each marked with a `' spide:reply` comment so the reader folds
//! it back into the call's `returns`). Things the markup has no syntax for are encoded so they read
//! back exactly: `«create»`/`«destroy»` message prefixes, interaction uses as `note over a, b: ref X`,
//! and pruned messages as `' spide:pruned …` comment lines (kept, not drawn).

use crate::model::*;
use crate::sequence::{self, Event};

const REPLY: &str = "' spide:reply";
const PRUNED: &str = "' spide:pruned ";

fn alias(name: &str, taken: &mut Vec<String>) -> String {
    let mut s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
    if s.is_empty() || s.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        s.insert_str(0, "p_");
    }
    let base = s.clone();
    let mut n = 2;
    while taken.contains(&s) {
        s = format!("{base}_{n}");
        n += 1;
    }
    taken.push(s.clone());
    s
}

fn quote(s: &str) -> String {
    s.replace('"', "'").replace('\n', " ")
}

fn one_line(s: &str) -> String {
    s.replace('\n', " ").trim().to_string()
}

fn call_text(m: &Message) -> String {
    if m.method.contains('(') {
        m.method.clone()
    } else {
        format!("{}({})", m.method, m.args.as_deref().unwrap_or(""))
    }
}

fn message_line(m: &Message, id: &dyn Fn(&str) -> String) -> Option<String> {
    Some(match m.kind {
        MessageKind::Sync => format!("{} -> {}: {}", id(&m.from), id(&m.to), one_line(&call_text(m))),
        MessageKind::Async => format!("{} ->> {}: {}", id(&m.from), id(&m.to), one_line(&call_text(m))),
        MessageKind::Create => format!("{} -> {}: «create» {}", id(&m.from), id(&m.to), one_line(&call_text(m))),
        MessageKind::Destroy => format!("{} -> {}: «destroy»", id(&m.from), id(&m.to)),
        MessageKind::Return => {
            let l = if m.method.is_empty() { m.returns.clone().unwrap_or_default() } else { m.method.clone() };
            if l.trim().is_empty() {
                format!("{} --> {}", id(&m.from), id(&m.to))
            } else {
                format!("{} --> {}: {}", id(&m.from), id(&m.to), one_line(&l))
            }
        }
        MessageKind::Ref => {
            let mut over: Vec<String> = [&m.from, &m.to].iter().filter(|p| !p.is_empty()).map(|p| id(p)).collect();
            over.dedup();
            if over.is_empty() {
                return Some(format!("note across: ref {}", one_line(&m.method)));
            }
            format!("note over {}: ref {}", over.join(", "), one_line(&m.method))
        }
    })
}

/// The markup for a sequence. `is_actor` names actors (drawn as stick figures).
pub fn to_markup(s: &Sequence, is_actor: &dyn Fn(&str) -> bool) -> String {
    let mut taken = vec![];
    let ids: Vec<(String, String)> = s.participants.iter().map(|p| (p.clone(), alias(p, &mut taken))).collect();
    let id = |p: &str| ids.iter().find(|(n, _)| n == p).map(|(_, a)| a.clone()).unwrap_or_else(|| p.to_string());
    let mut out = vec![format!("title {}", one_line(&s.name))];
    for p in &s.participants {
        let actor = is_actor(s.class_of(p)) || is_actor(p);
        let label = s.lifeline(p).label(actor);
        let kw = if actor { "actor" } else { "participant" };
        out.push(format!("{kw} \"{}\" as {}", quote(&label), id(p)));
    }
    let t = sequence::timeline(s, is_actor);
    let mut done = vec![false; s.messages.len()];
    for e in &t.events {
        match e {
            Event::FragmentStart(k) => {
                let f = &t.fragments[*k];
                let g = f.operands.first().and_then(|o| o.guard.as_deref()).map(|g| one_line(g.trim_matches(['[', ']']))).unwrap_or_default();
                let head = match f.kind {
                    FragmentKind::Break => "group break".to_string(),
                    k => k.as_str().to_string(),
                };
                out.push(if g.is_empty() { head } else { format!("{head} {g}") });
            }
            Event::OperandStart { fragment, operand } => {
                let g = t.fragments[*fragment].operands[*operand].guard.as_deref().map(|g| one_line(g.trim_matches(['[', ']']))).unwrap_or_default();
                out.push(if g.is_empty() { "else".into() } else { format!("else {g}") });
            }
            Event::FragmentEnd(_) => out.push("end".into()),
            Event::Activate { lifeline, .. } => out.push(format!("activate {}", id(lifeline))),
            Event::Deactivate { lifeline } => out.push(format!("deactivate {}", id(lifeline))),
            Event::Return { from, to, label, .. } => {
                out.push(REPLY.into());
                out.push(match label.as_deref().filter(|l| !l.trim().is_empty()) {
                    Some(l) => format!("{} --> {}: {}", id(from), id(to), one_line(l)),
                    None => format!("{} --> {}", id(from), id(to)),
                });
            }
            Event::Message(i) => {
                let m = &s.messages[*i];
                done[*i] = true;
                let Some(line) = message_line(m, &id) else { continue };
                if m.status == Status::Pruned {
                    out.push(format!("{PRUNED}{line}"));
                    continue;
                }
                out.push(line);
                if let Some(n) = m.note.as_deref().filter(|n| !n.trim().is_empty()) {
                    out.push(format!("note right of {}: {}", id(&m.to), one_line(n)));
                }
            }
            Event::Create(_) | Event::Destroy(_) => {}
        }
    }
    // messages the timeline couldn't place (unknown lifelines): keep them, not drawn
    for (i, m) in s.messages.iter().enumerate() {
        if !done[i] {
            if let Some(line) = message_line(m, &id) {
                out.push(format!("{PRUNED}{line}"));
            }
        }
    }
    out.join("\n")
}

fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').to_string()
}

struct Decl {
    alias: String,
}

/// Lifeline from its label: `ui : LoanController`, `: Loan`, `loans : Loan[*]`, or an actor's name.
fn lifeline_of(label: &str, actor: bool) -> Lifeline {
    if actor {
        return Lifeline { name: label.to_string(), class: label.to_string(), multi: false };
    }
    let (name, class) = match label.split_once(':') {
        Some((n, c)) => (n.trim().to_string(), c.trim().to_string()),
        None => (label.trim().to_string(), label.trim().to_string()),
    };
    let (class, multi) = match class.strip_suffix("[*]") {
        Some(c) => (c.trim().to_string(), true),
        None => (class, false),
    };
    let name = if name.is_empty() { class.clone() } else { name };
    Lifeline { name, class, multi }
}

/// Parse markup back into a sequence (name, use case and status are the caller's to keep).
pub fn from_markup(name: &str, markup: &str) -> Result<Sequence, String> {
    let mut s = Sequence { name: name.to_string(), ..Default::default() };
    let mut decls: Vec<Decl> = vec![];
    let mut lifelines: Vec<Lifeline> = vec![];
    // fragments under construction: (index into s.fragments)
    let mut open: Vec<usize> = vec![];
    let mut reply_next = false;
    let lifeline_id = |decls: &mut Vec<Decl>, lifelines: &mut Vec<Lifeline>, participants: &mut Vec<String>, alias: &str| -> String {
        if let Some(i) = decls.iter().position(|d| d.alias == alias) {
            return lifelines[i].name.clone();
        }
        // implicitly declared
        decls.push(Decl { alias: alias.to_string() });
        let l = Lifeline { name: alias.to_string(), class: alias.to_string(), multi: false };
        participants.push(l.name.clone());
        lifelines.push(l.clone());
        l.name
    };
    for raw in markup.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("@startuml") || line.starts_with("@enduml") {
            continue;
        }
        if line == REPLY {
            reply_next = true;
            continue;
        }
        let (pruned, line) = match line.strip_prefix(PRUNED.trim_end()) {
            Some(rest) => (true, rest.trim()),
            None => (false, line),
        };
        if !pruned && line.starts_with('\'') {
            continue;
        }
        let first = line.split_whitespace().next().unwrap_or("");
        match first {
            "title" | "caption" | "autonumber" | "activate" | "deactivate" | "destroy" | "hide" | "skinparam" => continue,
            "participant" | "actor" | "boundary" | "control" | "entity" | "database" | "collections" | "queue" => {
                let rest = line[first.len()..].trim();
                let (label, al) = match rest.rsplit_once(" as ") {
                    Some((l, a)) => (unquote(l), a.trim().to_string()),
                    None => (unquote(rest), unquote(rest)),
                };
                let l = lifeline_of(&label, first == "actor");
                if !decls.iter().any(|d| d.alias == al) {
                    decls.push(Decl { alias: al });
                    s.participants.push(l.name.clone());
                    lifelines.push(l);
                }
                continue;
            }
            "loop" | "opt" | "alt" | "par" | "critical" | "group" | "break" => {
                let rest = line[first.len()..].trim();
                let (kind, guard) = match first {
                    "loop" => (FragmentKind::Loop, rest),
                    "opt" => (FragmentKind::Opt, rest),
                    "alt" => (FragmentKind::Alt, rest),
                    "par" => (FragmentKind::Par, rest),
                    "critical" => (FragmentKind::Critical, rest),
                    "break" => (FragmentKind::Break, rest),
                    _ => match rest.strip_prefix("break") {
                        Some(g) => (FragmentKind::Break, g.trim()),
                        None => (FragmentKind::Critical, rest),
                    },
                };
                let guard = (!guard.is_empty()).then(|| guard.trim_matches(['[', ']']).to_string());
                s.fragments.push(Fragment { kind, operands: vec![Operand { guard, first: s.messages.len(), last: usize::MAX }] });
                open.push(s.fragments.len() - 1);
                continue;
            }
            "else" => {
                if let Some(&k) = open.last() {
                    let n = s.messages.len();
                    let f = &mut s.fragments[k];
                    if let Some(o) = f.operands.last_mut() {
                        o.last = n.wrapping_sub(1);
                        if n == o.first {
                            o.last = usize::MAX; // empty operand
                        }
                    }
                    let g = line["else".len()..].trim();
                    f.operands.push(Operand { guard: (!g.is_empty()).then(|| g.trim_matches(['[', ']']).to_string()), first: n, last: usize::MAX });
                }
                continue;
            }
            "end" => {
                if let Some(k) = open.pop() {
                    let n = s.messages.len();
                    if let Some(o) = s.fragments[k].operands.last_mut() {
                        o.last = if n > o.first { n - 1 } else { usize::MAX };
                    }
                }
                continue;
            }
            "note" => {
                let rest = line["note".len()..].trim();
                let (head, text) = rest.split_once(':').map(|(h, t)| (h.trim(), t.trim())).unwrap_or((rest, ""));
                if let Some(target) = text.strip_prefix("ref ") {
                    let over: Vec<&str> = head.strip_prefix("over").map(|o| o.split(',').map(str::trim).filter(|x| !x.is_empty()).collect()).unwrap_or_default();
                    let from = over.first().map(|a| lifeline_id(&mut decls, &mut lifelines, &mut s.participants, a)).unwrap_or_default();
                    let to = over.last().map(|a| lifeline_id(&mut decls, &mut lifelines, &mut s.participants, a)).unwrap_or_default();
                    s.messages.push(Message { from, to, method: target.trim().to_string(), kind: MessageKind::Ref, status: if pruned { Status::Pruned } else { Status::Accepted }, ..Default::default() });
                } else if let Some(m) = s.messages.last_mut() {
                    m.note = Some(text.to_string()).filter(|t| !t.is_empty());
                }
                continue;
            }
            _ => {}
        }
        // a message: `a -> b: text`
        let Some((arrow_at, arrow)) = ["-->>", "->>", "-->", "->"].iter().filter_map(|a| line.find(a).map(|i| (i, *a))).min_by_key(|(i, a)| (*i, std::cmp::Reverse(a.len()))) else {
            continue;
        };
        let from_alias = line[..arrow_at].trim();
        let rest = &line[arrow_at + arrow.len()..];
        let (to_alias, text) = match rest.split_once(':') {
            Some((t, x)) => (t.trim(), x.trim()),
            None => (rest.trim(), ""),
        };
        if from_alias.is_empty() || to_alias.is_empty() {
            continue;
        }
        let from = lifeline_id(&mut decls, &mut lifelines, &mut s.participants, from_alias);
        let to = lifeline_id(&mut decls, &mut lifelines, &mut s.participants, to_alias);
        if reply_next {
            reply_next = false;
            // an implicit reply: its label is the answered call's return value
            if let Some(call) = s.messages.iter_mut().rev().find(|m| m.kind == MessageKind::Sync && m.from == to && m.to == from && m.returns.is_none()) {
                call.returns = (!text.is_empty()).then(|| text.to_string());
                continue;
            }
            if text.is_empty() {
                continue;
            }
        }
        let dashed = arrow.starts_with("--");
        let (kind, body) = if let Some(b) = text.strip_prefix("«create»").or_else(|| text.strip_prefix("<<create>>")) {
            (MessageKind::Create, b.trim())
        } else if text.starts_with("«destroy»") || text.starts_with("<<destroy>>") {
            (MessageKind::Destroy, "")
        } else if dashed {
            (MessageKind::Return, text)
        } else if arrow.ends_with(">>") {
            (MessageKind::Async, text)
        } else {
            (MessageKind::Sync, text)
        };
        let mut m = Message { from, to, kind, status: if pruned { Status::Pruned } else { Status::Accepted }, ..Default::default() };
        match kind {
            MessageKind::Return => m.method = body.to_string(),
            MessageKind::Destroy => {}
            _ => match body.find('(') {
                Some(i) if body.ends_with(')') => {
                    m.method = body[..i].trim().to_string();
                    let args = body[i + 1..body.len() - 1].trim();
                    m.args = (!args.is_empty()).then(|| args.to_string());
                }
                _ => m.method = body.to_string(),
            },
        }
        s.messages.push(m);
    }
    // drop empty operands and fragments; keep outer-before-inner order
    for f in &mut s.fragments {
        f.operands.retain(|o| o.last != usize::MAX && o.last >= o.first);
    }
    s.fragments.retain(|f| !f.operands.is_empty());
    // lifelines worth storing: those whose name differs from the class or are multi-objects
    s.lifelines = lifelines.into_iter().filter(|l| l.name != l.class || l.multi).collect();
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Sequence {
        Sequence {
            name: "borrow".into(),
            participants: vec!["Member".into(), "ui".into(), "svc".into(), "loan".into()],
            lifelines: vec![
                Lifeline { name: "ui".into(), class: "LoanController".into(), multi: false },
                Lifeline { name: "svc".into(), class: "LendingService".into(), multi: false },
                Lifeline { name: "loan".into(), class: "Loan".into(), multi: false },
            ],
            messages: vec![
                Message { from: "Member".into(), to: "ui".into(), method: "borrow".into(), args: Some("isbn".into()), ..Default::default() },
                Message { from: "ui".into(), to: "svc".into(), method: "lend".into(), args: Some("member, copy".into()), returns: Some("loan".into()), ..Default::default() },
                Message { from: "svc".into(), to: "loan".into(), method: "Loan".into(), kind: MessageKind::Create, ..Default::default() },
                Message { from: "svc".into(), to: "svc".into(), method: "audit".into(), note: Some("logged".into()), ..Default::default() },
                Message { from: "svc".into(), to: "loan".into(), method: "notify".into(), kind: MessageKind::Async, ..Default::default() },
            ],
            fragments: vec![
                Fragment { kind: FragmentKind::Alt, operands: vec![Operand { guard: Some("available".into()), first: 2, last: 3 }, Operand { guard: Some("else".into()), first: 4, last: 4 }] },
            ],
            ..Default::default()
        }
    }

    fn visible(s: &Sequence) -> (Vec<String>, Vec<Lifeline>, Vec<Message>, Vec<Fragment>) {
        (s.participants.clone(), s.lifelines.clone(), s.messages.clone(), s.fragments.clone())
    }

    #[test]
    fn markup_round_trips() {
        let s = sample();
        let actor = |n: &str| n == "Member";
        let mk = to_markup(&s, &actor);
        assert!(mk.contains("actor \"Member\" as Member"), "{mk}");
        assert!(mk.contains("participant \"ui : LoanController\" as ui"));
        assert!(mk.contains("svc -> loan: «create» Loan()"));
        assert!(mk.contains("alt available"));
        assert!(mk.contains("' spide:reply\nsvc --> ui: loan"), "{mk}");
        let back = from_markup("borrow", &mk).unwrap();
        assert_eq!(visible(&back), visible(&s), "{mk}");
    }

    #[test]
    fn pruned_and_refs_survive() {
        let mut s = sample();
        s.messages[4].status = Status::Pruned;
        s.messages.push(Message { from: "ui".into(), to: "svc".into(), method: "Pay fine".into(), kind: MessageKind::Ref, ..Default::default() });
        s.fragments.clear();
        let actor = |n: &str| n == "Member";
        let back = from_markup("borrow", &to_markup(&s, &actor)).unwrap();
        assert_eq!(back.messages, s.messages);
    }

    #[test]
    fn colleague_edits_parse() {
        let mk = "title x\nparticipant \"o : Order\" as o\nactor Customer\nCustomer -> o: checkout(cart)\no --> Customer: receipt\nloop each line\n  o -> o: total()\nend";
        let s = from_markup("x", mk).unwrap();
        assert_eq!(s.participants, vec!["o", "Customer"]);
        assert_eq!(s.lifelines, vec![Lifeline { name: "o".into(), class: "Order".into(), multi: false }]);
        assert_eq!(s.messages.len(), 3);
        assert_eq!(s.messages[0].args.as_deref(), Some("cart"));
        assert_eq!(s.messages[1].kind, MessageKind::Return);
        assert_eq!(s.fragments, vec![Fragment { kind: FragmentKind::Loop, operands: vec![Operand { guard: Some("each line".into()), first: 2, last: 2 }] }]);
    }
}
