//! Language profiles: which rules apply to which language's constructs, and their thresholds.
//! A profile never penalises an idiom for being used (unsafe, macros, mutable state,
//! inheritance); it decides whether an OO heuristic is meaningful for a construct at all and how
//! large is too large in that language.

use crate::model::Class;

#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub language: &'static str,
    /// Cyclomatic complexity (branches + 1) above which a function is complex.
    pub complexity: u32,
    pub nesting: u32,
    /// Lines of code in one function.
    pub function_loc: u32,
    pub params: u32,
    /// Methods of one type / functions of one module before it is too large.
    pub type_members: usize,
    pub module_functions: usize,
    /// Law of Demeter chains are a smell (OO receivers), not iterator/threading pipelines.
    pub demeter: bool,
    /// "Data class" (data without behaviour) is a smell: not for Rust structs + impls, Python
    /// dataclasses, Clojure records or C structs.
    pub data_class: bool,
    /// LCOM-style cohesion applies to its types.
    pub cohesion: bool,
    pub notes: &'static [&'static str],
}

pub const PROFILES: &[Profile] = &[
    Profile { language: "java", complexity: 10, nesting: 4, function_loc: 30, params: 5, type_members: 20, module_functions: 30, demeter: true, data_class: true, cohesion: true, notes: &["OO responsibility assignment (GRASP/RDD) and package boundaries"] },
    Profile { language: "rust", complexity: 12, nesting: 4, function_loc: 45, params: 6, type_members: 25, module_functions: 40, demeter: false, data_class: false, cohesion: true, notes: &["traits are contracts; structs + impls are not anemic", "iterator chains are not Demeter violations", "unsafe/unwrap are judged by Clippy and context, not by presence"] },
    Profile { language: "typescript", complexity: 10, nesting: 4, function_loc: 35, params: 4, type_members: 20, module_functions: 35, demeter: true, data_class: false, cohesion: true, notes: &["structural types and unions; interfaces describe shapes", "options objects instead of long parameter lists"] },
    Profile { language: "python", complexity: 10, nesting: 4, function_loc: 30, params: 5, type_members: 20, module_functions: 35, demeter: true, data_class: false, cohesion: true, notes: &["module-level functions and Protocols are idiomatic", "dataclasses are values, not anemic objects"] },
    Profile { language: "c", complexity: 15, nesting: 5, function_loc: 60, params: 6, type_members: 30, module_functions: 50, demeter: false, data_class: false, cohesion: false, notes: &["modules (translation units) own data through functions; structs are plain data", "callbacks/function pointers are the extension point"] },
    Profile { language: "cpp", complexity: 12, nesting: 4, function_loc: 45, params: 5, type_members: 25, module_functions: 40, demeter: true, data_class: false, cohesion: true, notes: &["RAII and value types; pure-virtual classes are interfaces", "templates are not instantiated: generic code is judged by its declaration"] },
    Profile { language: "clojure", complexity: 8, nesting: 5, function_loc: 25, params: 4, type_members: 15, module_functions: 40, demeter: false, data_class: false, cohesion: false, notes: &["namespaces of pure functions; protocols and multimethods are the extension points", "threading macros are pipelines, not Demeter violations", "keep side effects (atoms, IO) at the edges"] },
];

/// Default for designs without code (UML-level OO judgement).
pub const DESIGN: Profile = Profile { language: "design", complexity: 10, nesting: 4, function_loc: 30, params: 5, type_members: 20, module_functions: 30, demeter: true, data_class: true, cohesion: true, notes: &["UML-level design: OO heuristics"] };

pub fn profile(lang: &str) -> &'static Profile {
    PROFILES.iter().find(|p| p.language == lang).unwrap_or(&DESIGN)
}

/// The language of a design class (its code link), if any.
pub fn language_of(c: &Class) -> Option<&str> {
    c.source.as_ref().map(|s| s.lang()).or_else(|| c.methods.iter().find_map(|m| m.source.as_ref().map(|s| s.lang())))
}

pub fn of_class(c: &Class) -> &'static Profile {
    language_of(c).map(profile).unwrap_or(&DESIGN)
}

/// Does a legacy (OO course) issue apply to this class under its profile?
pub fn applies(kind: &str, c: &Class) -> bool {
    let p = of_class(c);
    let module = c.is_module();
    match kind {
        "data_class" => p.data_class && !module,
        "demeter" => p.demeter,
        "cohesion" => p.cohesion && !module,
        // a module of functions is judged by its own size rule (module_functions)
        "god" | "large_class" => !module || c.methods.len() > p.module_functions,
        "creator" | "fat_controller" | "ui_bypasses_controller" | "exposed_state" => !module && p.data_class,
        // dynamically typed languages don't annotate types in the design sense
        "missing_types" => !matches!(p.language, "python" | "clojure"),
        // Java keeps the course's calibrated long-method/parameter rules; others use their profile
        "long_method" | "long_params" => p.language == "java" || p.language == "design",
        _ => true,
    }
}
