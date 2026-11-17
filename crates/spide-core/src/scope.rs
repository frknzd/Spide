//! Package-backed class views: exact-package contents and one-hop boundary neighbors.
//! Projection never recomputes health or mutates the original design/layout.
use crate::{metrics::Metrics, model::*};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageScope {
    pub package: String,
    pub primary: BTreeSet<String>,
    pub classes: BTreeSet<String>,
}

impl PackageScope {
    /// Resolve direct incoming/outgoing neighbors from the existing dependency graph.
    pub fn new(d: &Design, m: &Metrics, package: &str, ghosts: bool) -> Result<Self, String> {
        let live = |s: Status| s.is_live() || (ghosts && s == Status::Proposed);
        if !d.packages.iter().any(|p| p.name == package && live(p.status)) {
            return Err(format!("unknown or hidden package: {package}"));
        }
        let eligible: BTreeSet<String> = d.classes.iter().filter(|c| live(c.status)).map(|c| c.name.clone()).collect();
        let primary: BTreeSet<String> = d.classes.iter().filter(|c| live(c.status) && c.package.as_deref() == Some(package)).map(|c| c.name.clone()).collect();
        let mut classes = primary.clone();
        let edges = d.relations.iter().filter(|r| live(r.status)).map(|r| (&r.from, &r.to))
            .chain(m.implicit_edges.iter().map(|e| (&e.from, &e.to)));
        for (from, to) in edges {
            if eligible.contains(from) && eligible.contains(to) && (primary.contains(from) || primary.contains(to)) {
                classes.insert(from.clone());
                classes.insert(to.clone());
            }
        }
        Ok(Self { package: package.into(), primary, classes })
    }

    /// Context-to-context edges are not part of the selected package's boundary.
    pub fn includes_edge(&self, from: &str, to: &str) -> bool {
        self.classes.contains(from) && self.classes.contains(to) && (self.primary.contains(from) || self.primary.contains(to))
    }

    /// Owned model projection shared by scene and text exporters. Saved full layouts are excluded.
    pub fn design(&self, d: &Design) -> Design {
        let mut out = d.clone();
        out.classes.retain(|c| self.classes.contains(&c.name));
        out.relations.retain(|r| self.includes_edge(&r.from, &r.to));
        let packages: BTreeSet<&str> = out.classes.iter().filter_map(|c| c.package.as_deref()).chain(std::iter::once(self.package.as_str())).collect();
        out.packages.retain(|p| packages.contains(p.name.as_str()));
        out.layout.clear();
        out
    }

    /// Preserve full-project health while limiting graph edges used by scoped rendering.
    pub fn metrics(&self, m: &Metrics, d: &Design) -> Metrics {
        let mut out = m.clone();
        out.implicit_edges.retain(|e| self.includes_edge(&e.from, &e.to));
        out.overlap.classes.retain(|e| self.includes_edge(&e.a, &e.b));
        out.overlap.claims.retain(|e| self.includes_edge(e.method.split('.').next().unwrap_or(""), &e.owner));
        let packages: BTreeSet<&str> = d.classes.iter().filter(|c| self.classes.contains(&c.name)).filter_map(|c| c.package.as_deref()).chain(std::iter::once(self.package.as_str())).collect();
        out.package_deps.retain(|e| packages.contains(e.from.as_str()) && packages.contains(e.to.as_str()) && (e.from == self.package || e.to == self.package));
        out
    }
}

/// Collision-free scope suffix for cache identities, filenames and job keys (UTF-8 hex).
pub fn package_key(package: &str) -> String {
    package.as_bytes().iter().map(|b| format!("{b:02x}")).collect()
}
