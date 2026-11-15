//! Pure command metadata and validation (shared by the palette and tests).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandInfo {
    pub command: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub help: &'static str,
    pub template: Option<&'static str>,
    pub required: bool,
    pub keywords: &'static str,
}

const fn c(command: &'static str, label: &'static str, group: &'static str, help: &'static str) -> CommandInfo {
    CommandInfo { command, label, group, help, template: None, required: false, keywords: "" }
}
const fn t(command: &'static str, label: &'static str, group: &'static str, help: &'static str, template: &'static str, required: bool) -> CommandInfo {
    CommandInfo { command, label, group, help, template: Some(template), required, keywords: "" }
}
const fn k(mut i: CommandInfo, keywords: &'static str) -> CommandInfo {
    i.keywords = keywords;
    i
}

pub const COMMANDS: &[CommandInfo] = &[
    k(t("find", "Find class", "Navigate", "Zoom to a class", "find ", true), "search locate"),
    t("focus", "Focus package", "Navigate", "Highlight one package", "focus ", true),
    c("fit", "Fit diagram", "Navigate", "Show the whole active diagram"),
    c("tidy", "Tidy layout", "Canvas", "Rearrange unpinned classes"),
    k(t("detail 3", "Design detail", "Canvas", "Use the default exam-style detail level", "detail ", false), "level members"),
    c("lens concerns", "Concern lens", "Canvas", "Colour methods by what they do"),
    c("lens overlap", "Overlap lens", "Canvas", "Show duplicated behaviour"),
    c("lens off", "Turn lenses off", "Canvas", "Return to the standard diagram"),
    c("pruned", "Show or hide pruned", "Canvas", "Toggle elements you removed"),
    k(t("add", "Add class", "Design", "Add a class, optionally in a package", "add ", true), "new class"),
    c("accept", "Accept one proposal", "Design", "Accept a reviewed saved plan: /accept <proposal-id>"),
    c("reject", "Reject one proposal", "Design", "Reject one saved plan: /reject <proposal-id>"),
    c("undo", "Undo", "Design", "Undo the last design change"),
    c("redo", "Redo", "Design", "Redo the last undone change"),
    c("brief", "Brief and rules", "Design", "Edit goals, quality attributes and constraints"),
    t("mode", "Partner mode", "Design", "Show or set examiner, socratic, devils_advocate or sketch", "mode ", false),
    c("timeline", "Timeline", "History", "Browse previous revisions"),
    c("trace", "Trace", "History", "What agents changed: intent, symbols, health delta, checks, outcome"),
    c("goto", "Open in Neovim", "Code", "Open a class or Class.method in Neovim (⌘-click on the canvas)"),
    c("nvim", "Embedded Neovim", "Code", "Show Neovim in the side pane (your config)"),
    t("branch", "Create branch", "History", "Branch from this design", "branch ", true),
    c("branches", "List branches", "History", "Show available branches"),
    t("switch", "Switch branch", "History", "Make another branch active", "switch ", true),
    t("compare", "Compare", "History", "Compare decision options, or a named branch", "compare ", false),
    t("merge", "Merge branch", "History", "Propose another branch's changes", "merge ", true),
    k(c("project", "Spide Project", "Code", "Keep the design with its code: create, open or show sync"), "repository module sync"),
    t("readjust", "Readjust design to code", "Code", "The agent proposes design changes for what changed in the code", "readjust ", false),
    t("implement", "Implement in code", "Code", "The agent implements approved design changes in the code", "implement ", false),
    c("verify", "Verify", "Code", "Re-index the code and let the agent run the test command"),
    c("synced", "Mark in sync…", "Code", "Accept the remaining design ↔ code differences"),
    c("reindex", "Re-index code", "Code", "Parse the project's Java sources again"),
    c("lens sync", "Sync lens", "Code", "Show design ↔ code status on the class diagram"),
    t("coderoot", "Java package root", "Code", "Set or clear the package root for ArchUnit", "coderoot ", false),
    k(c("export", "Export…", "Export", "PNG, SVG, PDF, PlantUML, JSON, ADRs or ArchUnit — with preview and options"), "pdf svg image print save"),
    c("export png", "Export PNG", "Export", "Export the active diagram as an image"),
    c("export puml", "Export PlantUML", "Export", "Export the active diagram as PlantUML"),
    c("export all", "Export all PlantUML", "Export", "Export every non-empty diagram"),
    c("export adr", "Export ADRs", "Export", "Write decision records"),
    c("export archunit", "Export ArchUnit", "Export", "Write architecture tests"),
    c("open", "Open design JSON…", "Project", "Open an existing design file"),
    c("open project", "Open Spide Project…", "Project", "Open a code folder with spide/project.json"),
    c("new", "New project folder…", "Project", "Create a design in a folder"),
    c("save", "Save copy as…", "Project", "Save the current design as JSON"),
    t("name", "Rename design", "Project", "Set the design title", "name ", true),
    c("folder", "Reveal project folder", "Project", "Open the project in Finder"),
    c("agent", "Restart pi", "Project", "Restart the native pi session; the transcript display resets"),
];

/// The command without a leading `/spide ` or `/`.
pub fn strip_prefix(text: &str) -> String {
    let t = text.trim();
    let t = t.strip_prefix('/').unwrap_or(t);
    let t = match t.strip_prefix("spide") {
        Some(rest) if rest.starts_with(char::is_whitespace) => rest.trim_start(),
        _ => t,
    };
    t.strip_prefix('/').unwrap_or(t).to_string()
}

/// The canonical command, or a validation error for an argument-taking command.
pub fn normalize(text: &str) -> Result<String, String> {
    let command = strip_prefix(text);
    if command.is_empty() {
        return Err("Enter a command.".into());
    }
    let first = command.split_whitespace().next().unwrap_or("");
    if let Some(info) = COMMANDS.iter().find(|c| c.command.split(' ').next() == Some(first)) {
        if info.required && command.split_whitespace().count() < 2 {
            let what = if info.template.map(str::trim) == Some("add") { "a class name" } else { "a value" };
            return Err(format!("{} needs {what}.", info.label));
        }
    }
    Ok(command)
}

/// Palette matches for a query (first word against command, label, group, help, keywords).
pub fn matches(query: &str) -> Vec<&'static CommandInfo> {
    let q = strip_prefix(query).to_lowercase();
    let first = q.split_whitespace().next().unwrap_or("").to_string();
    COMMANDS.iter().filter(|c| q.is_empty() || format!("{} {} {} {} {}", c.command, c.label, c.group, c.help, c.keywords).to_lowercase().contains(&first)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_commands_validate_required_arguments_and_accept_prefixes() {
        assert_eq!(normalize("find").unwrap_err(), "Find class needs a value.");
        assert_eq!(normalize("/spide find Loan").unwrap(), "find Loan");
        assert_eq!(normalize("/tidy").unwrap(), "tidy");
        assert_eq!(normalize("   ").unwrap_err(), "Enter a command.");
        assert!(normalize("add").unwrap_err().contains("class name"));
    }

    #[test]
    fn every_palette_entry_is_unique_and_documented() {
        let set: std::collections::HashSet<&str> = COMMANDS.iter().map(|c| c.command).collect();
        assert_eq!(set.len(), COMMANDS.len());
        for c in COMMANDS {
            assert!(!c.label.is_empty() && !c.help.is_empty() && !c.group.is_empty(), "{}", c.command);
        }
        for c in COMMANDS.iter().filter(|c| c.required) {
            assert!(c.template.is_some_and(|t| t.ends_with(' ')), "{}", c.command);
        }
    }
}
