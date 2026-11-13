//! Overlap analysis: duplicated behaviour across classes ("number one in the stink parade"),
//! behaviour that another class claims (its data or its stated responsibility), concepts whose
//! behaviour is scattered over many classes (shotgun surgery), and pass-through delegation.
//!
//! Scores are explainable: every method pair carries the parts of its similarity.

use crate::detail;
use crate::model::*;
use crate::responsibility::{stem, words};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// Two methods at or above this similarity are near-duplicates.
pub const DUPLICATE: f64 = 0.6;

const STOP: &[&str] = &["of", "to", "for", "the", "a", "an", "by", "in", "on", "at", "and", "or", "if", "is", "with", "from", "into", "as", "it", "its", "this", "that", "be", "one"];
const PRIMITIVES: &[&str] = &["int", "long", "double", "float", "short", "byte", "char", "boolean", "void", "string", "integer", "object"];

/// Synonyms → one canonical word, so `deleteItem` and `removeItem` meet.
fn canon(w: &str) -> String {
    let s = stem(w);
    let c = match s.as_str() {
        "delete" | "drop" | "erase" | "discard" => "remove",
        "tally" | "number" | "num" => "count",
        "fetch" | "lookup" | "retrieve" | "search" => "find",
        "make" | "build" | "construct" | "new" | "instantiate" => "create",
        "check" | "verify" | "validat" | "valid" | "validate" => "validate",
        "display" | "render" | "present" => "show",
        "print" | "output" | "emit" => "write",
        "strip" | "trim" | "without" => "strip",
        "calc" | "calculate" | "compute" => "compute",
        "amount" | "sum" => "total",
        "trail" | "trailing" => "trail",
        "parm" | "param" | "parameter" | "argument" | "arg" => "argument",
        _ => return s,
    };
    c.to_string()
}

pub fn tokens(text: &str) -> BTreeSet<String> {
    words(text).into_iter().filter(|w| !STOP.contains(&w.as_str()) && w.len() > 1).map(|w| canon(&w)).collect()
}

fn type_tokens(ty: &str) -> BTreeSet<String> {
    detail::type_names(ty).map(|t| t.to_lowercase()).filter(|t| !PRIMITIVES.contains(&t.as_str()) && !["list", "set", "map", "collection", "optional"].contains(&t.as_str())).collect()
}

fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    a.intersection(b).count() as f64 / a.union(b).count() as f64
}

/// What a method is about, as sets of words/types.
struct Features<'a> {
    class: &'a Class,
    m: &'a Method,
    name: BTreeSet<String>,
    sig: BTreeSet<String>,
    touches: BTreeSet<String>,
    text: BTreeSet<String>,
    /// The same sets as sorted token ids (fast pairwise comparison): name, sig, touches, text.
    ids: [Vec<u32>; 4],
}

fn intern(map: &mut std::collections::HashMap<String, u32>, set: &BTreeSet<String>) -> Vec<u32> {
    let mut v: Vec<u32> = set
        .iter()
        .map(|t| {
            let n = map.len() as u32;
            *map.entry(t.clone()).or_insert(n)
        })
        .collect();
    v.sort_unstable();
    v
}

fn jaccard_ids(a: &[u32], b: &[u32]) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let (mut i, mut j, mut inter) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                inter += 1;
                i += 1;
                j += 1;
            }
        }
    }
    inter as f64 / (a.len() + b.len() - inter) as f64
}

fn features<'a>(class_names: &BTreeSet<String>, c: &'a Class, m: &'a Method) -> Features<'a> {
    let mut sig: BTreeSet<String> = BTreeSet::new();
    for p in &m.params {
        sig.extend(type_tokens(&p.ty).into_iter().map(|t| format!("in:{t}")));
    }
    if let Some(r) = &m.returns {
        sig.extend(type_tokens(r).into_iter().map(|t| format!("out:{t}")));
    }
    // what it touches: classes it calls into, the types of the data it uses and receives
    let mut touches: BTreeSet<String> = BTreeSet::new();
    for call in &m.calls {
        if let Some((cls, meth)) = call.split_once('.') {
            if cls != c.name && cls != "this" {
                touches.insert(format!("{cls}.{meth}"));
                touches.insert(cls.to_lowercase());
            }
        }
    }
    for u in &m.uses {
        if let Some(a) = c.attributes.iter().find(|a| &a.name == u) {
            touches.extend(type_tokens(&a.ty));
        }
    }
    for p in &m.params {
        touches.extend(type_tokens(&p.ty).into_iter().filter(|t| class_names.contains(t)));
    }
    let text = m.description.as_deref().map(tokens).unwrap_or_default();
    Features { class: c, m, name: tokens(&m.name), sig, touches, text, ids: Default::default() }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MethodPair {
    /// `Class.method`
    pub a: String,
    pub b: String,
    pub score: f64,
    pub name: f64,
    pub signature: f64,
    pub touches: f64,
    pub text: f64,
    /// Shared name words, e.g. ["count", "call"].
    pub shared: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Delegation {
    /// `Class.method` that only forwards
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ClassOverlap {
    pub a: String,
    pub b: String,
    /// Share of A's behavioural methods with a near-duplicate in B (and vice versa).
    pub share_a: f64,
    pub share_b: f64,
    /// Overlap of their stated responsibilities (words).
    pub responsibility: f64,
    pub pairs: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Scattered {
    pub concept: String,
    pub classes: Vec<String>,
    pub methods: Vec<String>,
}

/// A method dealing with a concept another class owns: its data (attribute) or its stated responsibility.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Claim {
    pub method: String,
    pub owner: String,
    /// attribute | responsibility
    pub via: String,
    pub evidence: String,
    pub shared: Vec<String>,
    /// The method uses none of its own class's data.
    pub uses_no_own_data: bool,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct Report {
    /// Method pairs at or above the reporting threshold, best first.
    pub pairs: Vec<MethodPair>,
    pub delegations: Vec<Delegation>,
    pub classes: Vec<ClassOverlap>,
    pub scattered: Vec<Scattered>,
    pub claims: Vec<Claim>,
    /// Non-controller classes whose behaviour is mostly pass-through (Middle Man).
    pub middle_men: Vec<String>,
}

/// Is the method plain state access, construction, or an Object method (never "duplication")?
pub fn trivial(c: &Class, m: &Method) -> bool {
    m.is_abstract || detail::is_accessor(&c.name, m) || ["toString", "equals", "hashCode", "compareTo"].contains(&m.name.as_str())
}

/// Do both classes implement the same method of a supertype (polymorphism, not duplication)?
fn overrides_same(d: &Design, anc: &BTreeMap<String, Vec<String>>, a: &Class, b: &Class, name: &str) -> bool {
    let declares = |x: &String| d.class(x).is_some_and(|c| c.methods.iter().any(|m| m.name == name));
    let (aa, ab) = (anc.get(&a.name), anc.get(&b.name));
    let in_a = aa.is_some_and(|v| v.iter().any(declares)) || ab.is_some_and(|v| v.contains(&a.name));
    let in_b = ab.is_some_and(|v| v.iter().any(declares)) || aa.is_some_and(|v| v.contains(&b.name));
    in_a && in_b || ab.is_some_and(|v| v.contains(&a.name)) || aa.is_some_and(|v| v.contains(&b.name))
}

fn calls_target(m: &Method, class: &str, method: &str) -> bool {
    m.calls.iter().any(|c| c.split_once('.').is_some_and(|(cls, n)| cls == class && n == method))
}

/// A UI event handler (`onAddEndpoint`): it mirrors a domain operation by design.
fn handler(m: &Method) -> bool {
    m.name.len() > 2 && m.name.starts_with("on") && m.name[2..].starts_with(|c: char| c.is_uppercase())
}

/// The method implements/overrides one declared by a supertype (polymorphism).
fn inherited_method(d: &Design, anc: &BTreeMap<String, Vec<String>>, c: &Class, name: &str) -> Option<String> {
    anc.get(&c.name)?.iter().find(|a| d.class(a).is_some_and(|x| x.methods.iter().any(|m| m.name == name))).cloned()
}

/// A method that only forwards to one method of another class.
fn forwards(c: &Class, m: &Method) -> Option<String> {
    let external: Vec<&String> = m.calls.iter().filter(|x| x.split_once('.').is_some_and(|(cls, _)| cls != c.name && cls != "this")).collect();
    let local = m.calls.len() - external.len();
    (external.len() == 1 && local == 0 && m.uses.len() <= 1).then(|| external[0].clone())
}

fn similarity(a: &Features, b: &Features) -> MethodPair {
    let name = jaccard_ids(&a.ids[0], &b.ids[0]);
    let signature = if a.ids[1].is_empty() && b.ids[1].is_empty() { 0.5 } else { jaccard_ids(&a.ids[1], &b.ids[1]) };
    let touches = jaccard_ids(&a.ids[2], &b.ids[2]);
    let have_text = !a.ids[3].is_empty() && !b.ids[3].is_empty();
    let text = if have_text { jaccard_ids(&a.ids[3], &b.ids[3]) } else { 0.0 };
    // weights: name .35, signature .25, touches .25, text .15 (redistributed when there is no text)
    let mut score = if have_text {
        0.35 * name + 0.25 * signature + 0.25 * touches + 0.15 * text
    } else {
        (0.35 * name + 0.25 * signature + 0.25 * touches) / 0.85
    };
    // duplication needs the same intent (name) *and* the same material (data/calls/text):
    // a shared name alone is a homonym, shared data alone is two different operations on it
    if touches < 0.2 && text < 0.3 {
        score = score.min(0.45);
    }
    if name < 0.34 {
        score = score.min(0.49);
    }
    MethodPair { a: String::new(), b: String::new(), score: (score * 100.0).round() / 100.0, name, signature, touches, text, shared: vec![] }
}

/// Analyse live, non-external classes. Pairs below `min_score` aren't reported.
pub fn analyse(d: &Design, min_score: f64) -> Report {
    analyse_with(d, min_score, false)
}

pub fn analyse_with(d: &Design, min_score: f64, ghosts: bool) -> Report {
    let live = |s: Status| s.is_live() || (ghosts && s == Status::Proposed);
    let classes: Vec<&Class> = d.classes.iter().filter(|c| live(c.status) && !d.is_external(&c.name)).collect();
    let mut r = Report::default();
    let class_names: BTreeSet<String> = d.classes.iter().map(|c| c.name.to_lowercase()).collect();
    let has_inheritance = d.relations.iter().any(|r| matches!(r.kind, RelationKind::Inheritance | RelationKind::Realization));
    let anc: BTreeMap<String, Vec<String>> = if has_inheritance {
        classes.iter().map(|c| (c.name.clone(), detail::ancestors(d, &c.name))).filter(|(_, v)| !v.is_empty()).collect()
    } else {
        BTreeMap::new()
    };
    let mut feats: Vec<Features> = vec![];
    for c in &classes {
        for m in c.methods.iter().filter(|m| live(m.status) && !trivial(c, m) && !detail::is_constructor(&c.name, m)) {
            feats.push(features(&class_names, c, m));
        }
    }
    let mut ids: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for f in feats.iter_mut() {
        f.ids = [intern(&mut ids, &f.name), intern(&mut ids, &f.sig), intern(&mut ids, &f.touches), intern(&mut ids, &f.text)];
    }
    // inverted index on name words: only methods sharing a name word can score ≥ 0.5
    let mut by_word: std::collections::HashMap<u32, Vec<usize>> = std::collections::HashMap::new();
    for (i, f) in feats.iter().enumerate() {
        for w in &f.ids[0] {
            by_word.entry(*w).or_default().push(i);
        }
    }
    // delegation (pass-through), reported separately
    let mut delegating: BTreeSet<String> = BTreeSet::new();
    for f in &feats {
        if let Some(to) = forwards(f.class, f.m) {
            delegating.insert(format!("{}.{}", f.class.name, f.m.name));
            r.delegations.push(Delegation { from: format!("{}.{}", f.class.name, f.m.name), to });
        }
    }
    // forwarding chains: A.m → B.n → C.o means A.m and C.o are one operation, not duplicates
    let fwd: BTreeMap<String, String> = r.delegations.iter().map(|x| (x.from.clone(), x.to.clone())).collect();
    let reaches = |from: &str, to: &str| {
        let mut cur = from.to_string();
        for _ in 0..8 {
            match fwd.get(&cur) {
                Some(n) if n == to => return true,
                Some(n) => cur = n.clone(),
                None => return false,
            }
        }
        false
    };
    // method pairs across classes
    let mut near: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut stamp: Vec<usize> = vec![usize::MAX; feats.len()];
    for i in 0..feats.len() {
        let candidates: Vec<usize> = if min_score >= 0.49 {
            let mut c = vec![];
            for w in &feats[i].ids[0] {
                for &j in &by_word[w] {
                    if j > i && stamp[j] != i {
                        stamp[j] = i;
                        c.push(j);
                    }
                }
            }
            c.sort_unstable();
            c
        } else {
            ((i + 1)..feats.len()).collect()
        };
        for j in candidates {
            let (a, b) = (&feats[i], &feats[j]);
            if a.class.name == b.class.name {
                continue;
            }
            // with too few shared name words the score stays below 0.5: skip the work
            if min_score >= 0.49 && jaccard_ids(&a.ids[0], &b.ids[0]) < 0.34 {
                continue;
            }
            // one calls the other: delegation, not duplication
            if calls_target(a.m, &b.class.name, &b.m.name) || calls_target(b.m, &a.class.name, &a.m.name) {
                continue;
            }
            if a.m.name == b.m.name && !anc.is_empty() && overrides_same(d, &anc, a.class, b.class, &a.m.name) {
                continue;
            }
            if handler(a.m) != handler(b.m) {
                continue;
            }
            let mut p = similarity(a, b);
            if p.score < min_score && p.score < DUPLICATE {
                continue;
            }
            let (ka, kb) = (format!("{}.{}", a.class.name, a.m.name), format!("{}.{}", b.class.name, b.m.name));
            if reaches(&ka, &kb) || reaches(&kb, &ka) {
                continue;
            }
            if p.score >= DUPLICATE {
                *near.entry((a.class.name.clone(), b.class.name.clone())).or_default() += 1;
            }
            if p.score >= min_score {
                p.a = ka;
                p.b = kb;
                p.shared = a.name.intersection(&b.name).cloned().collect();
                r.pairs.push(p);
            }
        }
    }
    r.pairs.sort_by(|x, y| y.score.partial_cmp(&x.score).unwrap().then(x.a.cmp(&y.a)));

    // class overlap matrix
    let mut behaviour_n: BTreeMap<&str, usize> = BTreeMap::new();
    for f in &feats {
        *behaviour_n.entry(f.class.name.as_str()).or_default() += 1;
    }
    let behaviour = |c: &str| behaviour_n.get(c).copied().unwrap_or(0);
    let resp_tok: Vec<BTreeSet<String>> = classes.iter().map(|c| c.responsibilities.iter().flat_map(|r| tokens(r)).collect()).collect();
    for i in 0..classes.len() {
        for j in (i + 1)..classes.len() {
            let (a, b) = (classes[i], classes[j]);
            let (ra, rb) = (&resp_tok[i], &resp_tok[j]);
            let n = if near.is_empty() { 0 } else { near.get(&(a.name.clone(), b.name.clone())).or_else(|| near.get(&(b.name.clone(), a.name.clone()))).copied().unwrap_or(0) };
            let resp = if ra.len() >= 3 && rb.len() >= 3 { jaccard(ra, rb) } else { 0.0 };
            if n == 0 && resp < 0.25 {
                continue;
            }
            let share = |c: &str| {
                let t = behaviour(c);
                if t == 0 { 0.0 } else { (n as f64 / t as f64).min(1.0) }
            };
            r.classes.push(ClassOverlap { a: a.name.clone(), b: b.name.clone(), share_a: share(&a.name), share_b: share(&b.name), responsibility: resp, pairs: n });
        }
    }

    // concepts another class owns (its attributes or its stated responsibilities)
    let attr_tok: BTreeMap<&str, Vec<(&Attribute, BTreeSet<String>)>> = classes
        .iter()
        .map(|c| (c.name.as_str(), c.attributes.iter().filter(|a| live(a.status)).map(|a| (a, tokens(&a.name))).filter(|(_, t)| t.len() >= 2).collect()))
        .collect();
    let resp_each: BTreeMap<&str, Vec<(&String, BTreeSet<String>)>> =
        classes.iter().map(|c| (c.name.as_str(), c.responsibilities.iter().map(|r| (r, tokens(r))).collect())).collect();
    for f in &feats {
        if delegating.contains(&format!("{}.{}", f.class.name, f.m.name)) || f.name.len() < 2 || handler(f.m) || inherited_method(d, &anc, f.class, &f.m.name).is_some() {
            continue;
        }
        let own_data = !f.m.uses.is_empty();
        for other in classes.iter().filter(|o| o.name != f.class.name) {
            for (a, at) in &attr_tok[other.name.as_str()] {
                if own_data || f.name.is_disjoint(at) {
                    continue;
                }
                let shared: BTreeSet<String> = f.name.intersection(at).cloned().collect();
                if shared.len() == at.len() {
                    r.claims.push(Claim {
                        method: format!("{}.{}", f.class.name, f.m.name),
                        owner: other.name.clone(),
                        via: "attribute".into(),
                        evidence: format!("{} already keeps {}", other.name, a.name),
                        shared: shared.into_iter().collect(),
                        uses_no_own_data: !own_data,
                    });
                }
            }
            for (resp, rt) in &resp_each[other.name.as_str()] {
                if f.name.is_disjoint(rt) {
                    continue;
                }
                let shared: BTreeSet<String> = f.name.intersection(rt).cloned().collect();
                // two or more of the method's words appear in what the other class says it is responsible for
                if shared.len() >= 2 && shared.len() as f64 >= 0.6 * f.name.len() as f64 {
                    let mine = resp_each[f.class.name.as_str()].iter().any(|(_, x)| x.intersection(&shared).count() == shared.len());
                    if !mine {
                        r.claims.push(Claim {
                            method: format!("{}.{}", f.class.name, f.m.name),
                            owner: other.name.clone(),
                            via: "responsibility".into(),
                            evidence: format!("{} says: “{resp}”", other.name),
                            shared: shared.into_iter().collect(),
                            uses_no_own_data: !own_data,
                        });
                    }
                }
            }
        }
    }

    // scattered concepts (shotgun surgery): a non-class concept handled by behaviour in ≥ 3 classes
    let class_words: BTreeSet<String> = classes.iter().flat_map(|c| tokens(&c.name)).collect();
    let mut by_concept: BTreeMap<String, BTreeSet<(String, String)>> = BTreeMap::new();
    for f in &feats {
        if delegating.contains(&format!("{}.{}", f.class.name, f.m.name)) {
            continue;
        }
        // the first word of a multi-word name is (usually) the verb, not the concept
        let ws: Vec<String> = words(&f.m.name).into_iter().filter(|w| !STOP.contains(&w.as_str())).map(|w| canon(&w)).collect();
        let concepts: &[String] = if ws.len() >= 2 { &ws[1..] } else { &[] };
        for t in concepts {
            if !class_words.contains(t) && t.len() > 2 {
                by_concept.entry(t.clone()).or_default().insert((f.class.name.clone(), f.m.name.clone()));
            }
        }
    }
    for (concept, ms) in by_concept {
        // overrides of one supertype method count as one place (polymorphism)
        let family = |c: &String, m: &String| d.class(c).and_then(|cl| inherited_method(d, &anc, cl, m)).unwrap_or_else(|| c.clone());
        let fams: BTreeSet<String> = ms.iter().map(|(c, m)| family(c, m)).collect();
        let cls: BTreeSet<&String> = ms.iter().map(|(c, _)| c).collect();
        if fams.len() >= 3 {
            r.scattered.push(Scattered {
                concept,
                classes: cls.into_iter().cloned().collect(),
                methods: ms.iter().map(|(c, m)| format!("{c}.{m}")).collect(),
            });
        }
    }

    // middle man: a (non-controller) class that mostly forwards
    for c in &classes {
        let mine: Vec<&Features> = feats.iter().filter(|f| f.class.name == c.name).collect();
        let fwd = mine.iter().filter(|f| delegating.contains(&format!("{}.{}", c.name, f.m.name))).count();
        let controllerish = matches!(c.stereotype.as_deref(), Some("controller" | "service" | "facade" | "adapter"))
            || ["Controller", "Facade", "Service", "Handler", "Adapter", "Proxy"].iter().any(|s| c.name.ends_with(s))
            || matches!(c.role.as_deref(), Some("controller" | "service" | "adapter"));
        if !controllerish && mine.len() >= 3 && fwd * 3 >= mine.len() * 2 {
            r.middle_men.push(c.name.clone());
        }
    }
    r
}
