//! Spide's GUI-independent application service: workspace lifecycle, policy-checked command
//! dispatch, trace journal, jobs, domain events and the agent-facing MCP surface. Shared by the
//! desktop app and `spide-headless`.

pub mod branches;
pub mod history;
pub mod code;
pub mod discovery;
pub mod editor;
pub mod nvim;
mod parity;
pub mod events;
pub mod jobs;
pub mod lucid;
pub mod mcp;
pub mod policy;
pub mod trace;
pub mod workspace;

use events::{DomainEvent, EventLog, EventSink, Fanout};
use policy::Class;
use serde_json::{json, Value};
use spide_core::metrics::{compute, Metrics};
use spide_core::tools;
use std::path::{Path, PathBuf};
use spide_core::Store;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use trace::{Journal, Record};
use workspace::Workspace;

/// Capabilities of the process hosting the service. Desktop-only actions fail with a structured
/// error elsewhere instead of pretending success.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Host {
    Desktop,
    Headless,
}

/// Where the design store lives: a headless `Workspace`, or the desktop's shared state.
pub trait StoreHost: Send + Sync {
    fn root(&self) -> PathBuf;
    /// Run `f` with exclusive access to the store.
    fn with(&self, f: &mut dyn FnMut(&mut Store));
    /// Persist and broadcast after a change.
    fn commit(&self) -> Result<(), String>;
    /// Pick up changes other tools made to the design file.
    fn refresh(&self) {}
    /// Holds the single-writer lock.
    fn writer(&self) -> bool;
    /// Give up / take back the single-writer role (desktop attachment). Default: unsupported.
    fn release(&self) -> Result<(), String> {
        Err("this host can't hand over the workspace".into())
    }
    fn reacquire(&self) -> Result<(), String> {
        Err("this host can't take over the workspace".into())
    }
    /// The active branch changed.
    fn on_branch(&self, _name: &str) {}
    /// A code index the host already maintains (the desktop's); None: the service indexes itself.
    fn code_snapshot(&self) -> Option<code::CodeSnapshot> {
        None
    }
}

impl StoreHost for Mutex<Workspace> {
    fn root(&self) -> PathBuf {
        self.lock().unwrap_or_else(|e| e.into_inner()).root.clone()
    }
    fn with(&self, f: &mut dyn FnMut(&mut Store)) {
        f(&mut self.lock().unwrap_or_else(|e| e.into_inner()).store)
    }
    fn commit(&self) -> Result<(), String> {
        let mut w = self.lock().unwrap_or_else(|e| e.into_inner());
        w.store.suggest_externals();
        w.save().map(|_| ())
    }
    fn refresh(&self) {
        self.lock().unwrap_or_else(|e| e.into_inner()).reload_if_external();
    }
    fn writer(&self) -> bool {
        self.lock().unwrap_or_else(|e| e.into_inner()).lock.is_some()
    }
    fn release(&self) -> Result<(), String> {
        self.lock().unwrap_or_else(|e| e.into_inner()).release()
    }
    fn reacquire(&self) -> Result<(), String> {
        self.lock().unwrap_or_else(|e| e.into_inner()).reacquire()
    }
}

/// Extra tools contributed by the host (the desktop's project/sync tools, editor bridge, …).
pub trait Extension: Send + Sync {
    fn tools(&self) -> Vec<Value>;
    /// `None`: not ours.
    fn call(&self, svc: &Service, name: &str, args: &Value) -> Option<Result<ToolResult, String>>;
}

/// A tool's result: text for the agent plus what the trace should record.
#[derive(Debug, Default, Clone)]
pub struct ToolResult {
    pub text: String,
    pub changed: bool,
    pub files: Vec<String>,
    pub symbols: Vec<String>,
    pub checks: Vec<String>,
    pub artifact: Option<String>,
}

impl ToolResult {
    pub fn text(t: impl Into<String>) -> ToolResult {
        ToolResult { text: t.into(), ..Default::default() }
    }
}

pub struct Service {
    store: Arc<dyn StoreHost>,
    pub host: Host,
    pub log: EventLog,
    sink: Arc<dyn EventSink>,
    pub journal: Journal,
    pub jobs: jobs::Jobs,
    run: String,
    seq: Mutex<u64>,
    extensions: Mutex<Vec<Arc<dyn Extension>>>,
    /// Trust override for tests/embedding (None: read the owner's trust file each call).
    trust_override: Mutex<Option<policy::ProjectTrust>>,
    code: Mutex<code::CodeState>,
    pub evidence: Arc<code::EvidenceStore>,
    pub editors: editor::Editors,
    /// Headless only: the desktop took over the workspace; MCP traffic is forwarded to it.
    pub yielded: std::sync::atomic::AtomicBool,
    /// Proves human actions from Neovim (pushed to the plugin over RPC, never written to disk).
    pub user_token: String,
}

pub const SERVICE_INSTRUCTIONS: &str = "\n\nPermissions: the project's permission (get_policy) decides what you may do: read_only (inspect), \
propose_only (stage proposals for the user), autonomous (also apply: approve_decision, undo, run_check…). Apply tools need \
`expected_rev` (from workspace_info/get_design) and accept an `idempotency_key` for safe retries; a stale revision returns a conflict — re-read and retry. \
Every mediated change is traced (get_trace). You can never change permissions, trust or approve rule exceptions.";

fn changed_classes(before: &spide_core::Design, after: &spide_core::Design) -> Vec<String> {
    let mut out = vec![];
    for c in &after.classes {
        if before.classes.iter().find(|b| b.name == c.name) != Some(c) {
            out.push(c.name.clone());
        }
    }
    for c in &before.classes {
        if !after.classes.iter().any(|a| a.name == c.name) {
            out.push(format!("-{}", c.name));
        }
    }
    out
}

impl Service {
    pub fn open(dir: &Path, host: Host, extra: Option<Arc<dyn EventSink>>) -> Result<Service, String> {
        Ok(Self::from_workspace(Workspace::open(dir)?, host, extra))
    }

    pub fn from_workspace(ws: Workspace, host: Host, extra: Option<Arc<dyn EventSink>>) -> Service {
        let writer = ws.lock.is_some();
        Self::new(Arc::new(Mutex::new(ws)), writer, host, extra)
    }

    pub fn new(store: Arc<dyn StoreHost>, writer: bool, host: Host, extra: Option<Arc<dyn EventSink>>) -> Service {
        let root = store.root();
        let log = EventLog::default();
        let mut sinks: Vec<Arc<dyn EventSink>> = vec![Arc::new(log.clone())];
        sinks.extend(extra);
        let journal = Journal::new(&root);
        let jobs = jobs::Jobs::new(&root);
        if writer {
            journal.recover();
            jobs.recover();
            let _ = journal.compact();
        }
        Service {
            store,
            host,
            log,
            sink: Arc::new(Fanout(sinks)),
            journal,
            jobs,
            run: format!("R{}", spide_core::store::now_ms()),
            seq: Mutex::new(0),
            extensions: Mutex::new(vec![]),
            trust_override: Mutex::new(None),
            code: Mutex::new(code::CodeState::default()),
            evidence: Arc::new(code::EvidenceStore::new(&root)),
            editors: editor::Editors::default(),
            yielded: std::sync::atomic::AtomicBool::new(false),
            user_token: workspace::token(),
        }
    }

    pub fn add_extension(&self, e: Arc<dyn Extension>) {
        self.extensions.lock().unwrap().push(e);
    }

    /// Run `f` on the store (exclusive).
    pub fn with_store<R>(&self, f: impl FnOnce(&mut Store) -> R) -> R {
        let mut f = Some(f);
        let mut out = None;
        self.store.with(&mut |s| out = Some((f.take().unwrap())(s)));
        out.expect("store host ran the closure")
    }

    pub fn root(&self) -> PathBuf {
        self.store.root()
    }

    pub fn store_host(&self) -> &dyn StoreHost {
        &*self.store
    }

    pub fn emit(&self, e: DomainEvent) {
        self.sink.emit(e);
    }

    pub fn set_trust_override(&self, t: Option<policy::ProjectTrust>) {
        *self.trust_override.lock().unwrap() = t;
    }

    pub fn trust(&self) -> policy::ProjectTrust {
        if let Some(t) = self.trust_override.lock().unwrap().clone() {
            return t;
        }
        policy::trust_for(&self.root())
    }

    /// Findings for the design (or indexed code), with
    /// the workspace's evidence and languages.
    pub fn health(&self, scope: Option<&str>) -> Result<spide_core::health::HealthReport, String> {
        let design = self.with_store(|s| s.design.clone());
        let code = self.code().ok();
        let use_code = match scope {
            Some("code") => true,
            Some("design") => false,
            _ => !design.classes.iter().any(|c| c.status.is_live()),
        };
        let target: spide_core::Design = if use_code {
            let mut d = code.as_ref().map(|c| (*c.design).clone()).unwrap_or_default();
            d.health_exceptions = design.health_exceptions.clone();
            d
        } else {
            design
        };
        if let Some(c) = &code {
            self.evidence.refresh(c);
        }
        let languages = code.map(|c| c.languages).unwrap_or_default();
        let ev = self.evidence.set.lock().unwrap();
        let m = spide_core::metrics::compute_with(&target, false, spide_core::health::Context { evidence: Some(&ev), languages: &languages, now: spide_core::store::now_ms() });
        let mut r = m.health_report;
        if use_code { r.scope = "code".into(); }
        Ok(r)
    }

    /// The code index (host's, or built here on demand).
    pub fn code(&self) -> Result<code::CodeSnapshot, String> {
        if let Some(s) = self.store.code_snapshot() {
            return Ok(s);
        }
        let root = self.root();
        self.code.lock().unwrap().get(&root)
    }

    pub fn instructions(&self) -> String {
        format!("{}{}", tools::INSTRUCTIONS, SERVICE_INSTRUCTIONS)
    }

    fn next_op(&self) -> String {
        let mut s = self.seq.lock().unwrap();
        *s += 1;
        format!("{}-{}", self.run, *s)
    }

    pub fn tool_list(&self) -> Value {
        let mut list = tools::tool_list();
        let arr = list.as_array_mut().expect("tool list is an array");
        let guard = json!({"expected_rev":{"type":"integer","description":"Design revision you based this on (conflict if it moved)."},
                           "idempotency_key":{"type":"string","description":"Same key → same result, applied once."},
                           "intent":{"type":"string","description":"One line: why (shown in the trace)."}});
        let with_guard = |mut props: Value, required: Vec<&str>| {
            for (k, v) in guard.as_object().unwrap() {
                props[k] = v.clone();
            }
            let mut req: Vec<Value> = required.into_iter().map(Value::from).collect();
            req.push("expected_rev".into());
            json!({"type":"object","properties":props,"required":req})
        };
        arr.extend([
            json!({"name":"workspace_info","description":"Host (desktop|headless), capabilities, project root, permission and design revision.",
                "inputSchema":{"type":"object","properties":{}},"annotations":{"readOnlyHint":true}}),
            json!({"name":"get_events","description":"Domain events after sequence `after` (design changes, reveal/open requests, trace). Returns {events, latest}.",
                "inputSchema":{"type":"object","properties":{"after":{"type":"integer","minimum":0}}},"annotations":{"readOnlyHint":true}}),
            json!({"name":"get_policy","description":"Effective permission and what each class of tool requires.",
                "inputSchema":{"type":"object","properties":{}},"annotations":{"readOnlyHint":true}}),
            json!({"name":"get_trace","description":"Compact trace of mediated operations, newest first: intent → changed symbols/files → findings → checks → outcome. Filter only=failures|conflicts|unverified; detail=true adds full records.",
                "inputSchema":{"type":"object","properties":{"only":{"type":"string","enum":["all","failures","conflicts","unverified"]},"run":{"type":"string"},"actor":{"type":"string"},
                    "offset":{"type":"integer"},"limit":{"type":"integer"},"detail":{"type":"boolean"}}},"annotations":{"readOnlyHint":true}}),
            json!({"name":"job_status","description":"Status/output of a job (or all jobs without `id`).",
                "inputSchema":{"type":"object","properties":{"id":{"type":"string"}}},"annotations":{"readOnlyHint":true}}),
            json!({"name":"cancel_job","description":"Cancel a running job.","inputSchema":{"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}}),
            json!({"name":"run_check","description":"Run an owner-approved project command from spide/project.json `commands` (e.g. test, build) as a job. Autonomous + trusted workspaces only.",
                "inputSchema":{"type":"object","properties":{"name":{"type":"string"},"timeout_s":{"type":"integer"},"intent":{"type":"string"}},"required":["name"]}}),
            json!({"name":"approve_decision","description":"Apply a pending decision's selected plan (autonomous only). Same validation as the user's Approve.",
                "inputSchema": with_guard(json!({"id":{"type":"string"},"notes":{"type":"string"}}), vec!["id"])}),
            json!({"name":"select_option","description":"Select which alternative of a pending decision is its plan (autonomous only).",
                "inputSchema": with_guard(json!({"id":{"type":"string"},"option":{"type":"string"}}), vec!["id","option"])}),
            json!({"name":"dismiss_decision","description":"Dismiss a pending decision (autonomous only).",
                "inputSchema": with_guard(json!({"id":{"type":"string"}}), vec!["id"])}),
            json!({"name":"undo","description":"Undo the last design change (autonomous only).","inputSchema": with_guard(json!({}), vec![])}),
            json!({"name":"propose_exception","description":"Propose that a health rule doesn't apply somewhere (rule = signal rule id, scope = entity or prefix*), with reason, evidence, optional expires_at (ms) / review_when. Only the owner can approve; suppressed findings stay visible.",
                "inputSchema":{"type":"object","properties":{"rule":{"type":"string"},"scope":{"type":"string"},"reason":{"type":"string"},"evidence":{"type":"array","items":{"type":"string"}},"expires_at":{"type":"integer"},"review_when":{"type":"string"}},"required":["rule","scope","reason"]}}),
            json!({"name":"list_exceptions","description":"Health exceptions (proposed and approved).","inputSchema":{"type":"object","properties":{}},"annotations":{"readOnlyHint":true}}),
            json!({"name":"get_health","description":"Versioned design/code evidence: findings and provenance, tool status, gaps, and exceptions. No design grade. scope=design|code (default: design, or code when the design is empty).",
                "inputSchema":{"type":"object","properties":{"scope":{"type":"string","enum":["design","code"]}}},"annotations":{"readOnlyHint":true}}),
            json!({"name":"doctor","description":"Per language: parser support and limits, the language server and linters Spide would use (available/missing, with install hints), build configs that matter (compile_commands.json, tsconfig.json…), and whether the workspace is trusted to run them.",
                "inputSchema":{"type":"object","properties":{}},"annotations":{"readOnlyHint":true}}),
            json!({"name":"get_symbols","description":"Indexed code symbols (any language): modules/namespaces, types, functions, methods, fields with file:line. Filter by file, kind, query (substring of the qualified name); paginated.",
                "inputSchema":{"type":"object","properties":{"file":{"type":"string"},"kind":{"type":"string"},"query":{"type":"string"},"offset":{"type":"integer"},"limit":{"type":"integer"}}},"annotations":{"readOnlyHint":true}}),
            json!({"name":"find_symbol","description":"Source location of a symbol id, a qualified name, or a design element (class [+ method]) through its code link.",
                "inputSchema":{"type":"object","properties":{"id":{"type":"string"},"symbol":{"type":"string"},"class":{"type":"string"},"method":{"type":"string"}}},"annotations":{"readOnlyHint":true}}),
            json!({"name":"element_at","description":"The design element (and code node) at file:line — what a cursor in the editor points at.",
                "inputSchema":{"type":"object","properties":{"file":{"type":"string"},"line":{"type":"integer"}},"required":["file","line"]},"annotations":{"readOnlyHint":true}}),
            json!({"name":"get_source","description":"Numbered source lines of a workspace file (≤400 lines) and the file's content hash (use it as expected_hash for edits).",
                "inputSchema":{"type":"object","properties":{"file":{"type":"string"},"start_line":{"type":"integer"},"end_line":{"type":"integer"},"symbol":{"type":"string"},"class":{"type":"string"},"method":{"type":"string"}}},"annotations":{"readOnlyHint":true}}),
            json!({"name":"get_evidence","description":"Normalised LSP diagnostics and linter findings (category: correctness|safety|maintainability|complexity|performance|style), plus every tool run's status (ran, missing, not_configured, timed_out…). Missing runs mean unknown, not clean.",
                "inputSchema":{"type":"object","properties":{"file":{"type":"string"},"category":{"type":"string"},"limit":{"type":"integer"}}},"annotations":{"readOnlyHint":true}}),
            json!({"name":"open_source","description":"Open a symbol in the user's Neovim (external session preferred, else the embedded one) at its exact range. Give class [+ method], symbol, id, or file+line.",
                "inputSchema":{"type":"object","properties":{"class":{"type":"string"},"method":{"type":"string"},"symbol":{"type":"string"},"id":{"type":"string"},"file":{"type":"string"},"line":{"type":"integer"},"col":{"type":"integer"},"session":{"type":"string"}}}}),
            json!({"name":"reveal","description":"Select and focus the design element for file:line (or class [+ method]) in Spide's canvas; headless: returns the element and emits a reveal event.",
                "inputSchema":{"type":"object","properties":{"file":{"type":"string"},"line":{"type":"integer"},"class":{"type":"string"},"method":{"type":"string"}}}}),
            json!({"name":"editor_handoff","description":"Make another Neovim session the one Spide drives. Sessions left behind with unsaved buffers block it unless dirty=save (write them) or dirty=keep (leave them unsaved there).",
                "inputSchema":{"type":"object","properties":{"to":{"type":"string"},"dirty":{"type":"string","enum":["refuse","save","keep"]}},"required":["to"]}}),
            json!({"name":"get_editor","description":"Attached Neovim sessions (external/embedded), the preferred one, and their LSP clients.",
                "inputSchema":{"type":"object","properties":{}},"annotations":{"readOnlyHint":true}}),
            json!({"name":"propose_patch","description":"Stage a source edit for review: line edits [{start_line,end_line,text}] (1-based inclusive; insert with end_line=start_line-1) against the file's current content hash (from get_source). The user applies it from Spide or Neovim (:SpideApplyPatch); autonomous agents may apply_patch.",
                "inputSchema":{"type":"object","properties":{"file":{"type":"string"},"edits":{"type":"array","items":{"type":"object","properties":{"start_line":{"type":"integer"},"end_line":{"type":"integer"},"text":{"type":"string"}},"required":["start_line","end_line","text"]}},"expected_hash":{"type":"string"},"intent":{"type":"string"}},"required":["file","edits","expected_hash"]}}),
            json!({"name":"list_patches","description":"Staged patches and their state.","inputSchema":{"type":"object","properties":{}},"annotations":{"readOnlyHint":true}}),
            json!({"name":"apply_patch","description":"Apply a staged patch (id) or direct edits (file, edits, expected_hash). Open buffers change inside Neovim (undoable, guarded by changedtick; dirty buffers are never overwritten); closed files are written atomically if their hash still matches. Conflicts are reported, never forced.",
                "inputSchema":{"type":"object","properties":{"id":{"type":"string"},"file":{"type":"string"},"edits":{"type":"array","items":{"type":"object","properties":{"start_line":{"type":"integer"},"end_line":{"type":"integer"},"text":{"type":"string"}},"required":["start_line","end_line","text"]}},"expected_hash":{"type":"string"},"intent":{"type":"string"},"idempotency_key":{"type":"string"}}}}),
            json!({"name":"reject_patch","description":"Reject a staged patch.","inputSchema":{"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}}),
            json!({"name":"revert_op","description":"Revert an applied source change (trace op id) as a new guarded change (fails if the file changed since).",
                "inputSchema":{"type":"object","properties":{"op":{"type":"string"},"intent":{"type":"string"}},"required":["op"]}}),
            json!({"name":"request_change","description":"Ask for a change with code context (used by Neovim's :SpideAsk). Shown to the user/agent; nothing is applied.",
                "inputSchema":{"type":"object","properties":{"text":{"type":"string"},"file":{"type":"string"},"line":{"type":"integer"}},"required":["text"]}}),
            json!({"name":"analyze","description":"Run the language servers and linters (job; autonomous + trusted workspace). Results land in get_evidence and health.",
                "inputSchema":{"type":"object","properties":{"lsp":{"type":"boolean"},"linters":{"type":"boolean"},"timeout_s":{"type":"integer"}}}}),
            json!({"name":"redo","description":"Redo (autonomous only).","inputSchema": with_guard(json!({}), vec![])}),
        ]);
        arr.extend(parity::tools());
        arr.extend(lucid::tools());
        for e in self.extensions.lock().unwrap().iter() {
            arr.extend(e.tools());
        }
        // an extension may replace a built-in tool: keep the last definition of each name
        let mut seen = std::collections::BTreeSet::new();
        let mut out: Vec<Value> = arr.drain(..).rev().filter(|t| seen.insert(t["name"].as_str().unwrap_or("").to_string())).collect();
        out.reverse();
        Value::Array(out)
    }

    /// The single entry point for every agent/client command: policy → guard → intent → run → outcome.
    pub fn call_tool(&self, name: &str, args: &Value, actor: &str) -> Result<String, String> {
        if matches!(name, "get_assessment_requests" | "assess_health" | "request_assessment" | "cancel_assessment" | "set_priorities") {
            return Err(format!("{name} is retired: numeric health ratings are no longer supported. Use get_health to inspect findings and evidence gaps."));
        }
        let trust = self.trust();
        let decision = policy::check_caller(name, &trust, policy::Caller::from_actor(actor));
        let class = decision.class;
        // A valid policy does not grant ownership of the workspace. Reject writes
        // before dispatch (or journal writes), not after mutating the in-memory store.
        if !self.store.writer() && class.is_some_and(|c| c != Class::Read) {
            return Err("This workspace is read-only; another process owns it or it could not be opened.".into());
        }
        let traced = !matches!(class, Some(Class::Read) | None);
        let op = self.next_op();
        let intent = args.get("intent").and_then(Value::as_str).or_else(|| args.get("rationale").and_then(Value::as_str)).or_else(|| args.get("question").and_then(Value::as_str)).unwrap_or("").to_string();
        let base = Record { run: self.run.clone(), op: op.clone(), actor: actor.into(), tool: name.into(), intent, policy: format!("{}: {}", decision.permission.as_str(), decision.reason), ..Default::default() };
        if !decision.allowed {
            if class.is_some() {
                let _ = self.journal.append(Record { kind: "outcome".into(), ok: Some(false), error: Some(format!("denied: {}", decision.reason)), at: spide_core::store::now_ms(), ..base });
            }
            return Err(format!("permission denied: {}", decision.reason));
        }
        if !traced {
            return self.dispatch(name, args).map(|r| r.text);
        }
        // idempotent retry: return the recorded result of the same key
        let key = args.get("idempotency_key").and_then(Value::as_str).map(str::to_string);
        if let Some(k) = &key {
            let tag = format!("idem:{k}");
            if let Some(prev) = self.journal.read().into_iter().rev().find(|r| r.kind == "outcome" && r.artifact.as_deref().is_some_and(|a| a.starts_with(&tag))) {
                return if prev.ok == Some(true) { Ok(format!("already applied (idempotency_key {k}) at revision {:?}", prev.result_rev)) } else { Err(prev.error.unwrap_or_else(|| "previous attempt failed".into())) };
            }
        }
        self.store.refresh();
        let (before, base_rev) = self.with_store(|s| (s.design.clone(), s.design.rev));
        if class == Some(Class::Apply) {
            match args.get("expected_rev").and_then(Value::as_u64) {
                Some(r) if r == base_rev => {}
                Some(r) => return self.conflict(base, base_rev, format!("conflict: expected revision {r}, workspace is at {base_rev}; re-read and retry")),
                // content-guarded (file hash / buffer changedtick) or not about the design
                None if matches!(name, "cancel_job" | "apply_patch" | "revert_op" | "editor_handoff" | "reject_patch" | "export") => {}
                None => return Err("expected_rev is required for changes (read it from workspace_info)".into()),
            }
        }
        let _ = self.journal.append(Record { kind: "intent".into(), base_rev: Some(base_rev), at: spide_core::store::now_ms(), ..base.clone() });
        let started = Instant::now();
        let result = self.dispatch(name, args);
        let after = self.with_store(|s| s.design.clone());
        let mut rec = Record { kind: "outcome".into(), base_rev: Some(base_rev), result_rev: Some(after.rev), at: spide_core::store::now_ms(), ms: Some(started.elapsed().as_millis() as u64), ..base };
        if after.rev != base_rev {
            let (mb, ma): (Metrics, Metrics) = (compute(&before, false), compute(&after, false));
            let (added, resolved) = spide_core::impact::issue_diff(&mb, &ma);
            rec.issues_added = added;
            rec.issues_resolved = resolved;
            rec.symbols = changed_classes(&before, &after);
            if rec.symbols.is_empty() && rec.changes.is_empty() {
                rec.changes = after.log.last().map(|l| l.summary.clone()).unwrap_or_default();
            }
        }
        match &result {
            Ok(r) => {
                rec.ok = Some(true);
                rec.files.extend(r.files.clone());
                rec.symbols.extend(r.symbols.clone());
                rec.checks.extend(r.checks.clone());
                rec.artifact = r.artifact.clone();
            }
            Err(e) => {
                rec.ok = Some(false);
                rec.error = Some(e.clone());
            }
        }
        if let Some(k) = &key {
            rec.artifact = Some(format!("idem:{k}{}", rec.artifact.as_ref().map(|a| format!(" {a}")).unwrap_or_default()));
        }
        let summary = rec.summary();
        let _ = self.journal.append(rec);
        self.emit(DomainEvent::Trace { run: self.run.clone(), summary });
        result.map(|r| r.text)
    }

    fn conflict(&self, base: Record, rev: u64, msg: String) -> Result<String, String> {
        let _ = self.journal.append(Record { kind: "outcome".into(), base_rev: Some(rev), ok: Some(false), error: Some(msg.clone()), at: spide_core::store::now_ms(), ..base });
        Err(msg)
    }

    /// Persist and announce a design change.
    pub fn commit(&self, actor: &str) -> Result<(), String> {
        self.store.commit()?;
        let rev = self.with_store(|s| s.design.rev);
        self.emit(DomainEvent::DesignChanged { rev, actor: actor.to_string() });
        Ok(())
    }

    fn dispatch(&self, name: &str, args: &Value) -> Result<ToolResult, String> {
        let s = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let rev = || args.get("expected_rev").and_then(Value::as_u64).unwrap_or(0);
        match name {
            "workspace_info" => {
                let (root, rev) = (self.root(), self.with_store(|s| s.design.rev));
                Ok(ToolResult::text(
                    json!({
                        "host": match self.host { Host::Desktop => "desktop", Host::Headless => "headless" },
                        "root": root.display().to_string(),
                        "project": spide_core::project::is_project(&root),
                        "rev": rev,
                        "writer": self.store.writer(),
                        "permission": self.trust().permission.as_str(),
                        "capabilities": {"canvas": self.host == Host::Desktop, "extensions": self.extensions.lock().unwrap().iter().flat_map(|e| e.tools()).filter_map(|t| t["name"].as_str().map(str::to_string)).collect::<Vec<_>>()},
                    })
                    .to_string(),
                ))
            }
            "get_events" => {
                let after = args.get("after").and_then(Value::as_u64).unwrap_or(0);
                let (events, latest) = self.log.since(after);
                let events: Vec<Value> = events.into_iter().map(|(n, e)| json!({"seq": n, "event": e})).collect();
                Ok(ToolResult::text(json!({"events": events, "latest": latest}).to_string()))
            }
            "get_policy" => {
                let t = self.trust();
                Ok(ToolResult::text(
                    json!({"permission": t.permission, "trusted": t.trusted, "approved_commands": t.approved_commands,
                        "requires": {"read":"any","propose":"propose_only|autonomous","apply":"autonomous","execute":"autonomous + trusted","owner":"never via agents"},
                        "trust_file": policy::trust_path().display().to_string()})
                    .to_string(),
                ))
            }
            "get_trace" => {
                let mut f: trace::Filter = serde_json::from_value(args.clone()).unwrap_or_default();
                if f.limit == 0 {
                    f.limit = 30;
                }
                let (rows, total) = self.journal.timeline(&f);
                Ok(ToolResult::text(json!({"total": total, "rows": rows}).to_string()))
            }
            "job_status" => {
                let id = s("id");
                if id.is_empty() {
                    Ok(ToolResult::text(serde_json::to_string(&self.jobs.list()).unwrap()))
                } else {
                    self.jobs.get(&id).map(|j| ToolResult::text(serde_json::to_string(&j).unwrap())).ok_or_else(|| format!("unknown job {id}"))
                }
            }
            "cancel_job" => self.jobs.cancel(&s("id")).map(|_| ToolResult::text("cancelled")),
            "doctor" => {
                let root = self.root();
                let m = code::manifest(&root);
                let t = self.trust();
                let caps: Vec<Value> = m.all_languages().iter().map(|l| {
                    let c = spide_import::languages::CAPABILITIES.iter().find(|(x, _, _)| x == l);
                    json!({"language": l, "extracts": c.map(|c| c.1).unwrap_or(&[]), "limits": c.map(|c| c.2).unwrap_or(&[])})
                }).collect();
                Ok(ToolResult::text(json!({"languages": m.all_languages(), "parsers": caps, "tools": spide_lsp::defaults::doctor(&root, &m),
                    "trusted": t.trusted, "can_run_tools": t.trusted && t.permission == policy::Permission::Autonomous,
                    "note": "Tools run only in trusted workspaces (they execute project code). Missing tools mean unknown evidence, not a clean result."}).to_string()))
            }
            "get_symbols" => Ok(ToolResult::text(code::symbols(&self.code()?, args).to_string())),
            "find_symbol" => {
                let d = self.with_store(|st| st.design.clone());
                Ok(ToolResult::text(code::locate(&self.code()?, &d, args)?.to_string()))
            }
            "element_at" => {
                let d = self.with_store(|st| st.design.clone());
                let line = args.get("line").and_then(Value::as_u64).unwrap_or(1) as u32;
                Ok(ToolResult::text(code::element_at(&self.code()?, &d, &s("file"), line).unwrap_or(Value::Null).to_string()))
            }
            "get_source" => {
                let root = self.root();
                if args.get("file").is_some() {
                    let a = args.get("start_line").and_then(Value::as_u64).unwrap_or(1) as u32;
                    let b = args.get("end_line").and_then(Value::as_u64).unwrap_or(a as u64 + 200) as u32;
                    Ok(ToolResult::text(code::source(&root, &s("file"), a, b)?.to_string()))
                } else {
                    let d = self.with_store(|st| st.design.clone());
                    let loc = code::locate(&self.code()?, &d, args)?;
                    let a = loc["line"].as_u64().unwrap_or(1) as u32;
                    let b = loc["end_line"].as_u64().unwrap_or(a as u64) as u32;
                    let mut v = code::source(&root, loc["file"].as_str().unwrap_or(""), a, b)?;
                    v["symbol"] = loc;
                    Ok(ToolResult::text(v.to_string()))
                }
            }
            "get_evidence" => {
                if let Ok(snap) = self.code() {
                    self.evidence.refresh(&snap);
                }
                Ok(ToolResult::text(self.evidence.query(args).to_string()))
            }
            "analyze" => self.analyze(args),
            "editor_register" => {
                let sess = editor::Session {
                    id: s("id"),
                    kind: if s("kind") == "embedded" { "embedded".into() } else { "external".into() },
                    socket: s("socket"),
                    pid: args.get("pid").and_then(Value::as_u64).unwrap_or(0) as u32,
                    cwd: s("cwd"),
                    registered_at: 0,
                    last_active: 0,
                    lsp: args.get("lsp").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default(),
                };
                if sess.socket.is_empty() {
                    return Err("socket is required (v:servername)".into());
                }
                let sess = self.editors.register(sess)?;
                // hand the plugin the user token over RPC (not through this response: agents see responses)
                if let Ok((_, n)) = self.editors.pick(Some(&sess.id)) {
                    let _ = n.plugin("set_user_token", vec![json!(self.user_token)]);
                }
                self.emit(DomainEvent::Editors);
                Ok(ToolResult::text(json!({"session": sess, "root": self.root().display().to_string()}).to_string()))
            }
            "editor_unregister" => {
                self.editors.unregister(&s("id"));
                self.emit(DomainEvent::Editors);
                Ok(ToolResult::text("ok"))
            }
            "editor_prefer" => {
                self.editors.prefer(args.get("id").and_then(Value::as_str).map(str::to_string))?;
                self.emit(DomainEvent::Editors);
                Ok(ToolResult::text("ok"))
            }
            "editor_handoff" => {
                // switch which Neovim Spide drives; the one we leave keeps or saves its unsaved buffers
                let to = s("to");
                let policy = args.get("dirty").and_then(Value::as_str).unwrap_or("refuse");
                let mut report = vec![];
                for sess in self.editors.list().into_iter().filter(|x| x.id != to) {
                    let dirty = self.editors.dirty(&sess.id).unwrap_or_default();
                    if dirty.is_empty() {
                        continue;
                    }
                    match policy {
                        "save" => {
                            let left = self.editors.save_all(&sess.id)?;
                            if left.as_array().is_some_and(|a| !a.is_empty()) {
                                return Err(format!("could not save in {}: {left}", sess.id));
                            }
                            report.push(json!({"session": sess.id, "saved": dirty}));
                        }
                        "keep" => report.push(json!({"session": sess.id, "kept_unsaved": dirty})),
                        _ => return Err(format!("{} has unsaved buffers: {}; choose dirty=save or dirty=keep", sess.id, dirty.join(", "))),
                    }
                }
                self.editors.prefer(Some(to.clone()))?;
                self.emit(DomainEvent::Editors);
                Ok(ToolResult::text(json!({"preferred": to, "handoff": report}).to_string()))
            }
            "yield_workspace" => {
                if self.host != Host::Headless {
                    return Err("only a headless service hands its workspace to the desktop".into());
                }
                self.store.release()?;
                self.yielded.store(true, std::sync::atomic::Ordering::SeqCst);
                self.emit(DomainEvent::Notice { kind: "info".into(), text: "the desktop took over this workspace; forwarding".into() });
                Ok(ToolResult::text("released"))
            }
            "get_editor" => Ok(ToolResult::text(json!({"sessions": self.editors.list()}).to_string())),
            "editor_diagnostics" => {
                let root = self.root();
                let file = s("file");
                let p = code::safe_path(&root, &file)?;
                let rel = p.strip_prefix(root.canonicalize().map_err(|e| e.to_string())?).map(|r| r.display().to_string()).unwrap_or(file);
                let lang = spide_core::symbols::language_of(&rel).unwrap_or("").to_string();
                // unsaved buffers: a labelled preview, never mistaken for the saved file's state
                let unsaved = args.get("unsaved").and_then(Value::as_bool).unwrap_or(false);
                let findings: Vec<spide_core::evidence::Finding> = args.get("diagnostics").and_then(Value::as_array).into_iter().flatten().map(|d| {
                    let tool = d["source"].as_str().unwrap_or("nvim").to_ascii_lowercase();
                    let code = match &d["code"] { Value::String(c) => c.clone(), Value::Number(n) => n.to_string(), _ => String::new() };
                    let level = spide_core::evidence::Level::from_lsp(d["severity"].as_u64().unwrap_or(2));
                    let message = d["message"].as_str().unwrap_or("").to_string();
                    spide_core::evidence::Finding {
                        category: spide_core::evidence::categorize(&tool, &code, level, &message),
                        code, tool, provider: if unsaved { "editor-unsaved".into() } else { "editor".into() }, level, language: lang.clone(), file: rel.clone(),
                        range: spide_core::evidence::Range { line: d["lnum"].as_u64().unwrap_or(0) as u32 + 1, col: d["col"].as_u64().unwrap_or(0) as u32, end_line: d["end_lnum"].as_u64().unwrap_or(0) as u32 + 1, end_col: d["end_col"].as_u64().unwrap_or(0) as u32 },
                        message, entity: None,
                        // unsaved buffer: tied to the buffer's text, not the file's
                        doc_hash: args.get("text_hash").and_then(Value::as_str).map(str::to_string),
                    }
                }).collect();
                {
                    let mut set = self.evidence.set.lock().unwrap();
                    set.findings.retain(|f| !(f.provider.starts_with("editor") && f.file == rel));
                    // the editor's live LSP is authoritative for this file: drop our own LSP run's copies
                    set.findings.retain(|f| !(f.provider == "lsp" && f.file == rel));
                    set.add(findings);
                }
                self.evidence.save();
                self.emit(DomainEvent::EvidenceChanged);
                Ok(ToolResult::text("ok"))
            }
            "list_exceptions" => Ok(ToolResult::text(serde_json::to_string(&self.with_store(|st| st.design.health_exceptions.clone())).unwrap())),
            "propose_exception" => {
                let x = spide_core::health::HealthException {
                    rule: s("rule"), scope: s("scope"), reason: s("reason"),
                    evidence: args.get("evidence").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default(),
                    expires_at: args.get("expires_at").and_then(Value::as_u64),
                    review_when: args.get("review_when").and_then(Value::as_str).map(str::to_string),
                    ..Default::default()
                };
                let id = self.with_store(|st| spide_core::health::propose_exception(&mut st.design, x, "agent"))?;
                self.commit("agent")?;
                Ok(ToolResult::text(json!({"exception": id, "state": "awaiting the owner's approval"}).to_string()))
            }
            "approve_exception" | "reject_exception" => {
                let approve = name == "approve_exception";
                self.with_store(|st| spide_core::health::decide_exception(&mut st.design, &s("id"), approve, spide_core::store::now_ms()))?;
                self.commit("user")?;
                Ok(ToolResult::text(if approve { "approved" } else { "rejected" }))
            }
            "get_health" => Ok(ToolResult::text(serde_json::to_string(&self.health(args.get("scope").and_then(Value::as_str))?).unwrap())),
            "open_source" => {
                let root = self.root();
                let (file, line, col) = if args.get("file").is_some() {
                    (s("file"), args.get("line").and_then(Value::as_u64).unwrap_or(1) as u32, args.get("col").and_then(Value::as_u64).unwrap_or(0) as u32)
                } else {
                    let d = self.with_store(|st| st.design.clone());
                    let loc = code::locate(&self.code()?, &d, args)?;
                    (loc["file"].as_str().unwrap_or("").to_string(), loc["line"].as_u64().unwrap_or(1) as u32, loc["col"].as_u64().unwrap_or(0) as u32)
                };
                let abs = code::safe_path(&root, &file)?;
                self.emit(DomainEvent::OpenSource { file: file.clone(), line, column: col, symbol: args.get("symbol").and_then(Value::as_str).map(str::to_string) });
                Ok(ToolResult::text(self.editors.open(&abs, line, col, args.get("session").and_then(Value::as_str))?.to_string()))
            }
            "reveal" => {
                let el = if args.get("file").is_some() {
                    let d = self.with_store(|st| st.design.clone());
                    code::element_at(&self.code()?, &d, &s("file"), args.get("line").and_then(Value::as_u64).unwrap_or(1) as u32).ok_or("nothing indexed at that location")?
                } else {
                    json!({"design_class": s("class"), "method": args.get("method"), "in_design": true})
                };
                let target = el["design_class"].as_str().or(el["code_class"].as_str()).unwrap_or("").to_string();
                let symbol = match el["method"].as_str() {
                    Some(m) => format!("{target}.{m}"),
                    None => target,
                };
                self.emit(DomainEvent::Reveal { symbol, origin: "editor".into() });
                Ok(ToolResult::text(el.to_string()))
            }
            "request_change" => {
                let ctx = json!({"file": args.get("file"), "line": args.get("line")});
                self.emit(DomainEvent::ChangeRequest { text: s("text"), context: ctx });
                Ok(ToolResult::text("sent"))
            }
            "propose_patch" => {
                let root = self.root();
                let file = s("file");
                let abs = code::safe_path(&root, &file)?;
                let edits: Vec<editor::Edit> = serde_json::from_value(args.get("edits").cloned().unwrap_or(json!([]))).map_err(|e| format!("edits: {e}"))?;
                let text = std::fs::read_to_string(&abs).map_err(|e| e.to_string())?;
                let base = s("expected_hash");
                if base != editor::hash(&text) {
                    return Err(format!("conflict: {file} changed (hash {}); re-read it with get_source", editor::hash(&text)));
                }
                let after = editor::apply_edits(&text, &edits)?;
                let p = editor::Patch { id: format!("patch-{}", spide_core::store::now_ms()), file: file.clone(), diff: editor::diff(&text, &after), edits, base_hash: base, intent: s("intent"), created_at: spide_core::store::now_ms(), state: "pending".into() };
                editor::save_patch(&root, &p)?;
                Ok(ToolResult { text: json!({"patch": p.id, "diff": p.diff}).to_string(), files: vec![file], artifact: Some(format!("patch:{}", p.id)), ..Default::default() })
            }
            "list_patches" => Ok(ToolResult::text(serde_json::to_string(&editor::list_patches(&self.root())).unwrap())),
            "reject_patch" => {
                let root = self.root();
                let mut p = editor::load_patch(&root, &s("id"))?;
                p.state = "rejected".into();
                editor::save_patch(&root, &p)?;
                Ok(ToolResult::text("rejected"))
            }
            "apply_patch" => self.apply_patch(args),
            "revert_op" => {
                let root = self.root();
                let op = s("op");
                if !op.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                    return Err("invalid op".into());
                }
                let a: editor::Applied = serde_json::from_str(&std::fs::read_to_string(editor::applied_path(&root, &op)).map_err(|_| format!("no recorded source change for {op}"))?).map_err(|e| e.to_string())?;
                let n = a.after.lines().count() as u32;
                let edits = vec![editor::Edit { start_line: 1, end_line: n, text: a.before.trim_end_matches('\n').to_string() }];
                self.apply_patch(&json!({"file": a.file, "edits": edits, "expected_hash": editor::hash(&a.after), "intent": format!("revert {op}")}))
            }
            "run_check" => self.run_check(&s("name"), args.get("timeout_s").and_then(Value::as_u64).unwrap_or(600)),
            "approve_decision" => {
                self.with_store(|st| st.approve_decision(&s("id"), rev(), s("notes"))).map_err(|e| e.0)?;
                self.commit("agent")?;
                Ok(ToolResult::text(format!("approved {}", s("id"))))
            }
            "select_option" => {
                self.with_store(|st| st.select_decision_option(&s("id"), &s("option"), rev())).map_err(|e| e.0)?;
                self.commit("agent")?;
                Ok(ToolResult::text("selected"))
            }
            "dismiss_decision" => {
                self.with_store(|st| st.dismiss_decision(&s("id"))).map_err(|e| e.0)?;
                self.commit("agent")?;
                Ok(ToolResult::text(format!("dismissed {}", s("id"))))
            }
            "undo" | "redo" => {
                let done = self.with_store(|st| if name == "undo" { st.undo() } else { st.redo() });
                if !done {
                    return Err(format!("nothing to {name}"));
                }
                self.commit("agent")?;
                Ok(ToolResult::text(format!("{name} done")))
            }
            _ => {
                let exts: Vec<Arc<dyn Extension>> = self.extensions.lock().unwrap().clone();
                for e in exts {
                    if let Some(r) = e.call(self, name, args) {
                        return r;
                    }
                }
                if let Some(r) = self.parity(name, args) {
                    return r;
                }
                if let Some(r) = self.lucid(name, args) {
                    return r;
                }
                let out = self.with_store(|st| tools::call(st, name, args))?;
                if out.changed {
                    self.commit("agent")?;
                }
                Ok(ToolResult { text: out.text, changed: out.changed, ..Default::default() })
            }
        }
    }

    /// Apply edits through Neovim (open buffer) or to disk (closed file), guarded.
    fn apply_patch(&self, args: &Value) -> Result<ToolResult, String> {
        let root = self.root();
        let mut staged = None;
        let (file, edits, expected) = match args.get("id").and_then(Value::as_str) {
            Some(id) => {
                let p = editor::load_patch(&root, id)?;
                if p.state != "pending" {
                    return Err(format!("patch {id} is {}", p.state));
                }
                staged = Some(p.clone());
                (p.file, p.edits, p.base_hash)
            }
            None => (
                args.get("file").and_then(Value::as_str).ok_or("file or id required")?.to_string(),
                serde_json::from_value::<Vec<editor::Edit>>(args.get("edits").cloned().unwrap_or(json!([]))).map_err(|e| format!("edits: {e}"))?,
                args.get("expected_hash").and_then(Value::as_str).ok_or("expected_hash required (from get_source)")?.to_string(),
            ),
        };
        let abs = code::safe_path(&root, &file)?;
        let disk = std::fs::read_to_string(&abs).map_err(|e| e.to_string())?;
        let mark = |state: &str| {
            if let Some(mut p) = staged.clone() {
                p.state = state.into();
                let _ = editor::save_patch(&root, &p);
            }
        };
        let (before, after, via) = match self.editors.buffer(&abs) {
            Some((session, st)) => {
                // the buffer is the truth: compare against its text, apply inside Neovim
                let (_, bt) = self.editors.buffer_text(&abs).ok_or("could not read the Neovim buffer")?;
                let text = bt["text"].as_str().unwrap_or("").to_string();
                if editor::hash(&text) != expected {
                    mark("conflict");
                    return Err(format!("conflict: {file} differs in Neovim ({}) from what the edit was written against", if st["dirty"] == true { "unsaved changes" } else { "changed" }));
                }
                let after = editor::apply_edits(&text, &edits)?;
                self.editors.apply_in_buffer(&session, &abs, bt["changedtick"].as_i64().unwrap_or(-1), &edits)?;
                (text, after, format!("neovim:{session} (unsaved; save in Neovim)"))
            }
            None => {
                if editor::hash(&disk) != expected {
                    mark("conflict");
                    return Err(format!("conflict: {file} changed on disk since the edit was written (now {}); re-read and retry", editor::hash(&disk)));
                }
                let after = editor::apply_edits(&disk, &edits)?;
                workspace::write_atomic(&abs, &after).map_err(|e| e.to_string())?;
                (disk, after, "disk".to_string())
            }
        };
        mark("applied");
        let op = format!("{}-{}", self.run, spide_core::store::now_ms());
        let rec = editor::Applied { op: op.clone(), file: file.clone(), before: before.clone(), after: after.clone(), via: via.clone() };
        let path = editor::applied_path(&root, &op);
        let _ = workspace::write_atomic(&path, &serde_json::to_string(&rec).unwrap());
        self.code.lock().unwrap().invalidate();
        let d = editor::diff(&before, &after);
        let stat = format!("{file} +{} −{} via {via}", d.lines().filter(|l| l.starts_with('+')).count(), d.lines().filter(|l| l.starts_with('-')).count());
        Ok(ToolResult { text: json!({"applied": true, "via": via, "hash": editor::hash(&after), "revert": op, "diff": d}).to_string(), files: vec![file], artifact: Some(format!("source:{op} {stat}")), ..Default::default() })
    }

    fn analyze(&self, args: &Value) -> Result<ToolResult, String> {
        let root = self.root();
        let m = code::manifest(&root);
        let files = code::source_files(&root, &m);
        let opts = spide_lsp::providers::Options {
            run_lsp: args.get("lsp").and_then(Value::as_bool).unwrap_or(true),
            run_linters: args.get("linters").and_then(Value::as_bool).unwrap_or(true),
            timeout: Duration::from_secs(args.get("timeout_s").and_then(Value::as_u64).unwrap_or(180).clamp(5, 1800)),
            ..Default::default()
        };
        let ev = self.evidence.clone();
        let (run, sink) = (self.run.clone(), self.sink.clone());
        let id = self.jobs.spawn_task("analyze", "language servers + linters", None, move |cancel| {
            let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let watcher_flag = flag.clone();
            // mirror the job's cancel flag into the provider's
            let a = std::thread::scope(|sc| {
                sc.spawn(|| {
                    while !cancel.load(std::sync::atomic::Ordering::SeqCst) && !watcher_flag.load(std::sync::atomic::Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    watcher_flag.store(true, std::sync::atomic::Ordering::SeqCst);
                });
                let a = spide_lsp::providers::analyze(&root, &m, &files, &opts, flag.clone());
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
                a
            });
            {
                let mut s = ev.set.lock().unwrap();
                for r in &a.evidence.runs {
                    s.record_run(r.clone());
                }
                let tools: std::collections::BTreeSet<String> = a.evidence.runs.iter().filter(|r| r.ran()).map(|r| r.tool.clone()).collect();
                s.findings.retain(|f| !tools.contains(&f.tool) && !(opts.run_lsp && f.provider == "lsp"));
                s.add(a.evidence.findings.clone());
            }
            ev.save();
            let summary = format!("analysis: {} findings; {}", a.evidence.findings.len(), a.evidence.runs.iter().map(|r| format!("{} {}", r.tool, r.status)).collect::<Vec<_>>().join(", "));
            sink.emit(DomainEvent::Trace { run, summary: summary.clone() });
            sink.emit(DomainEvent::EvidenceChanged);
            if a.cancelled { Err("cancelled".into()) } else { Ok(summary) }
        });
        Ok(ToolResult { text: json!({"job": id}).to_string(), artifact: Some(format!("job:{id}")), ..Default::default() })
    }

    fn run_check(&self, name: &str, timeout_s: u64) -> Result<ToolResult, String> {
        let root = self.root();
        let manifest = std::fs::read_to_string(root.join(spide_core::project::MANIFEST)).map_err(|_| "no spide/project.json: run_check needs a Spide Project".to_string()).and_then(|t| spide_core::project::Manifest::parse(&t))?;
        let cmd = manifest.commands.get(name).ok_or_else(|| format!("no command {name:?} in spide/project.json (have: {})", manifest.commands.keys().cloned().collect::<Vec<_>>().join(", ")))?;
        let trust = self.trust();
        if !trust.approved_commands.iter().any(|c| c == &cmd.run) {
            return Err(format!("command {:?} is not owner-approved; the user must approve it (`spide-headless trust --approve-command {:?}`)", cmd.run, cmd.run));
        }
        let cwd = match &cmd.cwd {
            Some(c) => spide_core::project::normalize(&root.join(c)),
            None => root.clone(),
        };
        let journal_run = self.run.clone();
        let label = format!("{name}: {}", cmd.run);
        let jr = Journal::new(&root);
        let lbl = label.clone();
        let id = self.jobs.spawn_command("check", &label, &cmd.run, &cwd, Duration::from_secs(timeout_s.clamp(1, 3600)), None, move |j| {
            let _ = jr.append(Record {
                kind: "note".into(),
                run: journal_run,
                op: j.id.clone(),
                tool: "run_check".into(),
                intent: lbl.clone(),
                checks: vec![format!("{lbl}: {:?} (exit {:?})", j.state, j.exit_code)],
                ok: Some(j.state == jobs::JobState::Succeeded),
                at: spide_core::store::now_ms(),
                artifact: Some(format!("job:{}", j.id)),
                ..Default::default()
            });
        })?;
        Ok(ToolResult { text: json!({"job": id, "label": label}).to_string(), artifact: Some(format!("job:{id}")), checks: vec![format!("{label}: started")], ..Default::default() })
    }
}

#[cfg(test)]
mod tests;
