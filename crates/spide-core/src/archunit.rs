//! Generate an ArchUnit (JUnit 5) test that enforces the design's architecture rules in real code.

use crate::model::*;
use crate::rules::{self, Preset};
use std::collections::BTreeMap;
use std::fmt::Write;

fn layer_title(l: Layer) -> &'static str {
    match l {
        Layer::Ui => "Ui",
        Layer::Application => "Application",
        Layer::Domain => "Domain",
        Layer::Infrastructure => "Infrastructure",
    }
}

const LAYERS: [Layer; 4] = [Layer::Ui, Layer::Application, Layer::Domain, Layer::Infrastructure];

/// ArchUnit package identifier for a design package (`domain` → `org.acme.domain..` or `..domain..`).
fn pkg_id(root: Option<&str>, pkg: &str) -> String {
    match root {
        Some(r) => format!("{r}.{pkg}.."),
        None => format!("..{pkg}.."),
    }
}

fn java_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Glob on a simple class name → Java regex on the fully qualified name.
fn class_regex(glob: &str) -> String {
    let mut re = String::from(".*\\.");
    for c in glob.chars() {
        match c {
            '*' => re.push_str("[^.]*"),
            '?' => re.push_str("[^.]"),
            '.' | '$' | '(' | ')' | '[' | ']' | '{' | '}' | '+' | '^' | '|' | '\\' => {
                re.push('\\');
                re.push(c);
            }
            c => re.push(c),
        }
    }
    re
}

/// A rule pattern names packages if it is lower-case (`domain`, `orders.*`), else classes (`*Controller`).
fn is_package_pattern(p: &str) -> bool {
    p.chars().find(|c| c.is_alphabetic()).is_some_and(|c| c.is_lowercase())
}

fn predicate(root: Option<&str>, pattern: &str) -> String {
    if is_package_pattern(pattern) {
        format!("resideInAPackage({})", java_str(&pkg_id(root, pattern)))
    } else {
        format!("haveNameMatching({})", java_str(&class_regex(pattern)))
    }
}

fn ident(s: &str) -> String {
    let mut out = String::new();
    let mut up = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(if up { c.to_ascii_uppercase() } else { c });
            up = false;
        } else {
            up = !out.is_empty();
        }
    }
    if out.chars().next().is_none_or(|c| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

pub fn archunit_test(d: &Design) -> String {
    let root = d.settings.code_root.as_deref().filter(|r| !r.trim().is_empty());
    let r = &d.rules;
    let packages: Vec<&Package> = d.packages.iter().filter(|p| p.status.is_live()).collect();
    let mut by_layer: BTreeMap<u8, (Layer, Vec<String>)> = BTreeMap::new();
    for p in &packages {
        if let Some(l) = p.layer {
            let k = LAYERS.iter().position(|x| *x == l).unwrap() as u8;
            by_layer.entry(k).or_insert((l, vec![])).1.push(pkg_id(root, &p.name));
        }
    }

    let mut rules_src: Vec<String> = vec![];
    let mut needs = std::collections::BTreeSet::new();

    // layering per preset
    if r.preset != Preset::None && by_layer.len() >= 2 {
        let mut s = String::from("        layeredArchitecture().consideringOnlyDependenciesInLayers()\n");
        for (l, pkgs) in by_layer.values() {
            let ids: Vec<String> = pkgs.iter().map(|p| java_str(p)).collect();
            let _ = writeln!(s, "            .layer({}).definedBy({})", java_str(layer_title(*l)), ids.join(", "));
        }
        for (l, _) in by_layer.values() {
            let callers: Vec<String> = by_layer
                .values()
                .filter(|(f, _)| f != l && rules::allowed(*f, *l, r.preset).is_ok())
                .map(|(f, _)| java_str(layer_title(*f)))
                .collect();
            if callers.is_empty() {
                let _ = writeln!(s, "            .whereLayer({}).mayNotBeAccessedByAnyLayer()", java_str(layer_title(*l)));
            } else {
                let _ = writeln!(s, "            .whereLayer({}).mayOnlyBeAccessedByLayers({})", java_str(layer_title(*l)), callers.join(", "));
            }
        }
        s.push_str(&format!("            .because({});", java_str(r.preset.describe())));
        needs.insert("import static com.tngtech.archunit.library.Architectures.layeredArchitecture;");
        rules_src.push(format!("    /** {} preset. */\n    @ArchTest\n    static final ArchRule layers =\n{s}", r.preset.as_str()));
    }

    // modular monolith: other modules only through <module>.api
    if r.preset == Preset::ModularMonolith {
        let mut modules: Vec<&str> = packages.iter().filter_map(|p| rules::module_of(&p.name)).collect();
        modules.sort();
        modules.dedup();
        for a in &modules {
            for b in modules.iter().filter(|b| *b != a) {
                needs.insert("import static com.tngtech.archunit.base.DescribedPredicate.not;");
                needs.insert("import static com.tngtech.archunit.core.domain.JavaClass.Predicates.resideInAPackage;");
                rules_src.push(format!(
                    "    @ArchTest\n    static final ArchRule {}_uses_{}_only_via_api =\n        noClasses().that().resideInAPackage({})\n            .should().dependOnClassesThat(resideInAPackage({}).and(not(resideInAPackage({}))))\n            .because({});",
                    ident(a),
                    ident(b),
                    java_str(&pkg_id(root, a)),
                    java_str(&pkg_id(root, b)),
                    java_str(&pkg_id(root, &format!("{b}.api"))),
                    java_str(&format!("modules talk via {b}.api"))
                ));
            }
        }
    }

    // custom forbidden dependencies
    for (i, dr) in r.forbidden.iter().enumerate() {
        let because = dr.reason.clone().unwrap_or_else(|| dr.id());
        rules_src.push(format!(
            "    /** {} */\n    @ArchTest\n    static final ArchRule forbidden_{} =\n        noClasses().that().{}\n            .should().dependOnClassesThat().{}\n            .because({});",
            dr.id().replace("*/", "* /"),
            i + 1,
            predicate(root, &dr.from),
            predicate(root, &dr.to),
            java_str(&because)
        ));
    }

    // forbidden types per layer
    for (i, tr) in r.forbidden_types.iter().enumerate() {
        let Some((_, pkgs)) = by_layer.values().find(|(l, _)| *l == tr.layer) else { continue };
        let ids: Vec<String> = pkgs.iter().map(|p| java_str(p)).collect();
        rules_src.push(format!(
            "    @ArchTest\n    static final ArchRule no_{}_in_{} =\n        noClasses().that().resideInAnyPackage({})\n            .should().dependOnClassesThat().haveNameMatching({})\n            .because({});",
            ident(&tr.pattern.replace('*', "")).to_lowercase(),
            layer_title(tr.layer).to_lowercase(),
            ids.join(", "),
            java_str(&class_regex(&tr.pattern)),
            java_str(&tr.reason.clone().unwrap_or_else(|| tr.id()))
        ));
        let _ = i;
    }

    // no package cycles
    needs.insert("import static com.tngtech.archunit.library.dependencies.SlicesRuleDefinition.slices;");
    let slice = match root {
        Some(r) => format!("{r}.(*).."),
        None => "..(*)..".to_string(),
    };
    rules_src.push(format!(
        "    @ArchTest\n    static final ArchRule no_package_cycles =\n        slices().matching({}).should().beFreeOfCycles();",
        java_str(&slice)
    ));
    if !r.forbidden.is_empty() || !r.forbidden_types.is_empty() || r.preset == Preset::ModularMonolith {
        needs.insert("import static com.tngtech.archunit.lang.syntax.ArchRuleDefinition.noClasses;");
    }

    let mut o = String::new();
    if let Some(r) = root {
        let _ = writeln!(o, "package {r};\n");
    }
    let _ = writeln!(o, "import com.tngtech.archunit.junit.AnalyzeClasses;");
    let _ = writeln!(o, "import com.tngtech.archunit.junit.ArchTest;");
    let _ = writeln!(o, "import com.tngtech.archunit.lang.ArchRule;");
    let _ = writeln!(o);
    for n in &needs {
        let _ = writeln!(o, "{n}");
    }
    let _ = writeln!(o, "\n/**\n * Architecture rules of the Spide design \"{}\" ({} preset).\n * Generated by Spide — regenerate with /export archunit instead of editing.\n */", d.name.replace("*/", "* /"), r.preset.as_str());
    let analyze = root.map(|r| format!("@AnalyzeClasses(packages = {})", java_str(r))).unwrap_or_else(|| "@AnalyzeClasses".into());
    let _ = writeln!(o, "{analyze}");
    let _ = writeln!(o, "public class ArchitectureTest {{\n");
    let _ = writeln!(o, "{}", rules_src.join("\n\n"));
    let _ = writeln!(o, "}}");
    o
}
