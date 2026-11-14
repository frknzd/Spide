// Pure command metadata and validation (shared by the palette and tests).
export interface CommandInfo { command: string; label: string; group: string; help: string; template?: string; required?: boolean; keywords?: string }
export const COMMANDS: CommandInfo[] = [
  { command: "find", label: "Find class", group: "Navigate", help: "Zoom to a class", template: "find ", required: true, keywords: "search locate" },
  { command: "focus", label: "Focus package", group: "Navigate", help: "Highlight one package", template: "focus ", required: true },
  { command: "fit", label: "Fit diagram", group: "Navigate", help: "Show the whole active diagram" },
  { command: "tidy", label: "Tidy layout", group: "Canvas", help: "Rearrange unpinned classes" },
  { command: "detail 3", label: "Design detail", group: "Canvas", help: "Use the default exam-style detail level", template: "detail ", keywords: "level members" },
  { command: "lens concerns", label: "Concern lens", group: "Canvas", help: "Colour methods by what they do" },
  { command: "lens overlap", label: "Overlap lens", group: "Canvas", help: "Show duplicated behaviour" },
  { command: "lens off", label: "Turn lenses off", group: "Canvas", help: "Return to the standard diagram" },
  { command: "pruned", label: "Show or hide pruned", group: "Canvas", help: "Toggle elements you removed" },
  { command: "add", label: "Add class", group: "Design", help: "Add a class, optionally in a package", template: "add ", required: true, keywords: "new class" },
  { command: "accept", label: "Accept one proposal", group: "Design", help: "Accept a reviewed saved plan: /accept <proposal-id>" },
  { command: "reject", label: "Reject one proposal", group: "Design", help: "Reject one saved plan: /reject <proposal-id>" },
  { command: "undo", label: "Undo", group: "Design", help: "Undo the last design change" },
  { command: "redo", label: "Redo", group: "Design", help: "Redo the last undone change" },
  { command: "brief", label: "Brief and rules", group: "Design", help: "Edit goals, quality attributes and constraints" },
  { command: "mode", label: "Partner mode", group: "Design", help: "Show or set examiner, socratic, devils_advocate or sketch", template: "mode " },
  { command: "strict", label: "Strict examiner", group: "Design", help: "Rejected decisions cannot be overruled" },
  { command: "lenient", label: "Lenient examiner", group: "Design", help: "Allow overruling rejected decisions" },
  { command: "timeline", label: "Timeline", group: "History", help: "Browse previous revisions" },
  { command: "branch", label: "Create branch", group: "History", help: "Branch from this design", template: "branch ", required: true },
  { command: "branches", label: "List branches", group: "History", help: "Show available branches" },
  { command: "switch", label: "Switch branch", group: "History", help: "Make another branch active", template: "switch ", required: true },
  { command: "compare", label: "Compare", group: "History", help: "Compare decision options, or a named branch", template: "compare " },
  { command: "merge", label: "Merge branch", group: "History", help: "Propose another branch's changes", template: "merge ", required: true },
  { command: "import", label: "Import Java code", group: "Code", help: "Compare the design with a source folder" },
  { command: "drift", label: "Design ↔ code", group: "Code", help: "Show or hide the drift report" },
  { command: "code", label: "Show implementation", group: "Code", help: "Show an imported class or method", template: "code ", required: true },
  { command: "coderoot", label: "Java package root", group: "Code", help: "Set or clear the package root for ArchUnit", template: "coderoot " },
  { command: "export", label: "Export…", group: "Export", help: "PNG, SVG, PDF, PlantUML, JSON, ADRs or ArchUnit — with preview and options", keywords: "pdf svg image print save" },
  { command: "export png", label: "Export PNG", group: "Export", help: "Export the active diagram as an image" },
  { command: "export puml", label: "Export PlantUML", group: "Export", help: "Export the active diagram as PlantUML" },
  { command: "export all", label: "Export all PlantUML", group: "Export", help: "Export every non-empty diagram" },
  { command: "export adr", label: "Export ADRs", group: "Export", help: "Write decision records" },
  { command: "export archunit", label: "Export ArchUnit", group: "Export", help: "Write architecture tests" },
  { command: "open", label: "Open design JSON…", group: "Project", help: "Open an existing design file" },
  { command: "new", label: "New project folder…", group: "Project", help: "Create a design in a folder" },
  { command: "save", label: "Save copy as…", group: "Project", help: "Save the current design as JSON" },
  { command: "name", label: "Rename design", group: "Project", help: "Set the design title", template: "name ", required: true },
  { command: "folder", label: "Reveal project folder", group: "Project", help: "Open the project in Finder" },
  { command: "agent", label: "Restart pi", group: "Project", help: "Restart the native pi session; the transcript display resets" },
];
export const SPIDE_COMMANDS = COMMANDS.map((c) => c.command);

/** Returns the canonical command, or a validation error for an argument-taking command. */
export function normalizeCommand(text: string): { command?: string; error?: string } {
  const command = text.trim().replace(/^\/?spide\s+/, "").replace(/^\//, "");
  if (!command) return { error: "Enter a command." };
  const info = COMMANDS.find((c) => c.command.split(" ")[0] === command.split(/\s+/)[0]);
  if (info?.required && command.split(/\s+/).length < 2) return { error: `${info.label} needs ${info.template?.trim() === "add" ? "a class name" : "a value"}.` };
  return { command };
}
