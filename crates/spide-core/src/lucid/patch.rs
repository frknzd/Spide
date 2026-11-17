//! Push: the in-place edits that turn the Lucid document (as read now) into the target, rendered as
//! `lucid_run_script` scripts.
//!
//! Only Spide-owned items (with a `spide.id`) are ever updated, moved or deleted; colleagues' shapes,
//! notes, styling and comments are left alone. Scripts find items by Spide id at run time, so every
//! chunk is idempotent: re-running a chunk (after a timeout or a partial failure) changes nothing
//! that is already right, and Lucid reverts a failing script as a whole.

use super::snapshot::Snapshot;
use super::{Bbox, BlockShape, Item, ItemKind, Target, HASH_FIELD, SID_FIELD};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum PatchOp {
    /// Create an item and tag it with its Spide id.
    Add { item: Item },
    /// The item now has another Spide id (renamed in Spide): keep the Lucid shape, its comments and
    /// its links, re-tag it.
    Retag { from: String, to: String },
    /// Rewrite an item's content (same kind of shape).
    Update { sid: String, kind: ItemKind },
    /// Move a box (top-left); frames (packages, boundaries) are also resized to wrap their members.
    Move {
        sid: String,
        x: f64,
        y: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size: Option<(f64, f64)>,
    },
    Delete { sid: String },
}

impl PatchOp {
    pub fn sid(&self) -> &str {
        match self {
            PatchOp::Add { item } => item.sid.as_deref().unwrap_or_default(),
            PatchOp::Retag { to: sid, .. } | PatchOp::Update { sid, .. } | PatchOp::Move { sid, .. } | PatchOp::Delete { sid } => sid,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Patch {
    pub ops: Vec<PatchOp>,
}

impl Patch {
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

/// Content as compared for updates: what the snapshot can read back.
fn comparable(k: &ItemKind) -> ItemKind {
    match k {
        // shapes are always created as plain classes; `interface` is only what the snapshot infers
        ItemKind::Class { name, attributes, operations, .. } => ItemKind::Class { name: name.clone(), attributes: attributes.clone(), operations: operations.clone(), interface: false },
        // the snapshot can't tell an external system from ours (same block, same marker)
        ItemKind::Block { shape, text } => ItemKind::Block { shape: if *shape == BlockShape::ExternalSystem { BlockShape::System } else { *shape }, text: text.trim().to_string() },
        k => k.clone(),
    }
}

/// Can the remote item become the target item in place (same Lucid shape)?
fn same_shape(a: &ItemKind, b: &ItemKind) -> bool {
    use ItemKind::*;
    match (a, b) {
        (Class { .. }, Class { .. }) | (Package { .. }, Package { .. }) | (Actor { .. }, Actor { .. }) | (UseCase { .. }, UseCase { .. }) | (Boundary { .. }, Boundary { .. }) | (Sequence { .. }, Sequence { .. }) => true,
        (Block { shape: x, .. }, Block { shape: y, .. }) => x.class_name() == y.class_name(),
        // lines keep their ends; another end means another line
        (Relationship { from: f1, to: t1, .. }, Relationship { from: f2, to: t2, .. }) | (Line { from: f1, to: t1, .. }, Line { from: f2, to: t2, .. }) => f1 == f2 && t1 == t2,
        _ => false,
    }
}

/// A move (and, for frames, a resize) when the grid position (size) differs.
fn moved(sid: &str, target: Option<Bbox>, remote: Option<Bbox>, frame: bool) -> Option<PatchOp> {
    let (t, r) = (target?, remote?);
    let g = |v: f64| (v / 10.0).round() as i64;
    let resize = frame && (g(t.w) != g(r.w) || g(t.h) != g(r.h));
    (g(t.x) != g(r.x) || g(t.y) != g(r.y) || resize).then(|| PatchOp::Move { sid: sid.into(), x: t.x, y: t.y, size: resize.then_some((t.w, t.h)) })
}

/// What turns `remote` into `target`. Order: frames, boxes, lines, sequences; deletions of lines
/// before their boxes.
fn member_lines(k: &ItemKind) -> Vec<&str> {
    match k {
        ItemKind::Class { attributes, operations, .. } => attributes.iter().chain(operations).map(String::as_str).collect(),
        _ => vec![],
    }
}

fn similarity(a: &ItemKind, b: &ItemKind) -> f64 {
    let (x, y) = (member_lines(a), member_lines(b));
    if x.is_empty() && y.is_empty() {
        return 0.5;
    }
    let common = x.iter().filter(|l| y.contains(l)).count() as f64;
    common / (x.len() + y.len()) as f64 * 2.0
}

/// Spide ids that changed since the remote was written (renamed classes and concepts, then the lines
/// attached to them): old → new.
pub fn renames(remote: &Snapshot, target: &Target) -> Vec<(String, String)> {
    let gone: Vec<&Item> = remote.items.iter().filter(|r| r.sid.as_deref().is_some_and(|s| target.item(s).is_none())).collect();
    let new: Vec<&Item> = target.items.iter().filter(|t| t.sid.as_deref().is_some_and(|s| remote.by_sid(s).is_none())).collect();
    let mut out: Vec<(String, String)> = vec![];
    // boxes: best member overlap, at least half
    let mut pairs: Vec<(f64, String, String)> = vec![];
    for g in gone.iter().filter(|g| matches!(g.kind, ItemKind::Class { .. })) {
        for n in new.iter().filter(|n| matches!(n.kind, ItemKind::Class { .. })) {
            let (gs, ns) = (g.sid.clone().unwrap_or_default(), n.sid.clone().unwrap_or_default());
            if gs.split(':').next() != ns.split(':').next() {
                continue;
            }
            let sim = similarity(&g.kind, &n.kind);
            if sim >= 0.5 {
                pairs.push((sim, gs, ns));
            }
        }
    }
    pairs.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (_, g, n) in pairs {
        if !out.iter().any(|(a, b)| *a == g || *b == n) {
            out.push((g, n));
        }
    }
    // lines whose ends were renamed
    let map = |s: &str| out.iter().find(|(a, _)| a == s).map(|(_, b)| b.clone()).unwrap_or_else(|| s.to_string());
    let mut line_renames = vec![];
    for g in gone.iter().filter(|g| g.kind.is_line()) {
        let Some((f, t)) = g.kind.ends() else { continue };
        let (f, t) = (map(f), map(t));
        let rel = |k: &ItemKind| if let ItemKind::Relationship { rel, .. } = k { rel.clone() } else { String::new() };
        if let Some(n) = new.iter().find(|n| n.kind.ends() == Some((f.as_str(), t.as_str())) && rel(&n.kind) == rel(&g.kind) && !line_renames.iter().any(|(_, b): &(String, String)| Some(b.as_str()) == n.sid.as_deref())) {
            line_renames.push((g.sid.clone().unwrap_or_default(), n.sid.clone().unwrap_or_default()));
        }
    }
    out.extend(line_renames);
    out
}

pub fn plan(remote: &Snapshot, target: &Target) -> Patch {
    // apply renames to a copy of the remote first: then everything else compares by the new ids
    let renamed = renames(remote, target);
    let mut remote = remote.clone();
    for it in remote.items.iter_mut() {
        if let Some((_, to)) = renamed.iter().find(|(from, _)| it.sid.as_deref() == Some(from.as_str())) {
            it.sid = Some(to.clone());
        }
        let map = |s: &mut String| {
            if let Some((_, to)) = renamed.iter().find(|(from, _)| from == s) {
                *s = to.clone();
            }
        };
        match &mut it.kind {
            ItemKind::Relationship { from, to, .. } | ItemKind::Line { from, to, .. } => {
                map(from);
                map(to);
            }
            _ => {}
        }
    }
    let remote = &remote;
    let mut adds_frames = vec![];
    let mut adds_boxes = vec![];
    let mut adds_lines = vec![];
    let mut updates = vec![];
    let mut del_lines = vec![];
    let mut del_boxes = vec![];
    for t in &target.items {
        let Some(sid) = t.sid.as_deref() else { continue };
        match remote.by_sid(sid) {
            Some(r) if same_shape(&r.kind, &t.kind) => {
                if comparable(&r.kind) != comparable(&t.kind) {
                    updates.push(PatchOp::Update { sid: sid.into(), kind: t.kind.clone() });
                }
                if !t.kind.is_line() && !matches!(t.kind, ItemKind::Sequence { .. }) {
                    let frame = matches!(t.kind, ItemKind::Package { .. } | ItemKind::Boundary { .. });
                    updates.extend(moved(sid, t.bbox, r.bbox, frame));
                }
            }
            other => {
                if let Some(r) = other {
                    // another shape now: replace it
                    if r.kind.is_line() { del_lines.push(PatchOp::Delete { sid: sid.into() }) } else { del_boxes.push(PatchOp::Delete { sid: sid.into() }) }
                }
                let add = PatchOp::Add { item: t.clone() };
                match t.kind {
                    _ if t.kind.is_line() => adds_lines.push(add),
                    ItemKind::Package { .. } | ItemKind::Boundary { .. } => adds_frames.push(add),
                    _ => adds_boxes.push(add),
                }
            }
        }
    }
    for r in &remote.items {
        let Some(sid) = r.sid.as_deref() else { continue };
        if target.item(sid).is_none() {
            if r.kind.is_line() {
                del_lines.push(PatchOp::Delete { sid: sid.into() });
            } else {
                del_boxes.push(PatchOp::Delete { sid: sid.into() });
            }
        }
    }
    let mut ops: Vec<PatchOp> = renamed.into_iter().map(|(from, to)| PatchOp::Retag { from, to }).collect();
    ops.extend(del_lines);
    ops.extend(del_boxes);
    ops.extend(adds_frames);
    ops.extend(adds_boxes);
    ops.extend(updates);
    ops.extend(adds_lines);
    Patch { ops }
}

/// Split into chunks small enough for one script run (25 s); lines never precede their boxes because
/// chunks run in order and look ids up at run time.
pub fn chunks(p: &Patch, size: usize) -> Vec<Patch> {
    p.ops.chunks(size.max(1)).map(|c| Patch { ops: c.to_vec() }).collect()
}

/// Ops per script run.
pub const CHUNK: usize = 40;

const PRELUDE: &str = r#"const SID = "__SID__", HASH = "__HASH__";
const __read = await skills.basecanvasediting.read({});
const __all = new Set();
const __walk = (v) => {
  if (Array.isArray(v)) { v.forEach(__walk); return; }
  if (!v || typeof v !== "object") return;
  for (const [k, x] of Object.entries(v)) {
    if (typeof x === "string" && ["id", "itemId", "shapeId", "containerId", "generatorId", "lineId"].includes(k)) __all.add(x);
    else if (k === "itemIds" && Array.isArray(x)) x.forEach((i) => typeof i === "string" && __all.add(i));
    else __walk(x);
  }
};
for (const p of __read.pages || []) __walk(p.data);
for (const s of skills.uml.list()) __all.add(s.id);
for (const r of skills.uml.listRelationships()) __all.add(r.id);
for (const g of skills.uml.listSequenceDiagrams()) __all.add(g.generatorId);
const __ids = {};
for (const i of __all) {
  try { const f = skills.formulas.getShapeDataField(i, SID); if (f && typeof f.value === "string" && !(f.value in __ids)) __ids[f.value] = i; } catch (e) {}
}
const id = (sid) => { const i = __ids[sid]; if (!i) throw new Error("no item for " + sid); return i; };
const has = (sid) => sid in __ids;
const setData = (item, name, value) => {
  if (skills.formulas.getShapeDataField(item, name)) skills.formulas.setShapeDataValue(item, name, value);
  else skills.formulas.addShapeDataField(item, name, value);
};
const tag = (item, sid, hash) => { setData(item, SID, sid); setData(item, HASH, hash); __ids[sid] = item; };
let applied = 0;
"#;

fn js(v: &impl Serialize) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "null".into())
}

fn r(v: f64) -> f64 {
    v.round()
}

fn add_js(it: &Item) -> String {
    let sid = it.sid.clone().unwrap_or_default();
    let hash = it.kind.content_hash();
    let b = it.bbox.unwrap_or_default();
    let loc = format!("{{x: {}, y: {}}}", r(b.x), r(b.y));
    let size = if b.w > 0.0 && b.h > 0.0 { format!(", width: {}, height: {}", r(b.w), r(b.h)) } else { String::new() };
    let fill = it.fill.as_ref().map(|f| format!(", fillColor: {}", js(f))).unwrap_or_default();
    let body = match &it.kind {
        ItemKind::Class { name, attributes, operations, .. } => format!(
            "const s = skills.uml.addShape({{type: \"class\", location: {loc}{size}{fill}, compartments: {{name: {}, attributes: {}, operations: {}}}}});\n  tag(s.id, {sid}, {hash});",
            js(name), js(attributes), js(operations), sid = js(&sid), hash = js(&hash)
        ),
        ItemKind::Package { name, body } => format!(
            "const s = skills.uml.addShape({{type: \"package\", label: {n}, location: {loc}{size}{fill}}});\n  try {{ skills.uml.setText(s.id, {{Title: {n}, Text: {b}}}); }} catch (e) {{}}\n  skills.basecanvasediting.setZOrder(s.id, \"bottom\");\n  tag(s.id, {sid}, {hash});",
            n = js(name), b = js(body), sid = js(&sid), hash = js(&hash)
        ),
        ItemKind::Boundary { name } => format!(
            "const s = skills.uml.addShape({{type: \"systemBoundary\", label: {n}, location: {loc}{size}}});\n  skills.basecanvasediting.setZOrder(s.id, \"bottom\");\n  tag(s.id, {sid}, {hash});",
            n = js(name), sid = js(&sid), hash = js(&hash)
        ),
        ItemKind::Actor { name } => format!("const s = skills.uml.addShape({{type: \"actor\", label: {}, location: {loc}}});\n  tag(s.id, {}, {});", js(name), js(&sid), js(&hash)),
        ItemKind::UseCase { name } => format!("const s = skills.uml.addShape({{type: \"useCase\", label: {}, location: {loc}{size}}});\n  tag(s.id, {}, {});", js(name), js(&sid), js(&hash)),
        ItemKind::Block { shape, text } => format!(
            "const s = await skills.basecanvasediting.createBlock({{className: {}, label: {}, x: {}, y: {}{}}});\n  skills.basecanvasediting.editBlockStyle({{itemId: s.id, fillColor: {}}});\n  tag(s.id, {}, {});",
            js(&shape.class_name()), js(text), r(b.x), r(b.y), size, js(&it.fill.clone().unwrap_or_else(|| shape.fill().to_string())), js(&sid), js(&hash)
        ),
        ItemKind::Relationship { from, to, rel, label, from_mult, to_mult } => format!(
            "const l = skills.uml.addRelationship({{from: id({}), to: id({}), type: {}, label: {}, fromMultiplicity: {}, toMultiplicity: {}, shape: \"elbow\"}});\n  tag(l, {}, {});",
            js(from), js(to), js(rel), js(label), js(from_mult), js(to_mult), js(&sid), js(&hash)
        ),
        ItemKind::Line { from, to, label } => format!(
            "const l = skills.basecanvasediting.createLine({{from: {{kind: \"block\", blockId: id({})}}, to: {{kind: \"block\", blockId: id({})}}, label: {}, shape: \"elbow\", style: {{endpoint2: \"Arrow\"}}}}).lineId;\n  tag(l, {}, {});",
            js(from), js(to), js(label), js(&sid), js(&hash)
        ),
        ItemKind::Sequence { markup } => format!(
            "const g = skills.uml.createSequenceDiagram({{location: {loc}, markup: {}}});\n  tag(g.generatorId, {}, {});",
            js(markup), js(&sid), js(&hash)
        ),
        ItemKind::Other { .. } => return String::new(),
    };
    format!("if (!has({})) {{\n  {body}\n  applied++;\n}}\n", js(&sid))
}

fn update_js(sid: &str, kind: &ItemKind) -> String {
    let hash = kind.content_hash();
    let body = match kind {
        ItemKind::Class { name, attributes, operations, .. } => format!("skills.uml.setCompartments(id({s}), {{name: {}, attributes: {}, operations: {}}});", js(name), js(attributes), js(operations), s = js(&sid)),
        ItemKind::Package { name, body } => format!("skills.uml.setText(id({s}), {{Title: {}, Text: {}}});", js(name), js(body), s = js(&sid)),
        ItemKind::Boundary { name } | ItemKind::Actor { name } | ItemKind::UseCase { name } => format!("skills.uml.setLabel(id({}), {});", js(&sid), js(name)),
        ItemKind::Block { text, .. } => format!("await skills.basecanvasediting.setText({{itemId: id({}), text: {}}});", js(&sid), js(text)),
        ItemKind::Relationship { rel, label, from_mult, to_mult, .. } => format!(
            "skills.uml.setRelationshipType(id({s}), {});\n  skills.uml.setRelationshipLabel(id({s}), {});\n  skills.uml.setMultiplicity(id({s}), \"from\", {});\n  skills.uml.setMultiplicity(id({s}), \"to\", {});",
            js(rel), js(label), js(from_mult), js(to_mult), s = js(&sid)
        ),
        ItemKind::Line { label, .. } => format!("await skills.basecanvasediting.setText({{itemId: id({}), text: {}}});", js(&sid), js(label)),
        ItemKind::Sequence { markup } => format!("skills.uml.setSequenceMarkup(id({}), {});", js(&sid), js(markup)),
        ItemKind::Other { .. } => return String::new(),
    };
    format!("{{\n  {body}\n  setData(id({s}), HASH, {h});\n  applied++;\n}}\n", s = js(&sid), h = js(&hash))
}

/// The `lucid_run_script` source applying one chunk. Returns `{applied}`.
pub fn script(p: &Patch) -> String {
    let mut out = PRELUDE.replace("__SID__", SID_FIELD).replace("__HASH__", HASH_FIELD);
    for op in &p.ops {
        out.push_str(&match op {
            PatchOp::Add { item } => add_js(item),
            PatchOp::Retag { from, to } => format!("if (has({f}) && !has({t})) {{\n  const i = id({f});\n  setData(i, SID, {t});\n  __ids[{t}] = i;\n  delete __ids[{f}];\n  applied++;\n}}\n", f = js(from), t = js(to)),
            PatchOp::Update { sid, kind } => update_js(sid, kind),
            PatchOp::Move { sid, x, y, size } => {
                let resize = size.map(|(w, h)| format!("skills.basecanvasediting.resizeBlock({{itemId: id({}), width: {}, height: {}}});\n", js(sid), r(w), r(h))).unwrap_or_default();
                format!("{resize}skills.basecanvasediting.moveBlock({{itemId: id({}), x: {}, y: {}}});\napplied++;\n", js(sid), r(*x), r(*y))
            }
            PatchOp::Delete { sid } => format!("if (has({s})) {{\n  skills.basecanvasediting.deleteItem({{itemIds: [id({s})]}});\n  applied++;\n}}\n", s = js(sid)),
        });
    }
    out.push_str("return { applied };\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lucid::Bbox;

    fn class(sid: &str, name: &str, x: f64) -> Item {
        Item::new(sid, ItemKind::Class { name: name.into(), attributes: vec!["- a: int".into()], operations: vec![], interface: false }, Some(Bbox { x, y: 0.0, w: 100.0, h: 80.0 }))
    }

    fn rel(sid: &str, from: &str, to: &str) -> Item {
        Item::new(sid, ItemKind::Relationship { from: from.into(), to: to.into(), rel: "association".into(), label: String::new(), from_mult: String::new(), to_mult: String::new() }, None)
    }

    fn remote_of(items: Vec<Item>) -> Snapshot {
        Snapshot { items: items.into_iter().enumerate().map(|(i, mut it)| { it.lucid_id = Some(format!("L{i}")); it }).collect(), ..Default::default() }
    }

    #[test]
    fn minimal_ops_and_colleagues_untouched() {
        let target = Target { key: "class".into(), title: "t".into(), items: vec![class("class:A", "A", 0.0), class("class:B", "B", 300.0), rel("rel:A>B", "class:A", "class:B")] };
        // in sync: nothing to do
        let same = remote_of(target.items.clone());
        assert!(plan(&same, &target).is_empty());
        // a colleague's note and an interface-looking read are not changes
        let mut r = target.items.clone();
        if let ItemKind::Class { interface, .. } = &mut r[0].kind {
            *interface = true;
        }
        r.push(Item { sid: None, lucid_id: None, kind: ItemKind::Other { class_name: "StickyNote".into(), text: "hi".into() }, bbox: None, fill: None });
        assert!(plan(&remote_of(r), &target).is_empty());
        // B renamed + moved here, A gone there; a stale Spide item to delete
        let mut t2 = target.clone();
        t2.items[1] = class("class:B", "Bee", 500.0);
        let mut r = target.items.clone();
        r.remove(0);
        r.push(Item::new("class:Old", ItemKind::Class { name: "Old".into(), attributes: vec!["- legacy: String".into()], operations: vec!["+ run()".into()], interface: false }, Some(Bbox { x: 900.0, y: 0.0, w: 100.0, h: 80.0 })));
        let p = plan(&remote_of(r), &t2);
        assert_eq!(
            p.ops,
            vec![
                PatchOp::Delete { sid: "class:Old".into() },
                PatchOp::Add { item: class("class:A", "A", 0.0) },
                PatchOp::Update { sid: "class:B".into(), kind: class("class:B", "Bee", 500.0).kind },
                PatchOp::Move { sid: "class:B".into(), x: 500.0, y: 0.0, size: None },
            ]
        );
    }

    #[test]
    fn renamed_classes_keep_their_shape() {
        let remote = remote_of(vec![class("class:Loan", "Loan", 0.0), class("class:Book", "Book", 300.0), rel("rel:Loan->Book:association", "class:Loan", "class:Book")]);
        let target = Target { key: "class".into(), title: "t".into(), items: vec![class("class:Lending", "Lending", 0.0), class("class:Book", "Book", 300.0), rel("rel:Lending->Book:association", "class:Lending", "class:Book")] };
        let p = plan(&remote, &target);
        assert_eq!(
            p.ops,
            vec![
                PatchOp::Retag { from: "class:Loan".into(), to: "class:Lending".into() },
                PatchOp::Retag { from: "rel:Loan->Book:association".into(), to: "rel:Lending->Book:association".into() },
                PatchOp::Update { sid: "class:Lending".into(), kind: class("class:Lending", "Lending", 0.0).kind },
            ]
        );
        assert!(script(&p).contains("setData(i, SID, \"class:Lending\")"));
    }

    #[test]
    fn reattached_lines_and_reshaped_blocks_are_replaced() {
        let target = Target { key: "x".into(), title: "t".into(), items: vec![rel("rel:1", "class:A", "class:C"), Item::new("c4:DB", ItemKind::Block { shape: BlockShape::Datastore, text: "DB".into() }, Some(Bbox::default()))] };
        let remote = remote_of(vec![rel("rel:1", "class:A", "class:B"), Item::new("c4:DB", ItemKind::Block { shape: BlockShape::Container, text: "DB".into() }, Some(Bbox::default()))]);
        let p = plan(&remote, &target);
        assert_eq!(p.ops.iter().filter(|o| matches!(o, PatchOp::Delete { .. })).count(), 2);
        assert_eq!(p.ops.iter().filter(|o| matches!(o, PatchOp::Add { .. })).count(), 2);
        // deletions of lines first, line additions last
        assert!(matches!(&p.ops[0], PatchOp::Delete { sid } if sid == "rel:1"));
        assert!(matches!(p.ops.last().unwrap(), PatchOp::Add { item } if item.kind.is_line()));
    }

    #[test]
    fn scripts_are_idempotent_by_construction() {
        let p = Patch { ops: vec![PatchOp::Add { item: class("class:A", "A", 0.0) }, PatchOp::Delete { sid: "class:Z".into() }] };
        let s = script(&p);
        assert!(s.contains("if (!has(\"class:A\"))"));
        assert!(s.contains("if (has(\"class:Z\"))"));
        assert!(s.contains("\"spide.id\""));
        assert!(s.trim_end().ends_with("return { applied };"));
        assert_eq!(chunks(&Patch { ops: vec![PatchOp::Delete { sid: "a".into() }; 85] }, CHUNK).len(), 3);
    }
}
