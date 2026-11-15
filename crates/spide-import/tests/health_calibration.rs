//! Cross-language calibration: for every language, a tangled function (deep nesting, many
//! branches and parameters) must score worse than the same behaviour split into small functions,
//! judged by that language's profile; and the same small shop written in six languages must land
//! in the same health band. Orderings and bands, not exact numbers.

use spide_core::metrics::compute;
use spide_import::import_project;
use std::path::{Path, PathBuf};

fn params(lang: &str, n: usize) -> String {
    (0..n)
        .map(|i| match lang {
            "rust" => format!("a{i}: i32"),
            "typescript" => format!("a{i}: number"),
            "java" | "c" | "cpp" => format!("int a{i}"),
            _ => format!("a{i}"),
        })
        .collect::<Vec<_>>()
        .join(if lang == "clojure" { " " } else { ", " })
}

/// Nested conditionals `depth` deep.
fn nested(lang: &str, depth: usize, i: usize, indent: usize) -> String {
    let pad = "    ".repeat(indent);
    if depth == 0 {
        return match lang {
            "python" => format!("{pad}return a0\n"),
            "clojure" => "a0".into(),
            "rust" => format!("{pad}a0\n"),
            _ => format!("{pad}return a0;\n"),
        };
    }
    let v = format!("a{}", i % 7);
    match lang {
        "python" => format!("{pad}if {v} > {depth}:\n{}{pad}else:\n{pad}    return {depth}\n", nested(lang, depth - 1, i + 1, indent + 1)),
        "clojure" => format!("(if (> {v} {depth}) {} {depth})", nested(lang, depth - 1, i + 1, 0)),
        "rust" => format!("{pad}if {v} > {depth} {{\n{}{pad}}} else {{\n{pad}    {depth}\n{pad}}}\n", nested(lang, depth - 1, i + 1, indent + 1)),
        _ => format!("{pad}if ({v} > {depth}) {{\n{}{pad}}} else {{\n{pad}    return {depth};\n{pad}}}\n", nested(lang, depth - 1, i + 1, indent + 1)),
    }
}

fn function(lang: &str, name: &str, n_params: usize, depth: usize) -> String {
    let p = params(lang, n_params);
    let body = nested(lang, depth, 0, 1);
    match lang {
        "rust" => format!("pub fn {name}({p}) -> i32 {{\n{body}}}\n\n"),
        "typescript" => format!("export function {name}({p}): number {{\n{body}}}\n\n"),
        "python" => format!("def {name}({p}):\n{body}\n\n"),
        "java" => format!("    public static int {name}({p}) {{\n{body}    }}\n"),
        "c" | "cpp" => format!("int {name}({p}) {{\n{body}}}\n\n"),
        _ => format!("(defn {name} [{p}]\n  {body})\n\n"),
    }
}

fn file(lang: &str) -> (&'static str, &'static str, &'static str) {
    // (path, prefix, suffix)
    match lang {
        "rust" => ("src/lib.rs", "", ""),
        "typescript" => ("src/calc.ts", "", ""),
        "python" => ("calc/core.py", "", ""),
        "java" => ("src/main/java/org/calc/Calc.java", "package org.calc;\n\npublic class Calc {\n", "}\n"),
        "c" => ("src/calc.c", "", ""),
        "cpp" => ("src/calc.cpp", "namespace calc {\n", "}\n"),
        _ => ("src/calc/core.clj", "(ns calc.core)\n\n", ""),
    }
}

fn project(dir: &Path, lang: &str, functions: &[(String, usize, usize)]) -> u8 {
    let (path, pre, post) = file(lang);
    let mut text = pre.to_string();
    for (name, p, d) in functions {
        text.push_str(&function(lang, name, *p, *d));
    }
    text.push_str(post);
    let f = dir.join(path);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(&f, text).unwrap();
    if lang == "rust" {
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"calc\"\nversion = \"0.1.0\"\n").unwrap();
    }
    let r = import_project(dir, &[lang.to_string()], &[".".into()], &[], None).unwrap_or_else(|e| panic!("{lang}: {e}"));
    let m = compute(&r.design, false);
    let simplicity = m.health_report.dimensions["simplicity"].measured.unwrap();
    println!("{lang:<11} {:?} simplicity {simplicity:.1}  {}", m.health_report.measured, m.health_report.signals.iter().map(|s| s.rule.as_str()).collect::<Vec<_>>().join(","));
    m.health_report.measured.unwrap()
}

#[test]
fn tangled_functions_score_worse_than_split_ones_in_every_language() {
    for lang in spide_core::symbols::LANGUAGES {
        let (bad_dir, good_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let profile = spide_core::health::profiles::profile(lang);
        // one function: deeper and wider than the profile allows
        let bad = project(bad_dir.path(), lang, &[("decide".into(), profile.params as usize + 3, profile.complexity as usize + 4)]);
        // the same amount of decisions spread over small functions
        let parts = (profile.complexity as usize + 4).div_ceil(3);
        let good = project(good_dir.path(), lang, &(0..parts).map(|i| (format!("step{i}"), 2, 3)).collect::<Vec<_>>());
        assert!(bad < good, "{lang}: tangled {bad} should be below split {good}");
    }
}

#[test]
fn the_same_design_in_six_languages_lands_in_one_band() {
    let poly = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/poly");
    let mut scores = vec![];
    for (lang, dir) in [("rust", "rust"), ("typescript", "ts"), ("python", "py"), ("c", "c"), ("cpp", "cpp"), ("clojure", "clj")] {
        let r = import_project(&poly.join(dir), &[lang.to_string()], &[".".into()], &[], None).unwrap();
        let h = compute(&r.design, false).health_report;
        println!("{lang:<11} {:?} {:?}", h.measured, h.signals.iter().map(|s| format!("{}:{}", s.rule, s.target)).collect::<Vec<_>>());
        scores.push((lang, h.measured.unwrap()));
    }
    let lo = scores.iter().map(|s| s.1).min().unwrap();
    let hi = scores.iter().map(|s| s.1).max().unwrap();
    assert!(hi - lo <= 20, "same design, different languages: {scores:?}");
}
