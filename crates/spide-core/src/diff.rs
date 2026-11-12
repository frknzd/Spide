//! Diff two designs into ops (used when design.json is edited externally).

use crate::model::*;
use crate::ops::Op;

fn live_classes(d: &Design) -> Vec<&Class> {
    d.classes.iter().filter(|c| c.status.is_live()).collect()
}

fn strip_attr(a: &Attribute) -> Attribute {
    Attribute { status: Status::Accepted, proposal: None, origin: None, ..a.clone() }
}
fn strip_method(m: &Method) -> Method {
    Method { status: Status::Accepted, proposal: None, origin: None, ..m.clone() }
}

pub fn diff(old: &Design, new: &Design) -> Vec<Op> {
    let mut adds_pkg = vec![];
    let mut adds_cls = vec![];
    let mut members = vec![];
    let mut adds_rel = vec![];
    let mut removes = vec![];

    // packages
    for p in new.packages.iter().filter(|p| p.status.is_live()) {
        match old.packages.iter().find(|o| o.name == p.name && o.status.is_live()) {
            None => adds_pkg.push(Op::AddPackage { name: p.name.clone(), layer: p.layer, description: p.description.clone() }),
            Some(o) if o.layer != p.layer => adds_pkg.push(Op::SetLayer { package: p.name.clone(), layer: p.layer }),
            _ => {}
        }
    }

    let oc = live_classes(old);
    let nc = live_classes(new);
    for c in &nc {
        match oc.iter().find(|o| o.name == c.name) {
            None => adds_cls.push(Op::AddClass {
                name: c.name.clone(),
                stereotype: c.stereotype.clone(),
                package: c.package.clone(),
                is_abstract: c.is_abstract,
                attributes: c.attributes.iter().filter(|a| a.status.is_live()).map(strip_attr).collect(),
                methods: c.methods.iter().filter(|m| m.status.is_live()).map(strip_method).collect(),
                responsibilities: c.responsibilities.clone(),
                rationale: c.rationale.clone(),
                aggregate_root: c.aggregate_root,
            }),
            Some(o) => {
                if o.stereotype != c.stereotype
                    || o.package != c.package
                    || o.is_abstract != c.is_abstract
                    || o.responsibilities != c.responsibilities
                    || (o.rationale != c.rationale && c.rationale.is_some())
                    || o.aggregate_root != c.aggregate_root
                {
                    members.push(Op::UpdateClass {
                        name: c.name.clone(),
                        stereotype: (o.stereotype != c.stereotype).then(|| c.stereotype.clone().unwrap_or_default()),
                        package: (o.package != c.package).then(|| c.package.clone().unwrap_or_default()),
                        is_abstract: (o.is_abstract != c.is_abstract).then_some(c.is_abstract),
                        responsibilities: (o.responsibilities != c.responsibilities).then(|| c.responsibilities.clone()),
                        rationale: if o.rationale != c.rationale { c.rationale.clone() } else { None },
                        aggregate_root: (o.aggregate_root != c.aggregate_root).then_some(c.aggregate_root),
                    });
                }
                let oa: Vec<&Attribute> = o.attributes.iter().filter(|a| a.status.is_live()).collect();
                let na: Vec<&Attribute> = c.attributes.iter().filter(|a| a.status.is_live()).collect();
                for a in &na {
                    match oa.iter().find(|x| x.name == a.name) {
                        None => members.push(Op::AddAttribute { class: c.name.clone(), attribute: strip_attr(a) }),
                        Some(x) if strip_attr(x) != strip_attr(a) => members.push(Op::UpdateAttribute {
                            class: c.name.clone(),
                            name: a.name.clone(),
                            attribute: strip_attr(a),
                        }),
                        _ => {}
                    }
                }
                for a in oa.iter().filter(|x| !na.iter().any(|a| a.name == x.name)) {
                    removes.push(Op::RemoveAttribute { class: c.name.clone(), name: a.name.clone() });
                }
                let om: Vec<&Method> = o.methods.iter().filter(|m| m.status.is_live()).collect();
                let nm: Vec<&Method> = c.methods.iter().filter(|m| m.status.is_live()).collect();
                for m in &nm {
                    match om.iter().find(|x| x.name == m.name) {
                        None => members.push(Op::AddMethod { class: c.name.clone(), method: strip_method(m) }),
                        Some(x) if strip_method(x) != strip_method(m) => members.push(Op::UpdateMethod {
                            class: c.name.clone(),
                            name: m.name.clone(),
                            method: strip_method(m),
                        }),
                        _ => {}
                    }
                }
                for m in om.iter().filter(|x| !nm.iter().any(|m| m.name == x.name)) {
                    removes.push(Op::RemoveMethod { class: c.name.clone(), name: m.name.clone() });
                }
            }
        }
    }

    let or: Vec<&Relation> = old.live_relations().collect();
    let nr: Vec<&Relation> = new.live_relations().collect();
    let mut rel_removes = vec![];
    for r in &nr {
        match or.iter().find(|o| o.from == r.from && o.to == r.to && o.kind == r.kind) {
            None => adds_rel.push(Op::AddRelation {
                from: r.from.clone(),
                to: r.to.clone(),
                kind: r.kind,
                label: r.label.clone(),
                from_mult: r.from_mult.clone(),
                to_mult: r.to_mult.clone(),
                directed: Some(r.directed),
                rationale: r.rationale.clone(),
                interaction: Some(r.interaction),
            }),
            Some(o)
                if o.label != r.label
                    || o.from_mult != r.from_mult
                    || o.to_mult != r.to_mult
                    || o.directed != r.directed
                    || o.interaction != r.interaction =>
            {
                adds_rel.push(Op::UpdateRelation {
                    from: r.from.clone(),
                    to: r.to.clone(),
                    kind: r.kind,
                    label: Some(r.label.clone().unwrap_or_default()),
                    from_mult: Some(r.from_mult.clone().unwrap_or_default()),
                    to_mult: Some(r.to_mult.clone().unwrap_or_default()),
                    directed: Some(r.directed),
                    interaction: Some(r.interaction),
                })
            }
            _ => {}
        }
    }
    let removed_classes: Vec<&str> =
        oc.iter().filter(|o| !nc.iter().any(|c| c.name == o.name)).map(|o| o.name.as_str()).collect();
    for o in &or {
        let gone = !nr.iter().any(|r| r.from == o.from && r.to == o.to && r.kind == o.kind);
        let via_class = removed_classes.contains(&o.from.as_str()) || removed_classes.contains(&o.to.as_str());
        if gone && !via_class {
            rel_removes.push(Op::RemoveRelation { from: o.from.clone(), to: o.to.clone(), kind: Some(o.kind) });
        }
    }
    for name in removed_classes {
        removes.push(Op::RemoveClass { name: name.to_string() });
    }
    for p in old.packages.iter().filter(|p| p.status.is_live()) {
        if !new.packages.iter().any(|n| n.name == p.name && n.status.is_live()) {
            removes.push(Op::RemovePackage { name: p.name.clone() });
        }
    }

    // forces (the brief)
    let mut forces = vec![];
    let of: Vec<&Force> = old.live_forces().collect();
    for f in new.live_forces() {
        match of.iter().find(|o| o.name == f.name) {
            None => forces.push(Op::AddForce {
                name: f.name.clone(),
                kind: f.kind,
                description: f.description.clone(),
                priority: Some(f.priority),
            }),
            Some(o) if o.kind != f.kind || o.description != f.description || o.priority != f.priority => {
                forces.push(Op::UpdateForce {
                    name: f.name.clone(),
                    rename: None,
                    kind: Some(f.kind),
                    description: Some(f.description.clone().unwrap_or_default()),
                    priority: Some(f.priority),
                })
            }
            _ => {}
        }
    }
    for o in &of {
        if new.force(&o.name).is_none_or(|f| !f.status.is_live()) {
            forces.push(Op::RemoveForce { name: o.name.clone() });
        }
    }

    for c in new.change_scenarios.iter().filter(|c| c.status.is_live()) {
        if !old.change_scenarios.iter().any(|o| o.name == c.name && o.status.is_live()) {
            forces.push(Op::AddChangeScenario { name: c.name.clone(), description: c.description.clone() });
        }
    }
    for o in old.change_scenarios.iter().filter(|c| c.status.is_live()) {
        if !new.change_scenarios.iter().any(|c| c.name == o.name && c.status.is_live()) {
            forces.push(Op::RemoveChangeScenario { name: o.name.clone() });
        }
    }

    // C4: systems, containers, links, package → container
    let mut c4_adds = vec![];
    let mut c4_after_pkgs = vec![];
    let mut c4_removes = vec![];
    for x in new.systems.iter().filter(|x| x.status.is_live()) {
        if !old.systems.iter().any(|o| o.name == x.name && o.status.is_live()) {
            c4_adds.push(Op::AddSystem { name: x.name.clone(), description: x.description.clone() });
        }
    }
    for o in old.systems.iter().filter(|x| x.status.is_live()) {
        if !new.systems.iter().any(|x| x.name == o.name && x.status.is_live()) {
            c4_removes.push(Op::RemoveSystem { name: o.name.clone() });
        }
    }
    for k in new.containers.iter().filter(|x| x.status.is_live()) {
        match old.containers.iter().find(|o| o.name == k.name && o.status.is_live()) {
            None => c4_adds.push(Op::AddContainer {
                name: k.name.clone(),
                technology: k.technology.clone(),
                description: k.description.clone(),
                replicas: Some(k.replicas),
                datastore: k.datastore,
            }),
            Some(o) if o.technology != k.technology || o.description != k.description || o.replicas != k.replicas || o.datastore != k.datastore => {
                c4_adds.push(Op::UpdateContainer {
                    name: k.name.clone(),
                    technology: Some(k.technology.clone().unwrap_or_default()),
                    description: Some(k.description.clone().unwrap_or_default()),
                    replicas: Some(k.replicas),
                    datastore: Some(k.datastore),
                })
            }
            _ => {}
        }
    }
    for o in old.containers.iter().filter(|x| x.status.is_live()) {
        if !new.containers.iter().any(|x| x.name == o.name && x.status.is_live()) {
            c4_removes.push(Op::RemoveContainer { name: o.name.clone() });
        }
    }
    for p in new.packages.iter().filter(|p| p.status.is_live()) {
        let before = old.packages.iter().find(|o| o.name == p.name && o.status.is_live()).and_then(|o| o.container.clone());
        if before != p.container {
            c4_after_pkgs.push(Op::AssignContainer { package: p.name.clone(), container: p.container.clone() });
        }
    }
    for l in new.links.iter().filter(|x| x.status.is_live()) {
        if !old.links.iter().any(|o| o.from == l.from && o.to == l.to && o.status.is_live()) {
            c4_after_pkgs.push(Op::AddLink {
                from: l.from.clone(),
                to: l.to.clone(),
                interaction: Some(l.interaction),
                protocol: l.protocol.clone(),
                description: l.description.clone(),
            });
        }
    }
    for o in old.links.iter().filter(|x| x.status.is_live()) {
        if !new.links.iter().any(|l| l.from == o.from && l.to == o.to && l.status.is_live()) {
            c4_removes.insert(0, Op::RemoveLink { from: o.from.clone(), to: o.to.clone() });
        }
    }

    let mut out = forces;
    out.extend(c4_adds);
    out.extend(adds_pkg);
    out.extend(c4_after_pkgs);
    out.extend(rel_removes);
    out.extend(adds_cls);
    out.extend(members);
    out.extend(adds_rel);
    out.extend(removes);
    out.extend(c4_removes);
    out
}
