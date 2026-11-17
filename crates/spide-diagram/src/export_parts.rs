//! Shared by the structured exporters (Obsidian Canvas, Lucid): which model elements an export scene
//! draws, and where. Nodes come from the scene's hit boxes, whose hover keys name the model element
//! (`class:Order`, `package:domain`, `c4:API`, `actor:Member`, `usecase:Borrow`), so every exporter
//! places elements exactly where the PNG/SVG export draws them.

use crate::route::Pt;
use crate::scene::{HitShape, Scene};
use crate::views::{self, C4Level};
use crate::visuals::{layer_color, Measure, Rgb};
use spide_core::derive::{PackageNode, SystemView};
use spide_core::metrics::PackageDep;
use spide_core::model::{Class, Design, Package, Relation, Sequence, Status};
use std::collections::HashSet;

/// The model behind an export scene (where nodes and edges come from).
pub enum Source<'a> {
    /// Class diagram or domain model (`conceptual`: generalization reads "is a").
    Classes { classes: &'a [Class], relations: &'a [Relation], packages: &'a [Package], package_deps: &'a [PackageDep], conceptual: bool },
    Packages { packages: &'a [PackageNode], deps: &'a [PackageDep] },
    System { view: &'a SystemView, level: C4Level },
    UseCases { design: &'a Design },
    Sequence { seq: &'a Sequence, is_actor: &'a dyn Fn(&str) -> bool },
}

/// `[x1, y1, x2, y2]` in scene coordinates.
pub type R4 = [f64; 4];

pub fn area(r: R4) -> f64 {
    (r[2] - r[0]) * (r[3] - r[1])
}

pub fn contains(r: R4, p: Pt, tol: f64) -> bool {
    p[0] >= r[0] - tol && p[0] <= r[2] + tol && p[1] >= r[1] - tol && p[1] <= r[3] + tol
}

pub fn centre(r: R4) -> Pt {
    [(r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0]
}

/// One drawn element: its hover key, box, and whether it frames others (package frame, system boundary).
#[derive(Debug, Clone, PartialEq)]
pub struct PartNode {
    pub id: String,
    pub r: R4,
    pub group: bool,
    pub label: Option<String>,
    pub color: Option<Rgb>,
}

/// C4 element colours (as the system view draws them).
pub fn c4_color(sv: &SystemView, id: &str) -> Rgb {
    if sv.actors.iter().any(|a| a == id) {
        Rgb::hex(0xc084fc)
    } else if sv.systems.iter().any(|x| x.name == id) {
        Rgb::hex(0x8a93a6)
    } else if let Some(k) = sv.containers.iter().find(|k| k.name == id) {
        if k.datastore {
            Rgb::hex(0x34d399)
        } else {
            Rgb::hex(0x60a5fa)
        }
    } else {
        Rgb::hex(0x7aa2ff)
    }
}

/// The elements a (non-sequence) scene draws, with ghosts and package frames as requested.
pub fn scene_nodes(scene: &Scene, m: &dyn Measure, src: &Source, ghosts: bool, packages: bool) -> Vec<PartNode> {
    let mut nodes: Vec<PartNode> = vec![];
    let mut seen = HashSet::new();
    for h in &scene.hits {
        let (HitShape::Rect { x1, y1, x2, y2 }, Some(hover)) = (&h.shape, &h.hover) else { continue };
        let (kind, name) = hover.split_once(':').unwrap_or(("", ""));
        let ok = match kind {
            "class" => h.node.is_some(),
            "package" | "c4" | "actor" | "usecase" => true,
            _ => false,
        };
        if !ok || !seen.insert(hover.clone()) {
            continue;
        }
        let keep = match (src, kind) {
            (Source::Classes { classes, .. }, "class") => classes.iter().find(|c| c.name == name).is_none_or(|c| ghosts || c.status != Status::Proposed),
            (Source::Classes { packages: pk, .. }, "package") => packages && pk.iter().find(|p| p.name == name).is_none_or(|p| ghosts || p.status != Status::Proposed),
            (Source::Packages { packages: pk, .. }, "package") => pk.iter().find(|p| p.name == name).is_none_or(|p| ghosts || p.status != Status::Proposed),
            (Source::System { view, .. }, "c4") => {
                let proposed = view.containers.iter().any(|k| k.name == name && k.status == Status::Proposed) || view.systems.iter().any(|x| x.name == name && x.status == Status::Proposed);
                ghosts || !proposed
            }
            (Source::UseCases { .. }, "actor" | "usecase") => true,
            _ => false,
        };
        if !keep {
            continue;
        }
        let color = match (src, kind) {
            (Source::Classes { classes, packages: pk, .. }, "class") => classes.iter().find(|c| c.name == name).and_then(|c| c.package.as_ref()).and_then(|p| pk.iter().find(|x| &x.name == p)).and_then(|p| p.layer).map(layer_color),
            (Source::Classes { packages: pk, .. }, "package") => pk.iter().find(|p| p.name == name).and_then(|p| p.layer).map(layer_color),
            (Source::Packages { packages: pk, .. }, "package") => pk.iter().find(|p| p.name == name).and_then(|p| p.layer).map(layer_color),
            (Source::System { view, .. }, "c4") => Some(c4_color(view, name)),
            _ => None,
        };
        nodes.push(PartNode { id: hover.clone(), r: [*x1, *y1, *x2, *y2], group: false, label: None, color });
    }
    // use case diagram: the system boundary
    if let Source::UseCases { design } = src {
        let ucm = &design.use_cases;
        let actors: Vec<_> = ucm.actors.iter().filter(|a| a.status != Status::Pruned).collect();
        let ucs: Vec<_> = ucm.use_cases.iter().filter(|u| u.status != Status::Pruned).collect();
        if !ucs.is_empty() {
            let l = views::layout_use_cases(m, &actors, &ucs);
            let [bx, by, bw, bh] = l.boundary;
            nodes.push(PartNode { id: "system".into(), r: [bx, by, bx + bw, by + bh], group: true, label: Some(ucm.system.clone().unwrap_or_else(|| design.name.clone())), color: None });
        }
    }
    // a package frame around classes is a group; at the package level of detail it is the card itself
    let leaf_centres: Vec<Pt> = nodes.iter().filter(|n| !n.id.starts_with("package:") && !n.group).map(|n| centre(n.r)).collect();
    if matches!(src, Source::Classes { .. }) {
        for n in nodes.iter_mut().filter(|n| n.id.starts_with("package:")) {
            if leaf_centres.iter().any(|c| contains(n.r, *c, 0.0)) {
                n.group = true;
                n.label = Some(n.id["package:".len()..].to_string());
            }
        }
    }
    nodes
}
