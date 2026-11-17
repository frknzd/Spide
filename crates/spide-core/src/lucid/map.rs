//! Pull: what colleagues changed in a Lucid document, as design operations (ghost proposals).
//!
//! Three-way, like `git pull`: the *base* (what Lucid held after the last sync) and the *remote*
//! (what it holds now) are both projected onto the elements the diagram shows, keyed by Spide id
//! (or `lucid:<id>` for shapes colleagues drew). Only what changed between base and remote is
//! applied to the current design, so local work since the last sync is never reverted. Where both
//! sides changed the same element, the remote version goes into a separate *conflicts* proposal.
//! Everything the diagram doesn't show (descriptions, code links, responsibilities …) is kept.

use super::seq_markup;
use super::snapshot::Snapshot;
use super::uml_text as ut;
use super::{Bbox, BlockShape, Item, ItemKind, LAYOUT_SCALE};
use crate::model::*;
use crate::ops::{apply_all, Mode, Op};
use std::collections::BTreeMap;

/// A colleague's note or shape that maps to nothing in the design (becomes a Spide comment).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Note {
    pub lucid_id: String,
    pub text: String,
    /// Spide id of the nearest Spide item (what the note is probably about).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub near: Option<String>,
}

/// The result of mapping a pull.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pull {
    /// Additions and changes (renames first).
    pub changes: Vec<Op>,
    pub removals: Vec<Op>,
    /// Elements both sides changed: the remote version, for the user to decide.
    pub conflicts: Vec<Op>,
    /// Moved boxes (class and domain views).
    pub layout: Vec<Op>,
    pub notes: Vec<Note>,
    /// What couldn't be applied (e.g. edits to something deleted here).
    pub warnings: Vec<String>,
}

impl Pull {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty() && self.removals.is_empty() && self.conflicts.is_empty() && self.layout.is_empty()
    }
}

/// Three-way merge of keyed elements: remote changes since base are applied to `cur`; elements also
/// changed locally since base go to `conflicts` (the remote value; `None` = remote deleted it).
fn merge3<V: Clone + PartialEq>(cur: &BTreeMap<String, V>, base: &BTreeMap<String, V>, remote: &BTreeMap<String, V>, conflicts: &mut BTreeMap<String, Option<V>>, warnings: &mut Vec<String>, what: &str) -> BTreeMap<String, V> {
    let mut out = cur.clone();
    let keys: std::collections::BTreeSet<&String> = base.keys().chain(remote.keys()).collect();
    for k in keys {
        let (b, r, c) = (base.get(k), remote.get(k), cur.get(k));
        if b == r || c == r {
            continue;
        }
        if b.is_some() && c.is_none() {
            // deleted here, changed (or deleted) there
            if r.is_some() {
                warnings.push(format!("{what} {} was changed in Lucid but no longer exists here", k.split_once(':').map(|x| x.1).unwrap_or(k)));
            }
            continue;
        }
        if c != b && b.is_some() {
            conflicts.insert(k.clone(), r.cloned());
            continue;
        }
        match r {
            Some(v) => {
                out.insert(k.clone(), v.clone());
            }
            None => {
                out.remove(k);
            }
        }
    }
    out
}

fn apply_conflicts<V: Clone>(m: &BTreeMap<String, V>, conflicts: &BTreeMap<String, Option<V>>) -> BTreeMap<String, V> {
    let mut out = m.clone();
    for (k, v) in conflicts {
        match v {
            Some(v) => {
                out.insert(k.clone(), v.clone());
            }
            None => {
                out.remove(k);
            }
        }
    }
    out
}

fn item_key(i: &Item) -> String {
    i.sid.clone().unwrap_or_else(|| format!("lucid:{}", i.lucid_id.clone().unwrap_or_default()))
}

/// The smallest frame (package, boundary) containing a box's centre.
fn frame_of(frames: &[(String, Bbox)], b: Option<Bbox>) -> Option<&str> {
    let c = b?.centre();
    frames.iter().filter(|(_, f)| f.contains_point(c)).min_by(|a, b| (a.1.w * a.1.h).total_cmp(&(b.1.w * b.1.h))).map(|(n, _)| n.as_str())
}

/// The Spide-owned box nearest to a box's centre (distance to its edges).
fn nearest(items: &[Item], b: Option<Bbox>) -> Option<String> {
    let c = b?.centre();
    items
        .iter()
        .filter(|i| i.sid.is_some() && !i.kind.is_line() && !matches!(i.kind, ItemKind::Package { .. } | ItemKind::Boundary { .. }))
        .filter_map(|i| {
            let r = i.bbox?;
            let dx = (r.x - c.0).max(c.0 - (r.x + r.w)).max(0.0);
            let dy = (r.y - c.1).max(c.1 - (r.y + r.h)).max(0.0);
            Some(((dx * dx + dy * dy).sqrt(), i.sid.clone()?))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|x| x.1)
}

fn notes_of(remote: &Snapshot) -> Vec<Note> {
    remote
        .items
        .iter()
        .filter_map(|i| match &i.kind {
            ItemKind::Other { text, .. } if !text.trim().is_empty() => Some(Note { lucid_id: i.lucid_id.clone().unwrap_or_default(), text: text.trim().to_string(), near: nearest(&remote.items, i.bbox) }),
            _ => None,
        })
        .collect()
}

/// Ops that apply on top of `base` (each kept only if the batch so far still applies).
fn keep_applicable(base: &Design, ops: Vec<Op>, warnings: &mut Vec<String>) -> Vec<Op> {
    let mut d = base.clone();
    let mut out = vec![];
    for op in ops {
        match apply_all(&d, std::slice::from_ref(&op), Mode::Ghost("lucid-check")) {
            Ok(next) => {
                d = next;
                out.push(op);
            }
            Err(e) => warnings.push(format!("skipped \"{}\": {}", op.summary(), e.0)),
        }
    }
    out
}

fn is_removal(o: &Op) -> bool {
    matches!(o, Op::RemoveClass { .. } | Op::RemoveAttribute { .. } | Op::RemoveMethod { .. } | Op::RemoveRelation { .. } | Op::RemovePackage { .. } | Op::RemoveContainer { .. } | Op::RemoveSystem { .. } | Op::RemoveLink { .. })
}

/// Map a pull of diagram `key`. `base` is what Lucid held after the last sync (the target Spide
/// wrote, when the document was never synced); `remote` what it holds now.
pub fn pull(current: &Design, key: &str, base: &[Item], remote: &Snapshot) -> Pull {
    let mut p = match key {
        "class" => class_pull(current, base, &remote.items),
        "domain" => domain_pull(current, base, &remote.items),
        "usecase" => usecase_pull(current, base, &remote.items),
        "package" => package_pull(current, base, &remote.items),
        "system.context" | "system.containers" => c4_pull(current, base, &remote.items),
        k => match k.strip_prefix("sequence:") {
            Some(name) => sequence_pull(current, name, base, &remote.items),
            None => Pull { warnings: vec![format!("unknown diagram {key}")], ..Default::default() },
        },
    };
    p.notes = notes_of(remote);
    p
}

// ------------------------------------------------------------------ class diagram

#[derive(Debug, Clone, PartialEq)]
struct Head {
    name: String,
    stereotype: Option<String>,
    is_abstract: bool,
    package: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct RelV {
    from: String,
    to: String,
    kind: RelationKind,
    directed: bool,
    label: Option<String>,
    interaction: Interaction,
    from_mult: Option<String>,
    to_mult: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum Member {
    Attr(Attribute),
    Meth(Box<Method>),
}

#[derive(Default, Clone)]
struct ClassProj {
    heads: BTreeMap<String, Head>,
    /// `<class key>/a:<name>` · `<class key>/m:<name>#<n>`
    members: BTreeMap<String, Member>,
    rels: BTreeMap<String, RelV>,
    pos: BTreeMap<String, (f64, f64)>,
}

fn none_if_empty(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

fn head_of_title(title: &str, package: Option<String>) -> Head {
    let t = ut::parse_title(title);
    let (stereotype, is_abstract) = (t.stereotype, t.is_abstract);
    Head { name: t.name, stereotype, is_abstract, package }
}

fn member_keys(class_key: &str, attrs: &[Attribute], meths: &[Method], out: &mut BTreeMap<String, Member>) {
    for a in attrs {
        out.insert(format!("{class_key}/a:{}", a.name), Member::Attr(a.clone()));
    }
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for m in meths {
        let n = seen.entry(m.name.clone()).or_default();
        out.insert(format!("{class_key}/m:{}#{n}", m.name), Member::Meth(Box::new(m.clone())));
        *n += 1;
    }
}

fn rel_value(rel: &str, label: &str, from_mult: &str, to_mult: &str, from: String, to: String) -> RelV {
    let (kind, directed) = ut::relation_kind(rel);
    let (interaction, label) = ut::parse_relation_label(label);
    RelV { from, to, kind, directed, label, interaction, from_mult: none_if_empty(from_mult), to_mult: none_if_empty(to_mult) }
}

/// Project Lucid items (class view) onto heads, members, relations and positions.
fn class_proj(items: &[Item]) -> ClassProj {
    let frames: Vec<(String, Bbox)> = items.iter().filter_map(|i| match &i.kind {
        ItemKind::Package { name, .. } => Some((name.trim().to_string(), i.bbox?)),
        _ => None,
    }).collect();
    let mut p = ClassProj::default();
    for i in items {
        if let ItemKind::Class { name, attributes, operations, .. } = &i.kind {
            let key = item_key(i);
            p.heads.insert(key.clone(), head_of_title(name, frame_of(&frames, i.bbox).map(str::to_string)));
            let attrs: Vec<Attribute> = attributes.iter().filter_map(|l| ut::parse_attribute(l)).collect();
            let meths: Vec<Method> = operations.iter().filter_map(|l| ut::parse_method(l)).collect();
            member_keys(&key, &attrs, &meths, &mut p.members);
            if let Some(b) = i.bbox {
                p.pos.insert(key, (b.x, b.y));
            }
        }
    }
    for i in items {
        if let ItemKind::Relationship { from, to, rel, label, from_mult, to_mult } = &i.kind {
            if p.heads.contains_key(from) && p.heads.contains_key(to) {
                p.rels.insert(item_key(i), rel_value(rel, label, from_mult, to_mult, from.clone(), to.clone()));
            }
        }
    }
    p
}

/// The current design's class view, the way it reads back from Lucid.
fn class_proj_of_design(d: &Design) -> ClassProj {
    let mut p = ClassProj::default();
    for c in d.classes.iter().filter(|c| c.status.is_live()) {
        let key = format!("class:{}", c.name);
        p.heads.insert(key.clone(), head_of_title(&ut::format_title(c), c.package.clone().filter(|_| true)));
        let attrs: Vec<Attribute> = c.attributes.iter().filter(|a| a.status.is_live()).filter_map(|a| ut::parse_attribute(&ut::format_attribute(a))).collect();
        let meths: Vec<Method> = c.methods.iter().filter(|m| m.status.is_live()).filter_map(|m| ut::parse_method(&ut::format_method(m))).collect();
        member_keys(&key, &attrs, &meths, &mut p.members);
        if let Some(pos) = d.layout.get("class").and_then(|l| l.get(&c.name)) {
            p.pos.insert(key, (pos.x, pos.y));
        }
    }
    for r in d.live_relations() {
        p.rels.insert(
            format!("rel:{}", r.id),
            RelV { from: format!("class:{}", r.from), to: format!("class:{}", r.to), kind: r.kind, directed: r.directed, label: r.label.clone().filter(|l| !l.trim().is_empty()), interaction: r.interaction, from_mult: r.from_mult.clone().filter(|m| !m.trim().is_empty()), to_mult: r.to_mult.clone().filter(|m| !m.trim().is_empty()) },
        );
    }
    p
}

/// Packages only drawn as frames when they hold classes: a class whose package has no frame in Lucid
/// keeps its package unless the frame was there before.
fn normalise_packages(p: &mut ClassProj, framed: &std::collections::BTreeSet<String>) {
    for h in p.heads.values_mut() {
        if h.package.as_ref().is_some_and(|x| !framed.contains(x)) {
            h.package = None;
        }
    }
}

/// Build the design a merged projection describes, on top of `cur` (renames already applied).
fn build_class_design(cur: &Design, m: &ClassProj, name_of_key: &dyn Fn(&str) -> Option<String>) -> Design {
    let mut d = cur.clone();
    let key_name = |k: &str| -> String { m.heads.get(k).map(|h| h.name.clone()).or_else(|| name_of_key(k)).unwrap_or_default() };
    // classes
    let live_names: Vec<String> = d.classes.iter().filter(|c| c.status.is_live()).map(|c| c.name.clone()).collect();
    let mut keep: Vec<String> = vec![];
    for (key, h) in &m.heads {
        if h.name.trim().is_empty() {
            continue;
        }
        let existing = name_of_key(key).filter(|n| live_names.contains(n));
        let idx = match existing.and_then(|n| d.classes.iter().position(|c| c.name == n && c.status.is_live())) {
            Some(i) => i,
            None => {
                d.classes.push(Class { name: h.name.clone(), status: Status::Accepted, origin: Some("lucid".into()), ..Default::default() });
                d.classes.len() - 1
            }
        };
        let c = &mut d.classes[idx];
        // overlay what the title shows, only where it differs from how the class reads now
        let now = head_of_title(&ut::format_title(c), c.package.clone());
        if now.stereotype != h.stereotype || now.is_abstract != h.is_abstract {
            c.stereotype = h.stereotype.clone();
            c.is_abstract = h.is_abstract;
        }
        c.package = h.package.clone();
        keep.push(h.name.clone());
        // members
        let prefix = format!("{key}/");
        let mine: Vec<(&String, &Member)> = m.members.iter().filter(|(k, _)| k.starts_with(&prefix)).collect();
        let mut attrs: Vec<Attribute> = c.attributes.iter().filter(|a| !a.status.is_live()).cloned().collect();
        let mut new_attrs: Vec<Attribute> = vec![];
        for (_, v) in &mine {
            if let Member::Attr(a) = v {
                match c.attributes.iter().find(|x| x.status.is_live() && x.name == a.name) {
                    Some(x) => {
                        let shown = ut::parse_attribute(&ut::format_attribute(x));
                        new_attrs.push(if shown.as_ref() == Some(a) { x.clone() } else { Attribute { status: x.status, proposal: x.proposal.clone(), origin: x.origin.clone(), ..a.clone() } });
                    }
                    None => new_attrs.push(Attribute { origin: Some("lucid".into()), ..a.clone() }),
                }
            }
        }
        // keep the current order, new members after
        let order = |n: &str| c.attributes.iter().position(|x| x.name == n).unwrap_or(usize::MAX);
        new_attrs.sort_by_key(|a| order(&a.name));
        attrs.extend(new_attrs);
        let mut meths: Vec<Method> = c.methods.iter().filter(|x| !x.status.is_live()).cloned().collect();
        let mut new_meths: Vec<Method> = vec![];
        let mut used: Vec<usize> = vec![];
        for (_, v) in &mine {
            if let Member::Meth(mm) = v {
                let found = c.methods.iter().enumerate().find(|(i, x)| x.status.is_live() && x.name == mm.name && !used.contains(i));
                match found {
                    Some((i, x)) => {
                        used.push(i);
                        let shown = ut::parse_method(&ut::format_method(x));
                        new_meths.push(if shown.as_ref() == Some(&**mm) {
                            x.clone()
                        } else {
                            Method { params: mm.params.clone(), returns: mm.returns.clone(), visibility: mm.visibility, is_static: mm.is_static, is_abstract: mm.is_abstract, ..x.clone() }
                        });
                    }
                    None => new_meths.push(Method { origin: Some("lucid".into()), ..(**mm).clone() }),
                }
            }
        }
        let morder = |n: &str| c.methods.iter().position(|x| x.name == n).unwrap_or(usize::MAX);
        new_meths.sort_by_key(|x| morder(&x.name));
        meths.extend(new_meths);
        c.attributes = attrs;
        c.methods = meths;
    }
    d.classes.retain(|c| !c.status.is_live() || keep.contains(&c.name));
    // packages a class moved into
    for c in d.classes.clone().iter().filter(|c| c.status.is_live()) {
        if let Some(p) = &c.package {
            if !d.packages.iter().any(|x| &x.name == p && x.status != Status::Pruned) {
                d.packages.push(Package { name: p.clone(), layer: crate::ops::guess_layer(p), origin: Some("lucid".into()), ..Default::default() });
            }
        }
    }
    // relations
    let mut rels: Vec<Relation> = d.relations.iter().filter(|r| !r.status.is_live()).cloned().collect();
    for (key, v) in &m.rels {
        let (from, to) = (key_name(&v.from), key_name(&v.to));
        if from.is_empty() || to.is_empty() {
            continue;
        }
        let id = relation_id(&from, &to, v.kind);
        let existing = key.strip_prefix("rel:").and_then(|rid| d.relations.iter().find(|r| r.id == rid && r.status.is_live())).or_else(|| d.relations.iter().find(|r| r.id == id && r.status.is_live()));
        let mut r = existing.cloned().unwrap_or_else(|| Relation { origin: Some("lucid".into()), ..Default::default() });
        r.id = id;
        r.from = from;
        r.to = to;
        r.kind = v.kind;
        r.directed = v.directed;
        r.label = v.label.clone();
        r.interaction = v.interaction;
        r.from_mult = v.from_mult.clone();
        r.to_mult = v.to_mult.clone();
        r.status = if r.status.is_live() { r.status } else { Status::Accepted };
        if !rels.iter().any(|x| x.id == r.id && x.status.is_live()) {
            rels.push(r);
        }
    }
    d.relations = rels;
    d
}

fn class_pull(current: &Design, base_items: &[Item], remote_items: &[Item]) -> Pull {
    let mut out = Pull::default();
    let mut base = class_proj(base_items);
    let mut remote = class_proj(remote_items);
    let mut cur = class_proj_of_design(current);
    // a package without a frame in Lucid (base) can't have been changed there
    let framed = |items: &[Item]| items.iter().filter_map(|i| if let ItemKind::Package { name, .. } = &i.kind { Some(name.trim().to_string()) } else { None }).collect::<std::collections::BTreeSet<_>>();
    let (fb, fr) = (framed(base_items), framed(remote_items));
    let framed_any: std::collections::BTreeSet<String> = fb.union(&fr).cloned().collect();
    normalise_packages(&mut base, &framed_any);
    normalise_packages(&mut remote, &framed_any);
    normalise_packages(&mut cur, &framed_any);
    // renames: same Spide id, another name (applied first, so the rest diffs by the new names)
    let mut renamed = current.clone();
    let mut renames: Vec<Op> = vec![];
    let mut name_now: BTreeMap<String, String> = BTreeMap::new();
    for (key, rh) in &remote.heads {
        let Some(old) = key.strip_prefix("class:") else { continue };
        let bname = base.heads.get(key).map(|h| h.name.as_str()).unwrap_or(old);
        if rh.name != bname && !rh.name.trim().is_empty() && renamed.live_class(old).is_some() && renamed.live_class(&rh.name).is_none() {
            let op = Op::RenameClass { from: old.to_string(), to: rh.name.clone() };
            if let Ok(next) = apply_all(&renamed, std::slice::from_ref(&op), Mode::Commit(Status::Accepted, None)) {
                renamed = next;
                renames.push(op);
                name_now.insert(key.clone(), rh.name.clone());
            }
        }
    }
    // the current projection under the new names (keys stay the Spide ids)
    for (k, n) in &name_now {
        if let Some(h) = cur.heads.get_mut(k) {
            h.name = n.clone();
        }
        if let Some(h) = base.heads.get_mut(k) {
            h.name = n.clone();
        }
    }
    let mut ch = BTreeMap::new();
    let mut cm = BTreeMap::new();
    let mut cr = BTreeMap::new();
    let heads = merge3(&cur.heads, &base.heads, &remote.heads, &mut ch, &mut out.warnings, "class");
    let members = merge3(&cur.members, &base.members, &remote.members, &mut cm, &mut out.warnings, "member");
    let rels = merge3(&cur.rels, &base.rels, &remote.rels, &mut cr, &mut out.warnings, "relation");
    let name_of_key = |k: &str| -> Option<String> { name_now.get(k).cloned().or_else(|| k.strip_prefix("class:").map(str::to_string)) };
    let clean = ClassProj { heads: heads.clone(), members: members.clone(), rels: rels.clone(), pos: BTreeMap::new() };
    let merged = build_class_design(&renamed, &clean, &name_of_key);
    let ops = crate::diff::diff(&renamed, &merged);
    let ops = keep_applicable(&renamed, ops, &mut out.warnings);
    let (removals, changes): (Vec<Op>, Vec<Op>) = ops.into_iter().partition(is_removal);
    out.changes = renames.into_iter().chain(changes).collect();
    out.removals = removals;
    if !(ch.is_empty() && cm.is_empty() && cr.is_empty()) {
        let all = ClassProj { heads: apply_conflicts(&heads, &ch), members: apply_conflicts(&members, &cm), rels: apply_conflicts(&rels, &cr), pos: BTreeMap::new() };
        let with = build_class_design(&renamed, &all, &name_of_key);
        let after_clean = apply_all(&renamed, &out.changes.iter().skip_while(|o| matches!(o, Op::RenameClass { .. })).cloned().chain(out.removals.iter().cloned()).collect::<Vec<_>>(), Mode::Commit(Status::Accepted, None)).unwrap_or(merged.clone());
        let mut w = vec![];
        out.conflicts = keep_applicable(&after_clean, crate::diff::diff(&merged, &with), &mut w);
        out.warnings.extend(w);
    }
    out.layout = layout_ops("class", &base, &remote, &name_now, current);
    out
}

/// Moved boxes → pinned canvas positions (Lucid position / [`LAYOUT_SCALE`]).
fn layout_ops(view: &str, base: &ClassProj, remote: &ClassProj, renamed: &BTreeMap<String, String>, current: &Design) -> Vec<Op> {
    let mut positions = BTreeMap::new();
    for (key, (x, y)) in &remote.pos {
        let Some(&(bx, by)) = base.pos.get(key) else { continue };
        if ((x - bx).abs() < 5.0 && (y - by).abs() < 5.0) || !key.contains(':') {
            continue;
        }
        let Some(old) = key.split_once(':').map(|x| x.1) else { continue };
        let name = renamed.get(key).cloned().unwrap_or_else(|| old.to_string());
        let exists = if view == "class" { current.live_class(old).is_some() } else { current.domain.concepts.iter().any(|c| c.name == old) };
        if exists {
            positions.insert(name, Pos { x: (x / LAYOUT_SCALE).round(), y: (y / LAYOUT_SCALE).round(), pinned: true });
        }
    }
    if positions.is_empty() {
        vec![]
    } else {
        vec![Op::SetLayout { view: view.to_string(), positions }]
    }
}

// ------------------------------------------------------------------ domain model

fn concept_link_kind(rel: &str) -> ConceptLinkKind {
    match rel {
        "generalization" | "inheritance" | "realization" => ConceptLinkKind::Generalization,
        "composition" => ConceptLinkKind::Composition,
        "aggregation" => ConceptLinkKind::Aggregation,
        _ => ConceptLinkKind::Association,
    }
}

/// A concept link: (from key, to key, kind, label, from_mult, to_mult).
type LinkV = (String, String, ConceptLinkKind, Option<String>, Option<String>, Option<String>);
/// Concepts and links by key, and box positions.
type DomainProj = (BTreeMap<String, Concept>, BTreeMap<String, LinkV>, BTreeMap<String, (f64, f64)>);

fn domain_proj(items: &[Item]) -> DomainProj {
    let mut cs = BTreeMap::new();
    let mut pos = BTreeMap::new();
    for i in items {
        if let ItemKind::Class { name, attributes, .. } = &i.kind {
            let key = item_key(i);
            cs.insert(key.clone(), Concept { name: ut::parse_title(name).name, attributes: attributes.iter().filter_map(|l| ut::parse_concept_attribute(l)).collect(), ..Default::default() });
            if let Some(b) = i.bbox {
                pos.insert(key, (b.x, b.y));
            }
        }
    }
    let mut ls = BTreeMap::new();
    for i in items {
        if let ItemKind::Relationship { from, to, rel, label, from_mult, to_mult } = &i.kind {
            if cs.contains_key(from) && cs.contains_key(to) {
                ls.insert(item_key(i), (from.clone(), to.clone(), concept_link_kind(rel), none_if_empty(label), none_if_empty(from_mult), none_if_empty(to_mult)));
            }
        }
    }
    (cs, ls, pos)
}

fn domain_pull(current: &Design, base_items: &[Item], remote_items: &[Item]) -> Pull {
    let mut out = Pull::default();
    let (bc, bl, bp) = domain_proj(base_items);
    let (rc, rl, rp) = domain_proj(remote_items);
    let shown = |s: Status| s != Status::Pruned && s != Status::Proposed;
    let cc: BTreeMap<String, Concept> = current.domain.concepts.iter().filter(|c| shown(c.status)).map(|c| (format!("concept:{}", c.name), Concept { name: c.name.clone(), attributes: c.attributes.clone(), ..Default::default() })).collect();
    let cl: BTreeMap<String, LinkV> = current
        .domain
        .associations
        .iter()
        .filter(|a| shown(a.status))
        .map(|a| (format!("assoc:{}", a.id), (format!("concept:{}", a.from), format!("concept:{}", a.to), a.kind, a.label.clone().filter(|l| !l.is_empty()), a.from_mult.clone().filter(|m| !m.is_empty()), a.to_mult.clone().filter(|m| !m.is_empty()))))
        .collect();
    let (mut kc, mut kl) = (BTreeMap::new(), BTreeMap::new());
    let concepts = merge3(&cc, &bc, &rc, &mut kc, &mut out.warnings, "concept");
    let links = merge3(&cl, &bl, &rl, &mut kl, &mut out.warnings, "association");
    let build = |concepts: &BTreeMap<String, Concept>, links: &BTreeMap<String, LinkV>| -> DomainModel {
        let name = |k: &str| concepts.get(k).map(|c| c.name.clone());
        let mut m = DomainModel { generated_rev: current.domain.generated_rev, ..Default::default() };
        for (k, c) in concepts {
            let old = k.strip_prefix("concept:").and_then(|n| current.domain.concepts.iter().find(|x| x.name == n));
            m.concepts.push(Concept { name: c.name.clone(), attributes: c.attributes.clone(), description: old.and_then(|o| o.description.clone()), feedback: old.and_then(|o| o.feedback.clone()), status: Status::Accepted });
        }
        // keep the current order
        let order = |n: &str| current.domain.concepts.iter().position(|c| c.name == n).unwrap_or(usize::MAX);
        m.concepts.sort_by_key(|c| order(&c.name));
        for (k, (f, t, kind, label, fm, tm)) in links {
            let (Some(from), Some(to)) = (name(f), name(t)) else { continue };
            let old = k.strip_prefix("assoc:").and_then(|id| current.domain.associations.iter().find(|a| a.id == id));
            let id = old.map(|o| o.id.clone()).unwrap_or_else(|| format!("{from}-{}-{to}", kind.as_str()));
            m.associations.push(ConceptLink { id, from, to, kind: *kind, label: label.clone(), from_mult: fm.clone(), to_mult: tm.clone(), status: Status::Accepted, feedback: old.and_then(|o| o.feedback.clone()) });
        }
        m
    };
    let visible = |m: &DomainModel| -> (Vec<Concept>, Vec<ConceptLink>) {
        (m.concepts.iter().filter(|c| shown(c.status)).cloned().collect(), m.associations.iter().filter(|a| shown(a.status)).cloned().collect())
    };
    let merged = build(&concepts, &links);
    let mut now = current.domain.clone();
    now.concepts.retain(|c| shown(c.status));
    now.associations.retain(|a| shown(a.status));
    if visible(&merged) != visible(&now) {
        out.changes.push(Op::SetDomainModel { model: merged.clone() });
    }
    if !(kc.is_empty() && kl.is_empty()) {
        out.conflicts.push(Op::SetDomainModel { model: build(&apply_conflicts(&concepts, &kc), &apply_conflicts(&links, &kl)) });
    }
    let as_proj = |p: BTreeMap<String, (f64, f64)>| ClassProj { pos: p, ..Default::default() };
    out.layout = layout_ops("domain", &as_proj(bp), &as_proj(rp), &BTreeMap::new(), current);
    out
}

// ------------------------------------------------------------------ use cases

#[derive(Debug, Clone, PartialEq)]
enum UcLink {
    Actor(String, String),
    Include(String, String),
    Extend(String, String),
}

/// Actors, use cases and links by key, and the system's name.
type UcProj = (BTreeMap<String, String>, BTreeMap<String, String>, BTreeMap<String, UcLink>, Option<String>);

fn uc_proj(items: &[Item]) -> UcProj {
    let (mut actors, mut ucs, mut links) = (BTreeMap::new(), BTreeMap::new(), BTreeMap::new());
    let mut system = None;
    for i in items {
        match &i.kind {
            ItemKind::Actor { name } => {
                actors.insert(item_key(i), name.trim().to_string());
            }
            ItemKind::UseCase { name } => {
                ucs.insert(item_key(i), name.trim().to_string());
            }
            ItemKind::Boundary { name } => system = Some(name.trim().to_string()),
            _ => {}
        }
    }
    for i in items {
        if let ItemKind::Relationship { from, to, label, .. } | ItemKind::Line { from, to, label } = &i.kind {
            let l = label.to_lowercase();
            let link = if actors.contains_key(from) && ucs.contains_key(to) {
                UcLink::Actor(from.clone(), to.clone())
            } else if actors.contains_key(to) && ucs.contains_key(from) {
                UcLink::Actor(to.clone(), from.clone())
            } else if ucs.contains_key(from) && ucs.contains_key(to) && l.contains("extend") {
                UcLink::Extend(from.clone(), to.clone())
            } else if ucs.contains_key(from) && ucs.contains_key(to) {
                UcLink::Include(from.clone(), to.clone())
            } else {
                continue;
            };
            links.insert(item_key(i), link);
        }
    }
    (actors, ucs, links, system)
}

fn usecase_pull(current: &Design, base_items: &[Item], remote_items: &[Item]) -> Pull {
    let mut out = Pull::default();
    let (ba, bu, bl, bs) = uc_proj(base_items);
    let (ra, ru, rl, rs) = uc_proj(remote_items);
    let shown = |s: Status| s != Status::Pruned && s != Status::Proposed;
    let m = &current.use_cases;
    let ca: BTreeMap<String, String> = m.actors.iter().filter(|a| shown(a.status)).map(|a| (format!("actor:{}", a.name), a.name.clone())).collect();
    let cu: BTreeMap<String, String> = m.use_cases.iter().filter(|u| shown(u.status)).map(|u| (format!("usecase:{}", u.name), u.name.clone())).collect();
    let mut cl = BTreeMap::new();
    for u in m.use_cases.iter().filter(|u| shown(u.status)) {
        let uk = format!("usecase:{}", u.name);
        for a in &u.actors {
            cl.insert(format!("ucactor:{a}>{}", u.name), UcLink::Actor(format!("actor:{a}"), uk.clone()));
        }
        for t in &u.includes {
            cl.insert(format!("ucinclude:{}>{t}", u.name), UcLink::Include(uk.clone(), format!("usecase:{t}")));
        }
        for t in &u.extends {
            cl.insert(format!("ucextend:{}>{t}", u.name), UcLink::Extend(uk.clone(), format!("usecase:{t}")));
        }
    }
    let (mut k1, mut k2, mut k3) = (BTreeMap::new(), BTreeMap::new(), BTreeMap::new());
    let actors = merge3(&ca, &ba, &ra, &mut k1, &mut out.warnings, "actor");
    let ucs = merge3(&cu, &bu, &ru, &mut k2, &mut out.warnings, "use case");
    let links = merge3(&cl, &bl, &rl, &mut k3, &mut out.warnings, "use case link");
    let system = if bs != rs { rs.clone().or(m.system.clone()) } else { m.system.clone() };
    let build = |actors: &BTreeMap<String, String>, ucs: &BTreeMap<String, String>, links: &BTreeMap<String, UcLink>| -> UseCaseModel {
        let mut out = UseCaseModel { system: system.clone(), generated_rev: m.generated_rev, ..Default::default() };
        for (k, name) in actors {
            let old = k.strip_prefix("actor:").and_then(|n| m.actors.iter().find(|a| a.name == n));
            out.actors.push(Actor { name: name.clone(), description: old.and_then(|o| o.description.clone()), feedback: old.and_then(|o| o.feedback.clone()), status: Status::Accepted });
        }
        for (k, name) in ucs {
            let old = k.strip_prefix("usecase:").and_then(|n| m.use_cases.iter().find(|u| u.name == n));
            let mut u = UseCase { name: name.clone(), classes: old.map(|o| o.classes.clone()).unwrap_or_default(), description: old.and_then(|o| o.description.clone()), feedback: old.and_then(|o| o.feedback.clone()), ..Default::default() };
            for l in links.values() {
                match l {
                    UcLink::Actor(a, uc) if uc == k => {
                        if let Some(an) = actors.get(a) {
                            u.actors.push(an.clone());
                        }
                    }
                    UcLink::Include(f, t) if f == k => {
                        if let Some(tn) = ucs.get(t) {
                            u.includes.push(tn.clone());
                        }
                    }
                    UcLink::Extend(f, t) if f == k => {
                        if let Some(tn) = ucs.get(t) {
                            u.extends.push(tn.clone());
                        }
                    }
                    _ => {}
                }
            }
            out.use_cases.push(u);
        }
        let ao = |n: &str| m.actors.iter().position(|a| a.name == n).unwrap_or(usize::MAX);
        let uo = |n: &str| m.use_cases.iter().position(|u| u.name == n).unwrap_or(usize::MAX);
        out.actors.sort_by_key(|a| ao(&a.name));
        out.use_cases.sort_by_key(|u| uo(&u.name));
        // keep the current order of links inside a use case
        for u in &mut out.use_cases {
            if let Some(old) = m.use_cases.iter().find(|x| x.name == u.name) {
                let pos = |v: &Vec<String>, n: &str| v.iter().position(|x| x == n).unwrap_or(usize::MAX);
                u.actors.sort_by_key(|a| pos(&old.actors, a));
                u.includes.sort_by_key(|a| pos(&old.includes, a));
                u.extends.sort_by_key(|a| pos(&old.extends, a));
            }
        }
        out
    };
    let visible = |x: &UseCaseModel| -> (Option<String>, Vec<Actor>, Vec<UseCase>) {
        (x.system.clone(), x.actors.iter().filter(|a| shown(a.status)).cloned().collect(), x.use_cases.iter().filter(|u| shown(u.status)).cloned().collect())
    };
    let merged = build(&actors, &ucs, &links);
    if visible(&merged) != visible(m) {
        out.changes.push(Op::SetUseCases { model: merged });
    }
    if !(k1.is_empty() && k2.is_empty() && k3.is_empty()) {
        out.conflicts.push(Op::SetUseCases { model: build(&apply_conflicts(&actors, &k1), &apply_conflicts(&ucs, &k2), &apply_conflicts(&links, &k3)) });
    }
    out
}

// ------------------------------------------------------------------ packages

fn package_pull(current: &Design, base_items: &[Item], remote_items: &[Item]) -> Pull {
    let mut out = Pull::default();
    let names = |items: &[Item]| -> BTreeMap<String, String> { items.iter().filter_map(|i| if let ItemKind::Package { name, .. } = &i.kind { Some((item_key(i), name.trim().to_string())) } else { None }).collect() };
    let (b, r) = (names(base_items), names(remote_items));
    let c: BTreeMap<String, String> = current.packages.iter().filter(|p| p.status.is_live()).map(|p| (format!("package:{}", p.name), p.name.clone())).collect();
    let mut k = BTreeMap::new();
    let merged = merge3(&c, &b, &r, &mut k, &mut out.warnings, "package");
    let mut ops = vec![];
    for (key, name) in &merged {
        if !c.values().any(|n| n == name) {
            ops.push(Op::AddPackage { name: name.clone(), layer: crate::ops::guess_layer(name), description: None, external: false, system: None });
        }
        let _ = key;
    }
    for (key, name) in &c {
        if !merged.contains_key(key) && !merged.values().any(|n| n == name) {
            ops.push(Op::RemovePackage { name: name.clone() });
        }
    }
    let ops = keep_applicable(current, ops, &mut out.warnings);
    let (removals, changes): (Vec<Op>, Vec<Op>) = ops.into_iter().partition(is_removal);
    out.changes = changes;
    out.removals = removals;
    out
}

// ------------------------------------------------------------------ C4

#[derive(Debug, Clone, PartialEq)]
enum C4El {
    Container { name: String, technology: Option<String>, description: Option<String>, datastore: bool },
    System { name: String, description: Option<String> },
    Person { name: String },
    Ours { name: String },
}

impl C4El {
    fn name(&self) -> &str {
        match self {
            C4El::Container { name, .. } | C4El::System { name, .. } | C4El::Person { name } | C4El::Ours { name } => name,
        }
    }
}

/// A runtime link: (from, to, interaction, protocol).
type C4Link = (String, String, Interaction, Option<String>);

fn c4_proj(items: &[Item], system: &str) -> (BTreeMap<String, C4El>, BTreeMap<String, C4Link>) {
    let mut els = BTreeMap::new();
    for i in items {
        if let ItemKind::Block { shape, text } = &i.kind {
            let (name, tech, desc) = ut::parse_c4_text(text);
            let el = match shape {
                _ if name == system => C4El::Ours { name },
                BlockShape::Person => C4El::Person { name },
                BlockShape::Container => C4El::Container { name, technology: tech, description: desc, datastore: false },
                BlockShape::Datastore => C4El::Container { name, technology: tech, description: desc, datastore: true },
                BlockShape::System | BlockShape::ExternalSystem => C4El::System { name, description: desc },
            };
            els.insert(item_key(i), el);
        }
    }
    let mut links = BTreeMap::new();
    for i in items {
        if let ItemKind::Line { from, to, label } = &i.kind {
            let key = item_key(i);
            // derived lines (class dependencies, context edges) aren't edited from Lucid
            if key.starts_with("cdep:") || key.starts_with("ctx:") {
                continue;
            }
            if let (Some(f), Some(t)) = (els.get(from), els.get(to)) {
                let (inter, proto) = ut::parse_relation_label(label);
                links.insert(key, (f.name().to_string(), t.name().to_string(), inter, proto));
            }
        }
    }
    (els, links)
}

fn c4_pull(current: &Design, base_items: &[Item], remote_items: &[Item]) -> Pull {
    let mut out = Pull::default();
    let (be, bl) = c4_proj(base_items, &current.name);
    let (re, rl) = c4_proj(remote_items, &current.name);
    let shown = |s: Status| s.is_live();
    let mut ce: BTreeMap<String, C4El> = BTreeMap::new();
    for k in current.containers.iter().filter(|k| shown(k.status)) {
        ce.insert(format!("c4:{}", k.name), C4El::Container { name: k.name.clone(), technology: k.technology.clone().filter(|t| !t.trim().is_empty()), description: k.description.clone().filter(|t| !t.trim().is_empty()), datastore: k.datastore });
    }
    for x in current.systems.iter().filter(|x| shown(x.status)) {
        ce.insert(format!("c4:{}", x.name), C4El::System { name: x.name.clone(), description: x.description.clone().filter(|t| !t.trim().is_empty()) });
    }
    for a in current.use_cases.actors.iter().filter(|a| shown(a.status)) {
        ce.insert(format!("c4:{}", a.name), C4El::Person { name: a.name.clone() });
    }
    ce.insert(format!("c4:{}", current.name), C4El::Ours { name: current.name.clone() });
    // only what the base or remote diagram shows can have changed there
    let cl: BTreeMap<String, C4Link> = current.links.iter().filter(|l| shown(l.status)).map(|l| (format!("link:{}", l.id()), (l.from.clone(), l.to.clone(), l.interaction, l.protocol.clone().filter(|p| !p.is_empty())))).collect();
    let (mut k1, mut k2) = (BTreeMap::new(), BTreeMap::new());
    let els = merge3(&ce, &be, &re, &mut k1, &mut out.warnings, "element");
    let links = merge3(&cl, &bl, &rl, &mut k2, &mut out.warnings, "link");
    let build = |els: &BTreeMap<String, C4El>, links: &BTreeMap<String, C4Link>| -> Design {
        let mut d = current.clone();
        let mut containers: Vec<Container> = d.containers.iter().filter(|k| !shown(k.status)).cloned().collect();
        let mut systems: Vec<ExternalSystem> = d.systems.iter().filter(|x| !shown(x.status)).cloned().collect();
        for el in els.values() {
            match el {
                C4El::Container { name, technology, description, datastore } => {
                    let old = current.containers.iter().find(|k| &k.name == name);
                    containers.push(Container { name: name.clone(), technology: technology.clone(), description: description.clone(), datastore: *datastore, ..old.cloned().unwrap_or_default() });
                }
                C4El::System { name, description } => {
                    let old = current.systems.iter().find(|x| &x.name == name);
                    systems.push(ExternalSystem { name: name.clone(), description: description.clone(), ..old.cloned().unwrap_or_default() });
                }
                C4El::Person { .. } | C4El::Ours { .. } => {}
            }
        }
        let order = |n: &str| current.containers.iter().position(|k| k.name == n).unwrap_or(usize::MAX);
        containers.sort_by_key(|k| order(&k.name));
        d.containers = containers;
        d.systems = systems;
        let mut ls: Vec<Link> = d.links.iter().filter(|l| !shown(l.status)).cloned().collect();
        for (f, t, i, p) in links.values() {
            let old = current.links.iter().find(|l| &l.from == f && &l.to == t);
            ls.push(Link { from: f.clone(), to: t.clone(), interaction: *i, protocol: p.clone(), ..old.cloned().unwrap_or_default() });
        }
        d.links = ls;
        d
    };
    let merged = build(&els, &links);
    let mut ops = crate::diff::diff(current, &merged);
    ops.retain(|o| matches!(o, Op::AddContainer { .. } | Op::UpdateContainer { .. } | Op::RemoveContainer { .. } | Op::AddSystem { .. } | Op::RemoveSystem { .. } | Op::AddLink { .. } | Op::RemoveLink { .. }));
    // new persons are use case actors
    let new_people: Vec<String> = els.values().filter_map(|e| if let C4El::Person { name } = e { Some(name.clone()) } else { None }).filter(|n| !current.use_cases.actors.iter().any(|a| &a.name == n)).collect();
    if !new_people.is_empty() {
        let mut m = current.use_cases.clone();
        for n in new_people {
            m.actors.push(Actor { name: n, ..Default::default() });
        }
        ops.insert(0, Op::SetUseCases { model: m });
    }
    let ops = keep_applicable(current, ops, &mut out.warnings);
    let (removals, changes): (Vec<Op>, Vec<Op>) = ops.into_iter().partition(is_removal);
    out.changes = changes;
    out.removals = removals;
    if !(k1.is_empty() && k2.is_empty()) {
        let with = build(&apply_conflicts(&els, &k1), &apply_conflicts(&links, &k2));
        let mut w = vec![];
        out.conflicts = keep_applicable(&merged, crate::diff::diff(&merged, &with), &mut w);
        out.warnings.extend(w);
    }
    out
}

// ------------------------------------------------------------------ sequences

fn markup_of(items: &[Item]) -> Option<&str> {
    items.iter().find_map(|i| if let ItemKind::Sequence { markup } = &i.kind { Some(markup.as_str()) } else { None })
}

fn sequence_pull(current: &Design, name: &str, base_items: &[Item], remote_items: &[Item]) -> Pull {
    let mut out = Pull::default();
    let (Some(b), Some(r)) = (markup_of(base_items), markup_of(remote_items)) else {
        if markup_of(remote_items).is_none() {
            out.warnings.push(format!("the Lucid document has no sequence diagram for \"{name}\""));
        }
        return out;
    };
    if b == r {
        return out;
    }
    let parsed = match seq_markup::from_markup(name, r) {
        Ok(s) => s,
        Err(e) => {
            out.warnings.push(format!("sequence \"{name}\": {e}"));
            return out;
        }
    };
    let cur = current.sequences.iter().find(|s| s.name == name);
    let parsed_base = seq_markup::from_markup(name, b).ok();
    let mut s = parsed.clone();
    if let Some(c) = cur {
        s.use_case = c.use_case.clone();
        s.generated_rev = c.generated_rev;
        s.feedback = c.feedback.clone();
        // keep message feedback where the message is unchanged
        for m in &mut s.messages {
            if let Some(x) = c.messages.iter().find(|x| x.from == m.from && x.to == m.to && x.method == m.method && x.kind == m.kind) {
                m.feedback = x.feedback.clone();
            }
        }
    }
    let visible = |x: &Sequence| (x.participants.clone(), x.lifelines.clone(), x.messages.iter().map(|m| Message { problem: None, feedback: None, ..m.clone() }).collect::<Vec<_>>(), x.fragments.clone());
    let local_changed = match (cur, &parsed_base) {
        (Some(c), Some(pb)) => visible(c) != visible(pb),
        _ => false,
    };
    let op = Op::SetSequence { sequence: s };
    if local_changed {
        out.conflicts.push(op);
    } else {
        out.changes.push(op);
    }
    out
}

/// File a pull as ghost proposals (source `lucid`), like a branch merge: additions and changes,
/// removals, conflicts (Lucid's version) and moved boxes separately, so each can be accepted alone.
/// Returns the proposal ids.
pub fn propose(store: &mut crate::Store, title: &str, p: &Pull) -> Result<Vec<String>, crate::OpError> {
    let mut ids = vec![];
    let groups: [(&Vec<Op>, String); 4] = [
        (&p.changes, format!("From Lucid ({title}): additions and changes made by colleagues")),
        (&p.removals, format!("From Lucid ({title}): removals made by colleagues")),
        (&p.conflicts, format!("From Lucid ({title}): Lucid's version of elements that were also changed here since the last sync — accept to take Lucid's, reject to keep yours")),
        (&p.layout, format!("From Lucid ({title}): boxes moved by colleagues")),
    ];
    for (ops, why) in groups {
        if !ops.is_empty() {
            ids.push(store.propose_ops(ops.clone(), Some(why), "lucid")?);
        }
    }
    Ok(ids)
}

/// The document's comment threads and colleagues' notes as Spide comments (to upsert).
pub fn comments(key: &str, remote: &Snapshot, notes: &[Note]) -> Vec<Comment> {
    let mut out = vec![];
    for t in &remote.threads {
        let target = t.sid.clone().or_else(|| nearest(&remote.items, t.bbox.or_else(|| t.item.as_deref().and_then(|i| remote.by_lucid(i)).and_then(|i| i.bbox))));
        out.push(Comment {
            source: "lucid".into(),
            diagram: key.into(),
            thread: t.id.clone(),
            target,
            entries: t.comments.iter().map(|c| CommentEntry { author: c.author.clone(), text: c.text.clone(), at: c.at.clone() }).collect(),
            resolved: t.resolved,
            ..Default::default()
        });
    }
    for n in notes {
        out.push(Comment {
            source: "lucid".into(),
            diagram: key.into(),
            thread: format!("note:{}", n.lucid_id),
            target: n.near.clone(),
            entries: vec![CommentEntry { author: "note on the diagram".into(), text: n.text.clone(), at: String::new() }],
            ..Default::default()
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lucid::snapshot::{Comment as LComment, Thread};

    #[test]
    fn threads_and_notes_become_comments_anchored_to_elements() {
        let remote = Snapshot {
            items: vec![
                Item { sid: Some("class:Order".into()), lucid_id: Some("L1".into()), kind: ItemKind::Class { name: "Order".into(), attributes: vec![], operations: vec![], interface: false }, bbox: Some(Bbox { x: 0.0, y: 0.0, w: 100.0, h: 80.0 }), fill: None },
                Item { sid: None, lucid_id: Some("L2".into()), kind: ItemKind::Other { class_name: "Text".into(), text: "why?".into() }, bbox: Some(Bbox { x: 120.0, y: 0.0, w: 60.0, h: 30.0 }), fill: None },
            ],
            threads: vec![
                Thread { id: "T1".into(), sid: Some("class:Order".into()), item: Some("L1".into()), resolved: false, bbox: None, comments: vec![LComment { id: "c".into(), author: "Ana".into(), text: "Split Order?".into(), at: "2026-11-17".into() }] },
                Thread { id: "T2".into(), sid: None, item: Some("L2".into()), resolved: true, bbox: None, comments: vec![] },
            ],
            ..Default::default()
        };
        let notes = notes_of(&remote);
        let cs = comments("class", &remote, &notes);
        assert_eq!(cs.len(), 3);
        assert_eq!((cs[0].target.as_deref(), cs[0].entries[0].author.as_str()), (Some("class:Order"), "Ana"));
        assert_eq!(cs[1].target.as_deref(), Some("class:Order"), "a thread on a colleague's shape is about the nearest element");
        assert!(cs[1].resolved);
        assert_eq!((cs[2].thread.as_str(), cs[2].target.as_deref()), ("note:L2", Some("class:Order")));
        // upserting twice keeps one comment per thread, and the agent's link
        let mut store = crate::Store::default();
        assert_eq!(store.upsert_comments(cs.clone()).unwrap(), 3);
        let id = store.design.comments[0].id.clone();
        store.design.proposals.push(crate::model::Proposal { id: "P9".into(), ..Default::default() });
        store.address_comment(&id, "P9").unwrap();
        assert_eq!(store.upsert_comments(cs.clone()).unwrap(), 0);
        let mut again = cs;
        again[0].entries.push(CommentEntry { author: "Bo".into(), text: "+1".into(), at: String::new() });
        assert_eq!(store.upsert_comments(again).unwrap(), 1);
        assert_eq!(store.design.comments.len(), 3);
        assert_eq!(store.design.comments[0].addressed_by.as_deref(), Some("P9"));
        assert_eq!(store.design.comments[0].entries.len(), 2);
        // the agent sees open ones in get_design
        let digest: serde_json::Value = serde_json::from_str(&crate::tools::design_digest(&store)).unwrap();
        assert_eq!(digest["open_comments"].as_array().unwrap().len(), 1, "resolved and addressed ones are not open");
    }
}
