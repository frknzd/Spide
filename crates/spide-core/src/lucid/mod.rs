//! Lucid (Lucidchart) sync: the item model shared by the writer (design → target items), the reader
//! (read-script result → snapshot), the mapper (snapshot → design changes), the patcher (base snapshot
//! + target → in-place edit script) and the git-like sync state.
//!
//! Every item Spide owns carries a `spide.id` shape-data field (its *sid*, e.g. `class:Order`), so
//! identity survives renames and moves made by colleagues in Lucid. Items without a sid belong to
//! colleagues (notes, extra shapes); Spide never edits or deletes them.

pub mod map;
pub mod patch;
pub mod seq_markup;
pub mod snapshot;
pub mod state;
pub mod uml_text;

use serde::{Deserialize, Serialize};

/// The scale of the class and domain views in Lucid. Fixed, with absolute positions, so a Lucid
/// position maps back to a saved canvas position and moving one box never shifts the others.
pub const LAYOUT_SCALE: f64 = 1.3;

/// The shape-data field holding an item's Spide id.
pub const SID_FIELD: &str = "spide.id";
/// The shape-data field holding the hash of the content Spide last wrote.
pub const HASH_FIELD: &str = "spide.hash";

/// `x, y, w, h` in Lucid page coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct Bbox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Bbox {
    pub fn centre(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
    pub fn contains_point(&self, p: (f64, f64)) -> bool {
        p.0 >= self.x && p.0 <= self.x + self.w && p.1 >= self.y && p.1 <= self.y + self.h
    }
    /// Rounded to the 10 px grid (fingerprints ignore sub-grid jitter).
    pub fn grid(&self) -> [i64; 4] {
        let r = |v: f64| (v / 10.0).round() as i64;
        [r(self.x), r(self.y), r(self.w), r(self.h)]
    }
}

/// Shapes of the non-UML blocks (C4 elements).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockShape {
    Person,
    System,
    ExternalSystem,
    Container,
    Datastore,
}

impl BlockShape {
    /// Lucid block class used to draw it.
    pub fn class_name(self) -> &'static str {
        match self {
            // C4PersonBlock has fixed text areas of its own; a plain block keeps our text exactly
            BlockShape::Person | BlockShape::System | BlockShape::ExternalSystem | BlockShape::Container => "ProcessBlock",
            BlockShape::Datastore => "DatabaseBlock",
        }
    }
    pub fn fill(self) -> &'static str {
        match self {
            BlockShape::Person => "#e9d5ff",
            BlockShape::System => "#bfdbfe",
            BlockShape::ExternalSystem => "#e5e7eb",
            BlockShape::Container => "#dbeafe",
            BlockShape::Datastore => "#d1fae5",
        }
    }
}

/// What an item is. Lines refer to their ends by sid (or `lucid:<id>` for colleague shapes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ItemKind {
    /// UML class box: `name` is the title compartment (`«stereotype»` line, name, `{abstract}`).
    Class {
        name: String,
        attributes: Vec<String>,
        operations: Vec<String>,
        #[serde(default)]
        interface: bool,
    },
    /// UML package; `body` lists what it contains (derived, informational).
    Package {
        name: String,
        #[serde(default)]
        body: String,
    },
    Actor { name: String },
    UseCase { name: String },
    Boundary { name: String },
    /// C4 element: text is `Name`, then `[Kind: technology]`, then the description.
    Block { shape: BlockShape, text: String },
    /// Typed UML relationship (`association`, `directedAssociation`, `generalization`, `realization`,
    /// `dependency`, `aggregation`, `composition`).
    Relationship {
        from: String,
        to: String,
        rel: String,
        #[serde(default)]
        label: String,
        #[serde(default)]
        from_mult: String,
        #[serde(default)]
        to_mult: String,
    },
    /// A plain arrow with a label (C4 links).
    Line {
        from: String,
        to: String,
        #[serde(default)]
        label: String,
    },
    /// A generated sequence diagram (Lucid sequence markup).
    Sequence { markup: String },
    /// Anything else on the page (colleagues' notes, text, shapes): never written by Spide.
    Other {
        #[serde(default)]
        class_name: String,
        #[serde(default)]
        text: String,
    },
}

impl ItemKind {
    pub fn is_line(&self) -> bool {
        matches!(self, ItemKind::Relationship { .. } | ItemKind::Line { .. })
    }
    /// Line ends (sids), if a line.
    pub fn ends(&self) -> Option<(&str, &str)> {
        match self {
            ItemKind::Relationship { from, to, .. } | ItemKind::Line { from, to, .. } => Some((from, to)),
            _ => None,
        }
    }
    /// The canonical content (no geometry) as a stable string: what `spide.hash` hashes.
    pub fn canonical(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
    pub fn content_hash(&self) -> String {
        crate::assess::fnv(&self.canonical())
    }
}

/// One item of a Lucid page (target or snapshot).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// Spide id (`class:Order`); None: a colleague's item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sid: Option<String>,
    /// Lucid item id (snapshots only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lucid_id: Option<String>,
    pub kind: ItemKind,
    /// Geometry (blocks); lines and sequence generators may have none in targets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bbox: Option<Bbox>,
    /// Fill colour Spide asks for (targets only; colours are never read back).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
}

impl Item {
    pub fn new(sid: impl Into<String>, kind: ItemKind, bbox: Option<Bbox>) -> Self {
        Item { sid: Some(sid.into()), lucid_id: None, kind, bbox, fill: None }
    }
}

/// The diagrams Spide syncs, one Lucid document each.
pub fn diagram_title(design: &str, key: &str) -> String {
    let what = match key {
        "system.context" => "System context".to_string(),
        "system.containers" => "Containers".to_string(),
        "class" => "Class diagram".to_string(),
        "domain" => "Domain model".to_string(),
        "package" => "Packages".to_string(),
        "usecase" => "Use cases".to_string(),
        k => match k.strip_prefix("sequence:") {
            Some(n) => format!("Sequence: {n}"),
            None => k.to_string(),
        },
    };
    format!("{design} · {what}")
}

/// A diagram as Spide wants it in Lucid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Target {
    /// `class`, `domain`, `package`, `usecase`, `system.context`, `system.containers`, `sequence:<name>`
    pub key: String,
    pub title: String,
    pub items: Vec<Item>,
}

impl Target {
    pub fn item(&self, sid: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.sid.as_deref() == Some(sid))
    }
}

/// Every diagram key the design has content for.
pub fn diagram_keys(d: &crate::Design) -> Vec<String> {
    let mut out = vec![];
    let sys = crate::derive::system_view(d, &crate::metrics::compute(d, false));
    if !sys.actors.is_empty() || !sys.systems.is_empty() || !sys.containers.is_empty() {
        out.push("system.context".to_string());
    }
    if !sys.containers.is_empty() {
        out.push("system.containers".to_string());
    }
    if d.live_classes().next().is_some() {
        out.push("class".to_string());
    }
    if !d.domain.is_empty() {
        out.push("domain".to_string());
    }
    if d.packages.iter().any(|p| p.status.is_live()) {
        out.push("package".to_string());
    }
    if d.use_cases.use_cases.iter().any(|u| u.status != crate::Status::Pruned) {
        out.push("usecase".to_string());
    }
    for s in d.sequences.iter().filter(|s| s.status != crate::Status::Pruned) {
        out.push(format!("sequence:{}", s.name));
    }
    out
}
