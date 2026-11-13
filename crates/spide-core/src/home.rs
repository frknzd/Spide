//! Suggested home for a misplaced or duplicated method: rank the classes that could own it by
//! Information Expert (who has the data), layer legality, simulated impact (health, coupling,
//! cohesion) and role fit — plus a new Pure Fabrication class when no existing class fits.
//! Every candidate carries the ops that implement it, so the agent can turn the ranking straight
//! into decision options.

use crate::metrics::compute;
use crate::model::*;
use crate::ops::{apply_all, Mode, Op};
use crate::responsibility::{self, words};
use crate::rules;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HomeCandidate {
    pub class: String,
    /// keep | move | new
    pub kind: String,
    pub package: Option<String>,
    /// 0..1 share of the data the method(s) touch that this class owns (Information Expert)
    pub expert: f64,
    /// every caller may depend on it and it may depend on what the method needs (architecture rules)
    pub legal: bool,
    pub illegal_because: Vec<String>,
    pub health_before: u8,
    pub health_after: u8,
    /// Fan-out change of the class(es) the method leaves.
    pub source_fan_out_delta: i64,
    /// LCOM4 of the receiving class afterwards (1 = cohesive).
    pub lcom_after: Option<usize>,
    pub role_fit: bool,
    /// Ranking score (higher is better).
    pub score: f64,
    pub why: String,
    pub ops: Vec<Op>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HomeSuggestion {
    /// `Class.method` being placed
    pub methods: Vec<String>,
    pub candidates: Vec<HomeCandidate>,
    /// Name of the best candidate (a class, or the new class).
    pub recommended: String,
}

fn split(s: &str) -> Result<(String, String), String> {
    s.split_once('.').map(|(a, b)| (a.to_string(), b.trim_end_matches("()").to_string())).ok_or(format!("expected Class.method, got {s:?}"))
}

fn layer(d: &Design, class: &str) -> Option<Layer> {
    d.class(class).and_then(|c| c.package.as_deref()).and_then(|p| d.packages.iter().find(|x| x.name == p)).and_then(|p| p.layer)
}

/// Why `from` may not depend on `to` (layer rules and custom forbidden dependencies), if it may not.
fn forbidden(d: &Design, from: &str, from_pkg: Option<&str>, from_layer: Option<Layer>, to: &str, to_pkg: Option<&str>, to_layer: Option<Layer>) -> Option<String> {
    if from == to {
        return None;
    }
    if let (Some(a), Some(b)) = (from_layer, to_layer) {
        if let Err(v) = rules::allowed(a, b, d.rules.preset) {
            return Some(format!("{from} → {to}: {}", v.explain));
        }
    }
    rules::forbidden_by(&d.rules, from, from_pkg, to, to_pkg).next().map(|r| format!("{from} → {to}: forbidden by {}", r.id()))
}

/// Design class returned by `Class.method` (`List<X>` → X), if any.
fn returned_class(d: &Design, call: &str) -> Option<String> {
    let (cls, meth) = call.split_once('.')?;
    let r = d.class(cls)?.methods.iter().find(|m| m.name == meth)?.returns.clone()?;
    let found = crate::detail::type_names(&r).find(|t| d.class(t).is_some() && *t != cls).map(String::from);
    found
}

const COLLECTION: [&str; 6] = ["List", "Set", "Collection", "Iterable", "Map", "Queue"];

/// Points per class for the data a method touches (Information Expert):
/// - getter calls into a class count double, other calls once;
/// - a call that only *provides* another object (`getReport(): ApiReport`) credits the provided class;
/// - a whole that holds or hands out parts (`lines: List<OrderLine>`, `getEndpoints(): List<…>`) takes
///   over its parts' points: it does the work by asking its parts (partial experts);
/// - parameters of a class's type, a collection parameter → the classes that keep such a collection;
/// - the concept it owns (attribute/responsibility claims).
fn data_points(d: &Design, owner: &Class, m: &Method, claims: &[crate::overlap::Claim]) -> BTreeMap<String, f64> {
    let mut pts: BTreeMap<String, f64> = BTreeMap::new();
    let real = |c: &str| d.class(c).is_some() && !d.is_external(c);
    let mut parts_of: Vec<(String, String)> = vec![]; // (whole, part)
    for call in &m.calls {
        match call.split_once('.') {
            Some((cls, meth)) if cls != owner.name && cls != "this" && real(cls) => {
                if let Some(t) = returned_class(d, call).filter(|t| real(t)) {
                    let collection = d.class(cls).and_then(|c| c.methods.iter().find(|x| x.name == meth)).and_then(|x| x.returns.as_deref()).is_some_and(|r| COLLECTION.iter().any(|k| r.contains(k)));
                    if collection || m.calls.iter().any(|c| c.starts_with(&format!("{t}."))) {
                        // a provider: the provided object is what the work is about
                        *pts.entry(t.clone()).or_default() += 1.5;
                        if collection {
                            parts_of.push((cls.to_string(), t));
                            *pts.entry(cls.to_string()).or_default() += 1.0;
                        }
                        continue;
                    }
                }
                let getter = matches!(words(meth).first().map(String::as_str), Some("get" | "is" | "has" | "size" | "part" | "count"));
                *pts.entry(cls.to_string()).or_default() += if getter { 2.0 } else { 1.0 };
            }
            _ => *pts.entry(owner.name.clone()).or_default() += 0.5,
        }
    }
    *pts.entry(owner.name.clone()).or_default() += m.uses.len() as f64;
    // a collection it keeps (lines: List<OrderLine>) makes the owner the whole of those parts
    for u in &m.uses {
        if let Some(a) = owner.attributes.iter().find(|a| &a.name == u && COLLECTION.iter().any(|k| a.ty.contains(k))) {
            for t in crate::detail::type_names(&a.ty).filter(|t| real(t)) {
                parts_of.push((owner.name.clone(), t.to_string()));
            }
        }
    }
    for p in &m.params {
        let collection = COLLECTION.iter().any(|k| p.ty.contains(k));
        for t in crate::detail::type_names(&p.ty) {
            if t == owner.name || !real(t) {
                continue;
            }
            *pts.entry(t.to_string()).or_default() += 1.0;
            if collection {
                // who keeps a collection of these (records/contains them)?
                for k in d.classes.iter().filter(|k| k.status.is_live() && !d.is_external(&k.name) && k.name != t) {
                    if k.attributes.iter().any(|a| COLLECTION.iter().any(|c| a.ty.contains(c)) && crate::detail::type_names(&a.ty).any(|x| x == t)) {
                        *pts.entry(k.name.clone()).or_default() += 2.0;
                    }
                }
            }
        }
    }
    let key = format!("{}.{}", owner.name, m.name);
    for c in claims.iter().filter(|c| c.method == key) {
        *pts.entry(c.owner.clone()).or_default() += 2.5;
    }
    // work over a collection of parts belongs to the whole; each part is only a partial expert
    // (Sale.getTotal asks every SalesLineItem.getSubtotal)
    let parts_of: BTreeSet<(String, String)> = parts_of.into_iter().collect();
    for (whole, part) in &parts_of {
        let p = pts.get(part).copied().unwrap_or(0.0);
        *pts.entry(whole.clone()).or_default() += p;
        if let Some(x) = pts.get_mut(part) {
            *x *= 0.3;
        }
    }
    pts
}

/// The method as it would look in `home`: calls into home become local, a parameter of home's
/// type becomes `this`, attributes of the old owner it used become parameters.
fn moved_method(d: &Design, owner: &Class, m: &Method, home: &str) -> Method {
    let mut nm = m.clone();
    nm.status = Status::Accepted;
    nm.proposal = None;
    nm.origin = None;
    nm.concern = None;
    let home_class = d.class(home);
    let provider: Vec<&String> = m.calls.iter().filter(|c| returned_class(d, c).as_deref() == Some(home)).collect();
    if !provider.is_empty() {
        // the caller obtains `home` (e.g. extractor.getReport(ns)) and asks it; params only the provider needed go
        let provider_params: BTreeSet<String> = provider
            .iter()
            .filter_map(|c| c.split_once('.'))
            .filter_map(|(cls, meth)| d.class(cls).and_then(|k| k.methods.iter().find(|x| x.name == meth)))
            .flat_map(|x| x.params.iter().map(|p| p.ty.clone()))
            .collect();
        nm.params.retain(|p| !provider_params.contains(&p.ty));
    }
    nm.calls = m
        .calls
        .iter()
        .filter(|c| !provider.contains(c))
        .map(|c| match c.split_once('.') {
            Some((cls, meth)) if cls == home => meth.to_string(),
            Some(_) => c.clone(),
            None => format!("{}.{c}", owner.name),
        })
        .collect();
    nm.params.retain(|p| crate::detail::type_names(&p.ty).next() != Some(home) || p.ty.contains('<'));
    // what it read from the old owner now has to be passed in (except the object it only used to
    // obtain `home`: that call stays at the call site)
    let provider_types: BTreeSet<&str> = provider.iter().filter_map(|c| c.split_once('.').map(|(cls, _)| cls)).collect();
    for u in &m.uses {
        if let Some(a) = owner.attributes.iter().find(|a| &a.name == u && !crate::detail::type_names(&a.ty).any(|t| provider_types.contains(t))) {
            if !nm.params.iter().any(|p| &p.name == u) {
                nm.params.push(Param { name: a.name.clone(), ty: a.ty.clone() });
            }
        }
    }
    // getters it called on home read home's own attributes now
    let mut uses: Vec<String> = home_class
        .map(|h| {
            h.attributes
                .iter()
                .filter(|a| m.calls.iter().any(|c| c.split_once('.').is_some_and(|(cls, meth)| cls == home && meth.to_lowercase().ends_with(&a.name.to_lowercase()))))
                .map(|a| a.name.clone())
                .collect()
        })
        .unwrap_or_default();
    // a parameter home already keeps as an attribute (same type) becomes that attribute
    if let Some(h) = home_class {
        let norm = |t: &str| t.replace(' ', "");
        nm.params.retain(|p| match h.attributes.iter().find(|a| norm(&a.ty) == norm(&p.ty) && a.ty.contains('<')) {
            Some(a) => {
                uses.push(a.name.clone());
                false
            }
            None => true,
        });
    }
    uses.sort();
    uses.dedup();
    // a pure function of its parameters becomes a static helper of its new home
    let local_calls = nm.calls.iter().any(|c| !c.contains('.'));
    if uses.is_empty() && !local_calls && !m.is_abstract {
        nm.is_static = true;
    }
    nm.uses = uses;
    nm
}

/// Ops that move `methods` (all collapse into one implementation named after the first) into `home`.
fn move_ops(d: &Design, methods: &[(String, String)], home: &str, new_class: Option<Op>) -> Vec<Op> {
    let mut ops: Vec<Op> = new_class.into_iter().collect();
    let (c0, m0) = &methods[0];
    let owner0 = d.class(c0).unwrap();
    let meth0 = owner0.methods.iter().find(|m| &m.name == m0).unwrap();
    let nm = moved_method(d, owner0, meth0, home);
    let home_has = d.class(home).is_some_and(|h| h.methods.iter().any(|m| m.name == nm.name && m.status.is_live()));
    if !home_has {
        ops.push(Op::AddMethod { class: home.to_string(), method: nm.clone() });
    }
    for (c, m) in methods {
        if c != home {
            ops.push(Op::RemoveMethod { class: c.clone(), name: m.clone() });
        }
    }
    // rewire callers: `C.m` and local `m` (in the old owner) now go to `home.<name>`; calls that only
    // obtained `home` move to the call site
    let new_ref = format!("{home}.{}", nm.name);
    let provider: Vec<String> = meth0.calls.iter().filter(|c| returned_class(d, c).as_deref() == Some(home)).cloned().collect();
    for cls in d.classes.iter().filter(|c| c.status.is_live()) {
        for x in cls.methods.iter().filter(|x| x.status.is_live()) {
            if methods.iter().any(|(c, m)| c == &cls.name && m == &x.name) {
                continue;
            }
            let mut changed = false;
            let calls: Vec<String> = x
                .calls
                .iter()
                .map(|call| {
                    let hit = methods.iter().any(|(c, m)| {
                        call == &format!("{c}.{m}") || (cls.name == *c && (call == m || call == &format!("this.{m}")))
                    });
                    if hit {
                        changed = true;
                        if cls.name == home { nm.name.clone() } else { new_ref.clone() }
                    } else {
                        call.clone()
                    }
                })
                .collect();
            if changed {
                let mut upd = x.clone();
                let mut calls = calls;
                if cls.name != home {
                    calls.extend(provider.iter().cloned());
                }
                upd.calls = calls.into_iter().collect::<BTreeSet<_>>().into_iter().collect();
                upd.status = Status::Accepted;
                upd.proposal = None;
                ops.push(Op::UpdateMethod { class: cls.name.clone(), name: x.name.clone(), method: upd });
            }
        }
    }
    ops
}

/// A name for a new Pure Fabrication class from the method's own words.
fn fabrication_name(d: &Design, m: &str, concern: &str) -> String {
    let ws = words(m);
    let skip = ["get", "is", "has", "without", "with", "of", "to", "from", "for", "by", "on", "do", "calc", "compute", "calculate", "count", "find", "format", "write", "check", "validate", "match", "matches"];
    let mut concept: Vec<String> = ws.iter().filter(|w| !skip.contains(&w.as_str())).map(|w| {
        let mut c = w.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
    }).collect();
    if concept.is_empty() {
        concept.push("Shared".into());
    }
    let verb = ws.first().map(|w| responsibility::stem(w)).unwrap_or_default();
    let suffix = match (concern, verb.as_str()) {
        (_, "count" | "comput" | "calculat" | "total" | "sum") => "Calculator",
        (_, "match" | "filter") => "Matcher",
        (_, "check" | "validat" | "verify") => "Validator",
        ("formatting", _) | (_, "format" | "write") => "Formatter",
        ("persistence", _) => "Store",
        ("presentation", _) => "View",
        _ => "Rules",
    };
    let base = format!("{}{suffix}", concept.join(""));
    let mut name = base.clone();
    let mut i = 2;
    while d.class(&name).is_some() {
        name = format!("{base}{i}");
        i += 1;
    }
    name
}

/// Rank homes for one method (`["A.m"]`) or a group of duplicates (`["A.m", "B.n"]`).
pub fn suggest_home(d: &Design, methods: &[String]) -> Result<HomeSuggestion, String> {
    if methods.is_empty() {
        return Err("give at least one Class.method".into());
    }
    let ms: Vec<(String, String)> = methods.iter().map(|s| split(s)).collect::<Result<_, _>>()?;
    for (c, m) in &ms {
        let cls = d.class(c).filter(|c| c.status.is_live()).ok_or(format!("unknown class {c}"))?;
        cls.methods.iter().find(|x| &x.name == m && x.status.is_live()).ok_or(format!("unknown method {c}.{m}()"))?;
    }
    let base = crate::impact::committed(d);
    let before = compute(&base, false);
    let ov = &before.overlap;
    let resp = &before.responsibility;

    // data each candidate owns, summed over the group
    let mut pts: BTreeMap<String, f64> = BTreeMap::new();
    for (c, m) in &ms {
        let owner = d.class(c).unwrap();
        let meth = owner.methods.iter().find(|x| &x.name == m).unwrap();
        for (k, v) in data_points(d, owner, meth, &ov.claims) {
            *pts.entry(k).or_default() += v;
        }
    }
    let total: f64 = pts.values().sum::<f64>().max(1.0);
    let sources: BTreeSet<&String> = ms.iter().map(|(c, _)| c).collect();
    // who calls the method(s) (they must be allowed to depend on the new home)
    let mut callers: BTreeSet<String> = BTreeSet::new();
    for cls in d.classes.iter().filter(|c| c.status.is_live()) {
        for x in cls.methods.iter().filter(|x| x.status.is_live()) {
            if ms.iter().any(|(c, m)| x.calls.iter().any(|call| call == &format!("{c}.{m}") || (cls.name == *c && call == m))) {
                callers.insert(cls.name.clone());
            }
        }
    }
    // what the method needs (the new home must be allowed to depend on it)
    let mut needs: BTreeSet<String> = BTreeSet::new();
    for (c, m) in &ms {
        let meth = d.class(c).unwrap().methods.iter().find(|x| &x.name == m).unwrap();
        for call in &meth.calls {
            if let Some((cls, _)) = call.split_once('.') {
                if d.class(cls).is_some() {
                    needs.insert(cls.to_string());
                }
            }
        }
        for p in &meth.params {
            needs.extend(crate::detail::type_names(&p.ty).filter(|t| d.class(t).is_some()).map(String::from));
        }
    }
    let concern = ms
        .first()
        .and_then(|(c, m)| resp.method(c, m))
        .map(|mi| mi.concern.clone())
        .unwrap_or_else(|| "domain_logic".into());
    let src_fan_out = |m: &crate::metrics::Metrics| -> i64 { sources.iter().map(|s| m.classes.get(*s).map_or(0, |x| x.fan_out) as i64).sum() };

    let legality = |home: &str, pkg: Option<&str>, lay: Option<Layer>| -> Vec<String> {
        let mut why = vec![];
        for c in callers.iter().filter(|c| c.as_str() != home) {
            let cp = d.class(c).and_then(|x| x.package.clone());
            if let Some(w) = forbidden(d, c, cp.as_deref(), layer(d, c), home, pkg, lay) {
                why.push(w);
            }
        }
        for n in needs.iter().filter(|n| n.as_str() != home) {
            let np = d.class(n).and_then(|x| x.package.clone());
            if let Some(w) = forbidden(d, home, pkg, lay, n, np.as_deref(), layer(d, n)) {
                why.push(w);
            }
        }
        why
    };
    let evaluate = |home: &str, kind: &str, pkg: Option<String>, ops: Vec<Op>, expert: f64, role_fit: bool, why: String| -> HomeCandidate {
        let lay = pkg.as_deref().and_then(|p| d.packages.iter().find(|x| x.name == p)).and_then(|p| p.layer);
        let illegal = if kind == "keep" { vec![] } else { legality(home, pkg.as_deref(), lay) };
        let after = if ops.is_empty() { None } else { apply_all(&base, &ops, Mode::Commit(Status::Accepted, None)).ok().map(|a| compute(&a, false)) };
        let (h_after, fo_delta, lcom) = match &after {
            Some(a) => (a.health, src_fan_out(a) - src_fan_out(&before), a.classes.get(home).and_then(|x| x.lcom)),
            None => (before.health, 0, before.classes.get(home).and_then(|x| x.lcom)),
        };
        let gain = (h_after as f64 - before.health as f64) / 20.0;
        let mut score = 0.45 * expert + 0.25 * gain.clamp(-1.0, 1.0) + if role_fit { 0.15 } else { 0.0 } + if lcom.unwrap_or(1) <= 1 { 0.15 } else { 0.0 };
        if !illegal.is_empty() {
            score -= 1.0;
        }
        HomeCandidate {
            class: home.to_string(),
            kind: kind.to_string(),
            package: pkg,
            expert: (expert * 100.0).round() / 100.0,
            legal: illegal.is_empty(),
            illegal_because: illegal,
            health_before: before.health,
            health_after: h_after,
            source_fan_out_delta: fo_delta,
            lcom_after: lcom,
            role_fit,
            score: (score * 100.0).round() / 100.0,
            why,
            ops,
        }
    };

    let mut cands: Vec<HomeCandidate> = vec![];
    // keep it where it is (reference point; only meaningful for a single method)
    if ms.len() == 1 {
        let (c, _) = &ms[0];
        let own = pts.get(c).copied().unwrap_or(0.0) / total;
        let role = resp.classes.get(c).and_then(|x| x.role.as_ref()).map(|r| r.role.clone()).unwrap_or_default();
        let fit = responsibility::misfit(&role, &concern).is_none();
        cands.push(evaluate(c, "keep", d.class(c).and_then(|x| x.package.clone()), vec![], own, fit, format!("stays in {c} ({:.0}% of the data it uses is {c}'s)", own * 100.0)));
    }
    // existing classes that own some of the data
    for (h, p) in &pts {
        if sources.contains(h) && ms.len() == 1 {
            continue;
        }
        let Some(hc) = d.class(h).filter(|c| c.status.is_live() && !d.is_external(&c.name)) else { continue };
        let expert = p / total;
        let role = resp.classes.get(h).and_then(|x| x.role.as_ref()).map(|r| r.role.clone()).unwrap_or_default();
        let fit = responsibility::misfit(&role, &concern).is_none();
        let ops = move_ops(d, &ms, h, None);
        cands.push(evaluate(h, "move", hc.package.clone(), ops, expert, fit, format!("{h} owns {:.0}% of the data it uses (Information Expert)", expert * 100.0)));
    }
    // a new Pure Fabrication in the innermost layer every caller may depend on
    let name = fabrication_name(d, &ms[0].1, &concern);
    let order = [Layer::Domain, Layer::Application, Layer::Infrastructure, Layer::Ui];
    let pkg = order
        .iter()
        .filter_map(|l| d.packages.iter().find(|p| p.layer == Some(*l) && p.status.is_live() && !p.external))
        .find(|p| legality(&name, Some(&p.name), p.layer).is_empty())
        .map(|p| p.name.clone())
        .or_else(|| d.class(&ms[0].0).and_then(|c| c.package.clone()));
    let role = match concern.as_str() {
        "formatting" => "output",
        "persistence" => "persistence",
        "presentation" => "presentation",
        _ => "service",
    };
    let add = Op::AddClass {
        name: name.clone(),
        stereotype: Some("service".into()),
        package: pkg.clone(),
        is_abstract: false,
        attributes: vec![],
        methods: vec![],
        responsibilities: vec![format!("the one place for {}", words(&ms[0].1).join(" "))],
        rationale: Some(format!("Pure Fabrication: shared home for {}", methods.join(", "))),
        aggregate_root: false,
        external: false,
        role: Some(role.into()),
    };
    let ops = move_ops(d, &ms, &name, Some(add));
    let moves: Vec<&HomeCandidate> = cands.iter().filter(|c| c.kind == "move" && c.legal).collect();
    let best_expert = moves.iter().map(|c| c.expert).fold(0.0, f64::max);
    let expert_ok = moves.iter().any(|c| c.expert >= 0.25 && c.health_after >= c.health_before && c.lcom_after.unwrap_or(1) <= 1);
    let mut fab = evaluate(&name, "new", pkg, ops, 0.0, true, format!("new class {name} (Pure Fabrication)"));
    // the course: fabricate only when no expert can take it without hurting cohesion/coupling
    // (or no existing home is legal)
    if expert_ok {
        fab.score -= 0.1;
        fab.why = format!("new class {name} (Pure Fabrication) — not needed: an existing expert can take it");
    } else {
        fab.score += 0.3;
        fab.why = format!(
            "new class {name} (Pure Fabrication): no existing class can take it without hurting coupling or cohesion (best expert owns {:.0}% of the data)",
            best_expert * 100.0
        );
    }
    cands.push(fab);
    cands.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap().then(a.class.cmp(&b.class)));
    let recommended = cands.iter().find(|c| c.legal).map(|c| c.class.clone()).unwrap_or_else(|| cands[0].class.clone());
    Ok(HomeSuggestion { methods: methods.to_vec(), candidates: cands, recommended })
}
