//! Responsibility analysis: what a class is for (its role), what each method does (its concern),
//! and whether they fit — RDD (knowing/doing/deciding) and GRASP (Controller, Information Expert,
//! Creator, Pure Fabrication), as taught in the DOSS course.

/// Roles a class can play (its intended purpose).
pub const ROLES: [&str; 10] = ["presentation", "controller", "service", "domain", "persistence", "adapter", "output", "creator", "utility", "value"];

/// Kinds of work a method does.
pub const CONCERNS: [&str; 7] = ["presentation", "coordination", "domain_logic", "persistence", "formatting", "creation", "accessor"];

pub fn check_role(r: &str) -> Result<(), String> {
    if r.is_empty() || ROLES.contains(&r) {
        Ok(())
    } else {
        Err(format!("unknown role {r:?} (use one of {})", ROLES.join("|")))
    }
}

pub fn check_concern(c: &str) -> Result<(), String> {
    if c.is_empty() || CONCERNS.contains(&c) {
        Ok(())
    } else {
        Err(format!("unknown concern {c:?} (use one of {})", CONCERNS.join("|")))
    }
}
