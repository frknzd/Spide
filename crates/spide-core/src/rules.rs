//! Explicit, project-specific architecture rules: a style preset (which layer may depend on which),
//! custom forbidden dependencies, forbidden types per layer, and metric thresholds.

use crate::model::Layer;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    /// Clean architecture: ui/infrastructure → application → domain (dependencies point inward).
    #[default]
    Clean,
    /// Ports & adapters: like clean, and adapters (ui, infrastructure) never depend on each other.
    Hexagonal,
    /// Classic layering: ui → application → domain → infrastructure (only downward).
    Layered,
    /// Modules `<module>.<layer>`: clean layering inside a module; other modules only via `<module>.api`.
    ModularMonolith,
    /// No layering rules.
    None,
}

impl Preset {
    pub const ALL: [Preset; 5] = [Preset::Clean, Preset::Hexagonal, Preset::Layered, Preset::ModularMonolith, Preset::None];

    pub fn as_str(self) -> &'static str {
        match self {
            Preset::Clean => "clean",
            Preset::Hexagonal => "hexagonal",
            Preset::Layered => "layered",
            Preset::ModularMonolith => "modular_monolith",
            Preset::None => "none",
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Preset::Clean => "ui/infrastructure → application → domain; dependencies point inward",
            Preset::Hexagonal => "adapters (ui, infrastructure) → application ports → domain; adapters never depend on each other",
            Preset::Layered => "ui → application → domain → infrastructure; only downward",
            Preset::ModularMonolith => "packages <module>.<layer>; clean layering inside a module, other modules only via <module>.api",
            Preset::None => "no layering rules",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Thresholds {
    pub god_members: usize,
    pub god_responsibilities: usize,
    pub high_fan_out: usize,
    /// Sync messages per sequence before it counts as chatty.
    pub max_scenario_hops: usize,
    /// Nonblank, non-comment Java body lines before a long-method warning.
    pub max_method_lines: u32,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds { god_members: 16, god_responsibilities: 4, high_fan_out: 6, max_scenario_hops: 12, max_method_lines: 30 }
    }
}

/// `from` must not depend on `to`. Patterns are globs (`*`, `?`) matched against a class name
/// or its package name (`domain`, `*Controller`, `orders.*`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DepRule {
    pub from: String,
    pub to: String,
    pub reason: Option<String>,
}

impl DepRule {
    pub fn id(&self) -> String {
        format!("forbid {} → {}", self.from, self.to)
    }
}

/// Types matching `pattern` must not appear in classes of `layer` (e.g. `Http*` in domain).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TypeRule {
    pub layer: Layer,
    pub pattern: String,
    pub reason: Option<String>,
}

impl Default for TypeRule {
    fn default() -> Self {
        TypeRule { layer: Layer::Domain, pattern: String::new(), reason: None }
    }
}

impl TypeRule {
    pub fn id(&self) -> String {
        format!("no {} in {}", self.pattern, layer_name(self.layer))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Rules {
    pub preset: Preset,
    pub forbidden: Vec<DepRule>,
    pub forbidden_types: Vec<TypeRule>,
    pub thresholds: Thresholds,
}

/// Why a dependency is not allowed.
#[derive(Debug, Clone, PartialEq)]
pub struct Violation {
    pub rule: String,
    pub explain: String,
    pub fix_hint: String,
}

fn layer_name(l: Layer) -> &'static str {
    match l {
        Layer::Ui => "ui",
        Layer::Infrastructure => "infrastructure",
        Layer::Application => "application",
        Layer::Domain => "domain",
    }
}

/// Is a dependency from a class in layer `from` to one in layer `to` allowed by the preset?
pub fn allowed(from: Layer, to: Layer, preset: Preset) -> Result<(), Violation> {
    use Layer::*;
    let (f, t) = (layer_name(from), layer_name(to));
    let ring = |l: Layer| match l {
        Ui | Infrastructure => 0,
        Application => 1,
        Domain => 2,
    };
    match preset {
        Preset::None => Ok(()),
        Preset::Clean | Preset::ModularMonolith => {
            if ring(from) > ring(to) {
                Err(Violation {
                    rule: format!("{}: inward only", preset.as_str()),
                    explain: format!("{f} is an inner layer; it must not depend on the outer {t} layer"),
                    fix_hint: format!("invert the dependency: define an interface in {f} and implement it in {t}"),
                })
            } else {
                Ok(())
            }
        }
        Preset::Hexagonal => {
            if ring(from) > ring(to) {
                Err(Violation {
                    rule: "hexagonal: inward only".into(),
                    explain: format!("{f} is inside the hexagon; it must not depend on the {t} adapter"),
                    fix_hint: format!("add a port (interface) in {f} and let the {t} adapter implement it"),
                })
            } else if ring(from) == 0 && ring(to) == 0 && from != to {
                Err(Violation {
                    rule: "hexagonal: adapters are independent".into(),
                    explain: format!("the {f} adapter must not depend on the {t} adapter directly"),
                    fix_hint: "route the interaction through an application port".into(),
                })
            } else {
                Ok(())
            }
        }
        Preset::Layered => {
            let rank = |l: Layer| match l {
                Ui => 0,
                Application => 1,
                Domain => 2,
                Infrastructure => 3,
            };
            if rank(from) > rank(to) {
                Err(Violation {
                    rule: "layered: downward only".into(),
                    explain: format!("{f} is below {t}; layers may only depend on layers beneath them"),
                    fix_hint: format!("move the logic down into {f}, or let {t} call {f} instead (callback/observer)"),
                })
            } else {
                Ok(())
            }
        }
    }
}

/// Module of a package in a modular monolith (`orders.domain` → `orders`).
pub fn module_of(package: &str) -> Option<&str> {
    package.split_once(['.', '/']).map(|(m, _)| m)
}

/// Modular monolith: cross-module dependencies must target `<module>.api`.
pub fn module_violation(from_pkg: &str, to_pkg: &str) -> Option<Violation> {
    let (Some(a), Some(b)) = (module_of(from_pkg), module_of(to_pkg)) else { return None };
    if a == b {
        return None;
    }
    let target_is_api = to_pkg.rsplit(['.', '/']).next() == Some("api");
    (!target_is_api).then(|| Violation {
        rule: "modular monolith: modules talk via api".into(),
        explain: format!("module {a} reaches into {to_pkg}; other modules may only use {b}.api"),
        fix_hint: format!("expose what {a} needs through an interface or DTO in {b}.api"),
    })
}

/// Glob match with `*` (any run) and `?` (one char); case-sensitive.
pub fn glob(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti, mut star, mut mark) = (0, 0, None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

/// Does a pattern match a class (by name or by its package)?
pub fn matches_class(pattern: &str, class: &str, package: Option<&str>) -> bool {
    glob(pattern, class) || package.is_some_and(|p| glob(pattern, p))
}

/// Custom forbidden-dependency rules violated by `from → to`.
pub fn forbidden_by<'a>(
    rules: &'a Rules,
    from: &str,
    from_pkg: Option<&str>,
    to: &str,
    to_pkg: Option<&str>,
) -> impl Iterator<Item = &'a DepRule> + 'a {
    let (from, to) = (from.to_string(), to.to_string());
    let (fp, tp) = (from_pkg.map(String::from), to_pkg.map(String::from));
    rules
        .forbidden
        .iter()
        .filter(move |r| matches_class(&r.from, &from, fp.as_deref()) && matches_class(&r.to, &to, tp.as_deref()))
}

/// Type tokens of a type expression (`Map<String, HttpRequest>` → String, HttpRequest).
pub fn type_tokens(ty: &str) -> impl Iterator<Item = &str> {
    ty.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.')).filter(|t| !t.is_empty())
}
