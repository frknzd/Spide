//! Project-level permissions. The owner's trust configuration lives OUTSIDE the project
//! (`$XDG_CONFIG_HOME/spide/trust.json`, default `~/.config/spide/trust.json`) so an agent that can
//! edit project files cannot escalate its own permissions or approve commands.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// No project mutation at all.
    ReadOnly,
    /// Staged proposals, plans, patches and assessments; the human applies them.
    #[default]
    ProposeOnly,
    /// May also apply approved classes of writes and run owner-approved checks.
    Autonomous,
}

impl Permission {
    pub fn as_str(self) -> &'static str {
        match self {
            Permission::ReadOnly => "read_only",
            Permission::ProposeOnly => "propose_only",
            Permission::Autonomous => "autonomous",
        }
    }
    pub fn parse(s: &str) -> Option<Permission> {
        [Permission::ReadOnly, Permission::ProposeOnly, Permission::Autonomous].into_iter().find(|p| p.as_str() == s)
    }
}

/// What a tool does, for policy purposes. Every tool must be classified; unknown tools are denied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    Read,
    /// Creates staged artifacts (proposals, assessments, derived views) without applying design changes.
    Propose,
    /// Applies changes to the design or source.
    Apply,
    /// Executes project tooling (checks, linters, language servers).
    Execute,
    /// Changes permissions, trust, scoring policy or approves exceptions: owner only, never via agents.
    Owner,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ProjectTrust {
    pub permission: Permission,
    /// Executing project tooling (LSP, linters, checks) is allowed at all.
    pub trusted: bool,
    /// Exact command lines the owner approved (from the manifest's `commands`).
    pub approved_commands: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TrustFile {
    pub version: u32,
    /// Canonical project root → trust.
    pub projects: BTreeMap<String, ProjectTrust>,
}

pub fn trust_path() -> PathBuf {
    if let Some(p) = std::env::var_os("SPIDE_TRUST_FILE") {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))).unwrap_or_else(|| PathBuf::from("."));
    base.join("spide").join("trust.json")
}

pub fn key(root: &Path) -> String {
    std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf()).display().to_string()
}

impl TrustFile {
    pub fn load_from(path: &Path) -> TrustFile {
        std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }
    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())? + "\n";
        crate::workspace::write_atomic(path, &text).map_err(|e| e.to_string())?;
        crate::workspace::restrict(path);
        Ok(())
    }
    pub fn get(&self, root: &Path) -> ProjectTrust {
        self.projects.get(&key(root)).cloned().unwrap_or_default()
    }
}

/// The effective trust of a project, re-read on every check (the owner may change it while running).
pub fn trust_for(root: &Path) -> ProjectTrust {
    TrustFile::load_from(&trust_path()).get(root)
}

pub fn classify(tool: &str) -> Option<Class> {
    use Class::*;
    Some(match tool {
        "get_design" | "get_metrics" | "get_view" | "get_overlap" | "get_responsibilities" | "suggest_home" | "workspace_info" | "get_events" | "get_trace" | "job_status"
        | "get_policy" | "get_health" | "doctor" | "get_symbols" | "find_symbol" | "get_source" | "get_project" | "get_sync" | "get_code" | "list_history"
        | "list_branches" | "get_selection" | "list_exceptions" | "get_assessment_requests" | "get_evidence" | "element_at" | "get_editor" | "open_source" | "reveal" | "select" | "editor_register" | "editor_unregister" | "editor_prefer" | "editor_diagnostics" | "list_patches" | "compare_branch" | "ui_command" | "yield_workspace"
        | "get_comments" | "lucid_status" | "lucid_export" | "lucid_plan_push" | "lucid_read_script" => Read,
        "propose_ops" | "propose_decision" | "evaluate_defense" | "assess_class" | "set_use_cases" | "set_domain_model" | "set_sequence" | "set_change_impact"
        | "propose_patch" | "restore_revision" | "merge_branch" | "submit_assessment" | "propose_exception" | "request_change" | "request_assessment" | "assess_health" | "cancel_assessment"
        // Lucid sync never changes the design directly: pulls become proposals, pushes write Lucid
        | "address_comment" | "lucid_pull" | "lucid_record_push" | "lucid_link" | "lucid_unlink" => Propose,
        "approve_decision" | "dismiss_decision" | "select_option" | "undo" | "redo" | "apply_patch" | "switch_branch" | "create_branch"
        | "mark_in_sync" | "cancel_job" | "revert_op" | "editor_handoff" | "reject_patch" | "apply_ops" | "prune" | "restore" | "set_mode" | "export" => Apply,
        "run_check" | "analyze" => Execute,
        "set_permission" | "approve_exception" | "reject_exception" | "approve_command" | "trust_project" | "set_priorities" | "set_rules" => Owner,
        _ => return None,
    })
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Decision {
    pub allowed: bool,
    pub permission: Permission,
    pub class: Option<Class>,
    pub reason: String,
}

/// Who is calling. `User` is the in-process desktop UI; `EditorUser` a human action in Neovim,
/// proven by the user token Spide pushed into the plugin over RPC (never published in files);
/// everything else over MCP is an `Agent`, whatever it claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Caller {
    User,
    EditorUser,
    Agent,
}

impl Caller {
    pub fn from_actor(actor: &str) -> Caller {
        match actor {
            "user" => Caller::User,
            "editor-user" => Caller::EditorUser,
            _ => Caller::Agent,
        }
    }
}

/// `check` for a caller: humans may apply and run approved tooling regardless of the agent
/// permission; only the in-process UI may change policy.
pub fn check_caller(tool: &str, trust: &ProjectTrust, caller: Caller) -> Decision {
    let d = check(tool, trust);
    match (caller, d.class) {
        (Caller::Agent, _) | (_, None) => d,
        (Caller::User, Some(_)) => Decision { allowed: true, reason: "user".into(), ..d },
        (Caller::EditorUser, Some(Class::Owner)) => d,
        (Caller::EditorUser, Some(Class::Execute)) => Decision { allowed: trust.trusted, reason: if trust.trusted { "user".into() } else { "untrusted workspace: tools can't run".into() }, ..d },
        (Caller::EditorUser, Some(_)) => Decision { allowed: true, reason: "user (editor)".into(), ..d },
    }
}

/// Is `tool` allowed under `trust`? Checked on every call, not only at discovery.
pub fn check(tool: &str, trust: &ProjectTrust) -> Decision {
    let class = classify(tool);
    let p = trust.permission;
    let (allowed, reason) = match class {
        None => (false, format!("unknown tool {tool}")),
        Some(Class::Read) => (true, "read".into()),
        Some(Class::Propose) => (p >= Permission::ProposeOnly, format!("{tool} stages changes; needs propose_only or autonomous (project is {})", p.as_str())),
        Some(Class::Apply) => (p == Permission::Autonomous, format!("{tool} applies changes; needs autonomous (project is {}). Ask the user to approve in Spide or raise the permission with `spide-headless trust`.", p.as_str())),
        Some(Class::Execute) => (
            p == Permission::Autonomous && trust.trusted,
            format!("{tool} executes project tooling; needs autonomous and a trusted workspace (`spide-headless trust --trusted`)"),
        ),
        Some(Class::Owner) => (false, format!("{tool} is owner-only: use the Spide UI or `spide-headless trust`, agents cannot change policy")),
    };
    Decision { allowed, permission: p, class, reason: if allowed { "allowed".into() } else { reason } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix() {
        let t = |p| ProjectTrust { permission: p, trusted: false, approved_commands: vec![] };
        assert!(check("get_design", &t(Permission::ReadOnly)).allowed);
        assert!(!check("propose_ops", &t(Permission::ReadOnly)).allowed);
        assert!(check("propose_ops", &t(Permission::ProposeOnly)).allowed);
        assert!(!check("approve_decision", &t(Permission::ProposeOnly)).allowed);
        assert!(check("approve_decision", &t(Permission::Autonomous)).allowed);
        assert!(!check("run_check", &t(Permission::Autonomous)).allowed, "untrusted workspace");
        assert!(check("run_check", &ProjectTrust { trusted: true, ..t(Permission::Autonomous) }).allowed);
        for owner in ["set_permission", "approve_exception", "approve_command", "trust_project"] {
            assert!(!check(owner, &t(Permission::Autonomous)).allowed, "{owner}");
        }
        assert!(!check("rm_rf", &t(Permission::Autonomous)).allowed);
        assert!(check_caller("apply_patch", &t(Permission::ReadOnly), Caller::EditorUser).allowed);
        assert!(!check_caller("set_permission", &t(Permission::Autonomous), Caller::EditorUser).allowed);
        assert!(check_caller("set_permission", &t(Permission::ReadOnly), Caller::User).allowed);
        assert!(!check_caller("approve_decision", &t(Permission::ReadOnly), Caller::from_actor("pretend-user")).allowed);
    }

    #[test]
    fn every_core_tool_is_classified() {
        for t in spide_core::tools::tool_list().as_array().unwrap() {
            let n = t["name"].as_str().unwrap();
            assert!(classify(n).is_some(), "{n}");
        }
    }

    #[test]
    fn trust_roundtrip_is_private() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("trust.json");
        let mut f = TrustFile::default();
        f.projects.insert(key(d.path()), ProjectTrust { permission: Permission::Autonomous, trusted: true, approved_commands: vec!["cargo test".into()] });
        f.save_to(&p).unwrap();
        assert_eq!(TrustFile::load_from(&p).get(d.path()).permission, Permission::Autonomous);
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
    }
}
