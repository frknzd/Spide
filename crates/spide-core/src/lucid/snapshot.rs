//! A Lucid document as Spide sees it: the read script's raw result normalised into [`Item`]s (with
//! line ends resolved to Spide ids), comment threads, and a fingerprint of what matters for sync.

use super::{Bbox, BlockShape, Item, ItemKind, SID_FIELD};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// The read script (run with `lucid_run_script`; the `uml` API must be loaded for the document).
pub const READ_SCRIPT: &str = include_str!("read_script.js");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Comment {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub text: String,
    /// ISO-8601
    #[serde(default)]
    pub at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Thread {
    pub id: String,
    /// The Spide id of the item it is pinned to (resolved from the Lucid item), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sid: Option<String>,
    /// The Lucid item it is pinned to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
    #[serde(default)]
    pub resolved: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bbox: Option<Bbox>,
    #[serde(default)]
    pub comments: Vec<Comment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Snapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_id: Option<String>,
    /// `lucid_get_document_metadata.version` when read (set by the caller).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u64>,
    pub items: Vec<Item>,
    #[serde(default)]
    pub threads: Vec<Thread>,
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn bbox(v: &Value) -> Option<Bbox> {
    let b = v.as_object()?;
    let n = |k: &str| b.get(k).and_then(Value::as_f64);
    Some(Bbox { x: n("x")?, y: n("y")?, w: n("w")?, h: n("h")? })
}

fn lines(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default()
}

/// Classify one raw block (no lines).
fn block_kind(o: &Value, sid: Option<&str>) -> ItemKind {
    let class = s(o, "className");
    let uml = s(o, "umlType");
    let label = s(o, "label");
    let area = |k: &str| o.get("textAreas").and_then(|t| t.get(k)).and_then(Value::as_str).map(str::to_string);
    if let Some(m) = o.get("markup").and_then(Value::as_str) {
        return ItemKind::Sequence { markup: m.to_string() };
    }
    if class == "UMLClassBlock" || uml.starts_with("class") || uml.starts_with("interface") || uml == "activeClass" {
        let c = o.get("compartments");
        let name = c.and_then(|c| c.get("name")).and_then(Value::as_str).map(str::to_string).or_else(|| area("Title")).unwrap_or_else(|| label.lines().next().unwrap_or_default().to_string());
        let empty = Value::Null;
        // interface shapes hide their attribute compartment (Lucid fills it with placeholders)
        let attributes = if uml.starts_with("interface") { vec![] } else { lines(c.and_then(|c| c.get("attributes")).unwrap_or(&empty)) };
        let operations = lines(c.and_then(|c| c.get("operations")).unwrap_or(&empty));
        return ItemKind::Class { name, attributes, operations, interface: uml.starts_with("interface") };
    }
    if class == "UMLPackageBlock" || uml == "package" {
        return ItemKind::Package { name: area("Title").unwrap_or_else(|| label.lines().next().unwrap_or_default().to_string()), body: area("Text").unwrap_or_default() };
    }
    if class == "UMLActorBlock" || uml == "actor" {
        return ItemKind::Actor { name: area("Text").unwrap_or(label) };
    }
    if class.starts_with("UMLUseCase") || uml.starts_with("useCase") {
        return ItemKind::UseCase { name: area("Text").unwrap_or_else(|| label.lines().next().unwrap_or_default().to_string()) };
    }
    if uml == "systemBoundary" || class.contains("Boundary") || sid.is_some_and(|s| s.starts_with("boundary:")) {
        return ItemKind::Boundary { name: label.lines().next().unwrap_or_default().to_string() };
    }
    let c4 = sid.is_some_and(|s| s.starts_with("c4:"));
    if c4 || class == "DatabaseBlock" && label.contains("[Container") {
        let shape = if class == "DatabaseBlock" {
            BlockShape::Datastore
        } else if label.contains("[Person") {
            BlockShape::Person
        } else if label.contains("[Container") {
            BlockShape::Container
        } else {
            BlockShape::System
        };
        return ItemKind::Block { shape, text: label };
    }
    ItemKind::Other { class_name: class, text: label }
}

/// Normalise the read script's result (`{pageId, items, threads}`, or `{result: …}`).
pub fn from_raw(raw: &Value) -> Result<Snapshot, String> {
    let raw = raw.get("result").unwrap_or(raw);
    let items_raw = raw.get("items").and_then(Value::as_array).ok_or("the read result has no items")?;
    let sid_of: HashMap<String, String> = items_raw
        .iter()
        .filter_map(|o| Some((s(o, "id"), o.get("data")?.get(SID_FIELD)?.as_str()?.to_string())))
        .filter(|(_, sid)| !sid.is_empty())
        .collect();
    // a copy-pasted shape carries its original's Spide id: the first one owns it
    let mut seen_sid = std::collections::HashSet::new();
    let owned: std::collections::HashSet<String> = items_raw
        .iter()
        .filter_map(|o| {
            let id = s(o, "id");
            let sid = sid_of.get(&id)?;
            seen_sid.insert(sid.clone()).then_some(id)
        })
        .collect();
    let sid_of: HashMap<String, String> = sid_of.into_iter().filter(|(id, _)| owned.contains(id)).collect();
    let end = |id: Option<&str>| -> String {
        match id {
            Some(i) if !i.is_empty() => sid_of.get(i).cloned().unwrap_or_else(|| format!("lucid:{i}")),
            _ => String::new(),
        }
    };
    let c4_block = |id: Option<&str>| -> bool {
        id.and_then(|i| items_raw.iter().find(|o| s(o, "id") == i)).is_some_and(|o| s(o, "kind") != "line" && matches!(block_kind(o, sid_of.get(&s(o, "id")).map(String::as_str)), ItemKind::Block { .. }))
    };
    let mut items = vec![];
    for o in items_raw {
        let id = s(o, "id");
        let sid = sid_of.get(&id).cloned();
        let (from_id, to_id) = (o.get("from").and_then(Value::as_str), o.get("to").and_then(Value::as_str));
        let kind = if s(o, "kind") == "line" {
            match o.get("rel").filter(|_| !(c4_block(from_id) && c4_block(to_id))) {
                Some(r) => ItemKind::Relationship {
                    from: end(r.get("from").and_then(Value::as_str).or_else(|| o.get("from").and_then(Value::as_str))),
                    to: end(r.get("to").and_then(Value::as_str).or_else(|| o.get("to").and_then(Value::as_str))),
                    rel: s(r, "type"),
                    label: s(r, "label"),
                    from_mult: s(r, "fromMultiplicity"),
                    to_mult: s(r, "toMultiplicity"),
                },
                None => ItemKind::Line { from: end(o.get("from").and_then(Value::as_str)), to: end(o.get("to").and_then(Value::as_str)), label: lines(o.get("labels").unwrap_or(&Value::Null)).join(" ") },
            }
        } else {
            block_kind(o, sid.as_deref())
        };
        let bbox = o.get("bbox").and_then(bbox);
        items.push(Item { sid, lucid_id: Some(id), kind, bbox, fill: None });
    }
    let threads = raw
        .get("threads")
        .and_then(Value::as_array)
        .map(|ts| {
            ts.iter()
                .map(|t| {
                    let item = t.get("itemId").and_then(Value::as_str).map(str::to_string);
                    Thread {
                        id: s(t, "id"),
                        sid: item.as_ref().and_then(|i| sid_of.get(i).cloned()),
                        item,
                        resolved: t.get("resolved").and_then(Value::as_bool).unwrap_or(false),
                        bbox: t.get("bbox").and_then(bbox),
                        comments: t.get("comments").and_then(|c| serde_json::from_value(c.clone()).ok()).unwrap_or_default(),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Snapshot { page_id: raw.get("pageId").and_then(Value::as_str).map(str::to_string), version: None, items, threads })
}

impl Snapshot {
    pub fn by_sid(&self, sid: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.sid.as_deref() == Some(sid))
    }
    pub fn by_lucid(&self, id: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.lucid_id.as_deref() == Some(id))
    }
    /// Spide id → Lucid id of every Spide-owned item.
    pub fn id_map(&self) -> std::collections::BTreeMap<String, String> {
        self.items.iter().filter_map(|i| Some((i.sid.clone()?, i.lucid_id.clone()?))).collect()
    }
    /// What sync compares: every design-relevant item (not colleagues' notes, not comments, not
    /// colours), its content and its grid-rounded geometry, independent of Lucid's item order.
    pub fn fingerprint(&self) -> String {
        let mut parts: Vec<String> = self
            .items
            .iter()
            .filter(|i| !matches!(i.kind, ItemKind::Other { .. }))
            .map(|i| {
                let key = i.sid.clone().unwrap_or_else(|| format!("lucid:{}", i.lucid_id.clone().unwrap_or_default()));
                let geo = if i.kind.is_line() || matches!(i.kind, ItemKind::Sequence { .. }) { String::new() } else { i.bbox.map(|b| format!("{:?}", b.grid())).unwrap_or_default() };
                format!("{key}|{}|{geo}", i.kind.canonical())
            })
            .collect();
        parts.sort();
        crate::assess::fnv(&parts.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spike() -> Snapshot {
        let raw: Value = serde_json::from_str(include_str!("../../tests/fixtures/lucid/read-script-spike.json")).unwrap();
        from_raw(&raw).unwrap()
    }

    #[test]
    fn normalises_the_live_read() {
        let s = spike();
        let order = s.by_sid("class:PurchaseOrder").unwrap();
        assert_eq!(order.kind, ItemKind::Class { name: "PurchaseOrder".into(), attributes: vec!["- total: Money".into(), "- placedAt: Instant".into()], operations: vec!["+ checkout(): void".into()], interface: false });
        // interface placeholders are not attributes
        let ItemKind::Class { attributes, interface, .. } = &s.by_sid("class:Priced").unwrap().kind else { panic!() };
        assert!(attributes.is_empty() && *interface);
        // line ends resolve to Spide ids
        assert_eq!(s.by_sid("rel:PurchaseOrder-realization-Priced").unwrap().kind, ItemKind::Relationship { from: "class:PurchaseOrder".into(), to: "class:Priced".into(), rel: "realization".into(), label: String::new(), from_mult: String::new(), to_mult: String::new() });
        let ItemKind::Sequence { markup } = &s.by_sid("seq:Checkout").unwrap().kind else { panic!() };
        assert!(markup.contains("loop each line"));
        assert!(s.items.iter().any(|i| matches!(&i.kind, ItemKind::Actor { name } if name == "Customer")));
        assert!(s.items.iter().any(|i| matches!(&i.kind, ItemKind::UseCase { name } if name == "Place order")));
        assert!(s.items.iter().any(|i| matches!(&i.kind, ItemKind::Package { name, .. } if name == "domain")));
        assert!(s.items.iter().any(|i| matches!(&i.kind, ItemKind::Other { class_name, .. } if class_name == "SparkFrameBlock")));
        assert_eq!(s.id_map().get("class:OrderLine").map(String::as_str), Some("yjsNc6Afx~n6"));
    }

    #[test]
    fn fingerprint_ignores_order_jitter_and_notes() {
        let a = spike();
        let mut b = a.clone();
        b.items.reverse();
        assert_eq!(a.fingerprint(), b.fingerprint());
        // sub-grid jitter
        b.items.iter_mut().find(|i| i.sid.as_deref() == Some("class:OrderLine")).unwrap().bbox.as_mut().unwrap().x += 2.0;
        assert_eq!(a.fingerprint(), b.fingerprint());
        // colleagues' shapes and comments don't count
        b.items.retain(|i| !matches!(i.kind, ItemKind::Other { .. }));
        b.threads.push(Thread { id: "t".into(), ..Default::default() });
        assert_eq!(a.fingerprint(), b.fingerprint());
        // a moved box and an edited member do
        let mut c = a.clone();
        c.items.iter_mut().find(|i| i.sid.as_deref() == Some("class:OrderLine")).unwrap().bbox.as_mut().unwrap().y += 200.0;
        assert_ne!(a.fingerprint(), c.fingerprint());
        let mut e = a.clone();
        if let ItemKind::Class { attributes, .. } = &mut e.items.iter_mut().find(|i| i.sid.as_deref() == Some("class:OrderLine")).unwrap().kind {
            attributes.push("- note: String".into());
        }
        assert_ne!(a.fingerprint(), e.fingerprint());
    }
}
