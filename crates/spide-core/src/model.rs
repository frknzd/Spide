//! Design model: the single source of truth for everything Spide shows.
//!
//! Classes are identified by their (unique) name, members by their name within
//! a class, and relations by `from->to:kind`. Agents talk about names, so this
//! keeps the agent API simple.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Part of the design.
    #[default]
    Accepted,
    /// Shown as a ghost: proposed by an agent, not yet accepted.
    Proposed,
    /// Proposed for removal (ghost of a removal).
    Removing,
    /// Removed by the user; kept faded with the user's feedback.
    Pruned,
    /// Applied although the examiner rejected the defence.
    Overruled,
}

impl Status {
    /// Elements that count for metrics and derived views.
    pub fn is_live(self) -> bool {
        matches!(self, Status::Accepted | Status::Overruled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    #[default]
    Public,
    Private,
    Protected,
    Package,
}

impl Visibility {
    pub fn glyph(self) -> &'static str {
        match self {
            Visibility::Public => "+",
            Visibility::Private => "-",
            Visibility::Protected => "#",
            Visibility::Package => "~",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Attribute {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub visibility: Visibility,
    pub is_static: bool,
    pub multiplicity: Option<String>,
    pub status: Status,
    pub proposal: Option<String>,
    /// Where it came from: decision id (D…), proposal id (P…), `user` or `import`.
    pub origin: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Param {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Method {
    pub name: String,
    pub params: Vec<Param>,
    pub returns: Option<String>,
    pub visibility: Visibility,
    pub is_static: bool,
    pub is_abstract: bool,
    /// Attribute names of the owning class this method reads/writes (for LCOM4).
    pub uses: Vec<String>,
    /// Calls to other methods: `"Class.method"` or `"method"` (same class).
    pub calls: Vec<String>,
    pub description: Option<String>,
    /// What kind of work it does, set by the agent/user when the name doesn't say it
    /// (overrides the verb heuristics): presentation | coordination | domain_logic | persistence |
    /// formatting | creation | accessor.
    pub concern: Option<String>,
    pub status: Status,
    pub proposal: Option<String>,
    /// Where it came from: decision id (D…), proposal id (P…), `user` or `import`.
    pub origin: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Class {
    pub name: String,
    /// e.g. `interface`, `abstract`, `entity`, `service`, `controller`, `repository`, `enum`
    pub stereotype: Option<String>,
    pub package: Option<String>,
    pub is_abstract: bool,
    pub attributes: Vec<Attribute>,
    pub methods: Vec<Method>,
    pub responsibilities: Vec<String>,
    pub rationale: Option<String>,
    /// Consistency boundary: one transaction should change at most one aggregate.
    pub aggregate_root: bool,
    /// Belongs to an external library/system (not designed here): drawn compactly, coupling to it
    /// counts less, and it is left out of the responsibility checks.
    pub external: bool,
    /// Intended purpose, set by the agent or user (overrides the name/stereotype heuristics):
    /// presentation | controller | service | domain | persistence | adapter | output | creator | utility.
    pub role: Option<String>,
    pub status: Status,
    pub proposal: Option<String>,
    /// Where it came from: decision id (D…), proposal id (P…), `user` or `import`.
    pub origin: Option<String>,
    pub feedback: Option<String>,
}

impl Class {
    pub fn is_interface(&self) -> bool {
        self.stereotype.as_deref() == Some("interface")
    }
    pub fn is_enum(&self) -> bool {
        self.stereotype.as_deref() == Some("enum")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    #[default]
    Association,
    Aggregation,
    Composition,
    Inheritance,
    Realization,
    Dependency,
}

impl RelationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RelationKind::Association => "association",
            RelationKind::Aggregation => "aggregation",
            RelationKind::Composition => "composition",
            RelationKind::Inheritance => "inheritance",
            RelationKind::Realization => "realization",
            RelationKind::Dependency => "dependency",
        }
    }
}

/// `from` is the source of the dependency: the whole for aggregation/composition,
/// the child for inheritance/realization, the client for association/dependency.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Relation {
    pub id: String,
    pub from: String,
    pub to: String,
    pub kind: RelationKind,
    pub label: Option<String>,
    pub from_mult: Option<String>,
    pub to_mult: Option<String>,
    /// For associations: navigable from→to only (true) or bidirectional (false).
    pub directed: bool,
    pub interaction: Interaction,
    pub rationale: Option<String>,
    pub status: Status,
    pub proposal: Option<String>,
    /// Where it came from: decision id (D…), proposal id (P…), `user` or `import`.
    pub origin: Option<String>,
    pub feedback: Option<String>,
}

/// How a dependency is exercised at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Interaction {
    /// Caller waits for the callee.
    #[default]
    Sync,
    /// Fire-and-forget call / queue.
    Async,
    /// Publish/subscribe event.
    Event,
}

/// A deployable/runnable unit (C4 container): web app, API, database, worker…
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Container {
    pub name: String,
    pub technology: Option<String>,
    pub description: Option<String>,
    /// Running instances; 1 = single point of failure when others depend on it.
    pub replicas: u32,
    /// Datastores hold state (databases, queues, caches).
    pub datastore: bool,
    pub status: Status,
    pub proposal: Option<String>,
}

impl Default for Container {
    fn default() -> Self {
        Container { name: String::new(), technology: None, description: None, replicas: 1, datastore: false, status: Status::Accepted, proposal: None }
    }
}

/// An external system our system talks to (C4 context level): payment provider, IdP, mail…
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ExternalSystem {
    pub name: String,
    pub description: Option<String>,
    pub status: Status,
    pub proposal: Option<String>,
}

/// A runtime connection that is not derived from class dependencies: container ↔ external system,
/// actor → container, container → container through a protocol.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Link {
    pub from: String,
    pub to: String,
    pub interaction: Interaction,
    /// e.g. HTTPS/JSON, AMQP, JDBC
    pub protocol: Option<String>,
    pub description: Option<String>,
    pub status: Status,
    pub proposal: Option<String>,
}

impl Link {
    pub fn id(&self) -> String {
        format!("{}->{}", self.from, self.to)
    }
}

pub fn relation_id(from: &str, to: &str, kind: RelationKind) -> String {
    format!("{from}->{to}:{}", kind.as_str())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    Ui,
    Infrastructure,
    Application,
    Domain,
}

impl Layer {
    /// Clean-architecture ring: outer (0) → inner (2). Dependencies must point inward.
    pub fn rank(self) -> u8 {
        match self {
            Layer::Ui | Layer::Infrastructure => 0,
            Layer::Application => 1,
            Layer::Domain => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Package {
    pub name: String,
    pub layer: Option<Layer>,
    pub description: Option<String>,
    /// Container (deployable unit) this package runs in.
    pub container: Option<String>,
    /// An external library/system's package (e.g. a framework you build on).
    pub external: bool,
    /// C4 external system this package belongs to (an `ExternalSystem` name).
    pub system: Option<String>,
    pub status: Status,
    pub proposal: Option<String>,
    /// Where it came from: decision id (D…), proposal id (P…), `user` or `import`.
    pub origin: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Proposal {
    pub id: String,
    pub ops: Vec<crate::ops::Op>,
    pub rationale: Option<String>,
    /// `agent`, `file`, …
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DecisionOption {
    pub id: String,
    pub label: String,
    pub summary: Option<String>,
    pub ops: Vec<crate::ops::Op>,
    pub pros: Vec<String>,
    pub cons: Vec<String>,
    /// How this option affects the design forces (brief).
    pub tradeoffs: Vec<Tradeoff>,
}

/// Effect of a decision option on one force of the brief.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Tradeoff {
    /// Force name (see `Design::forces`).
    pub force: String,
    /// `+` (helps) or `-` (hurts).
    pub effect: String,
    pub note: Option<String>,
}

impl Tradeoff {
    pub fn helps(&self) -> bool {
        !self.effect.trim_start().starts_with('-')
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ForceKind {
    /// What the system must achieve.
    #[default]
    Goal,
    /// Quality attribute (performance, evolvability, testability, …).
    Quality,
    /// Fixed constraint (technology, budget, regulation, team).
    Constraint,
    /// Expected change the design should absorb.
    Change,
}

impl ForceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ForceKind::Goal => "goal",
            ForceKind::Quality => "quality",
            ForceKind::Constraint => "constraint",
            ForceKind::Change => "change",
        }
    }
}

/// A design force: part of the brief the design is judged against.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Force {
    pub name: String,
    pub kind: ForceKind,
    pub description: Option<String>,
    /// 1 = must, 2 = should, 3 = nice to have.
    pub priority: u8,
    pub status: Status,
    pub proposal: Option<String>,
}

impl Default for Force {
    fn default() -> Self {
        Force { name: String::new(), kind: ForceKind::Goal, description: None, priority: 2, status: Status::Accepted, proposal: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DecisionState {
    /// Waiting for the user to choose and defend.
    #[default]
    Open,
    /// Defence sent to the examiner.
    Evaluating,
    /// Examiner rejected; user may revise (or overrule in lenient mode).
    Rejected,
    Accepted,
    Overruled,
    Dismissed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Attempt {
    pub option: Option<String>,
    pub custom: Option<String>,
    pub defense: String,
    pub verdict: Option<String>,
    pub feedback: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Decision {
    pub id: String,
    pub question: String,
    pub context: Option<String>,
    pub options: Vec<DecisionOption>,
    pub state: DecisionState,
    pub chosen: Option<String>,
    pub custom: Option<String>,
    pub attempts: Vec<Attempt>,
    /// Ops actually applied (option ops, or examiner-provided ops for a custom answer).
    pub applied_ops: Vec<crate::ops::Op>,
    /// What applying it did to the design (recorded when it was applied).
    pub consequences: Option<crate::impact::Impact>,
    /// When it was accepted / overruled (ms since epoch).
    pub decided_at: Option<u64>,
}

impl Decision {
    pub fn is_pending(&self) -> bool {
        matches!(self.state, DecisionState::Open | DecisionState::Evaluating | DecisionState::Rejected)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Actor {
    pub name: String,
    pub description: Option<String>,
    pub status: Status,
    pub feedback: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct UseCase {
    pub name: String,
    pub actors: Vec<String>,
    pub includes: Vec<String>,
    pub extends: Vec<String>,
    /// Classes that realise this use case.
    pub classes: Vec<String>,
    pub description: Option<String>,
    pub status: Status,
    pub feedback: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct UseCaseModel {
    pub system: Option<String>,
    pub actors: Vec<Actor>,
    pub use_cases: Vec<UseCase>,
    /// Class-model revision this was generated from.
    pub generated_rev: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    #[default]
    Sync,
    Async,
    Return,
    Create,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Message {
    pub from: String,
    pub to: String,
    /// Method name on `to` (for sync/async/create), or a label for returns.
    pub method: String,
    pub args: Option<String>,
    pub returns: Option<String>,
    pub kind: MessageKind,
    pub note: Option<String>,
    /// Filled by validation: why this message doesn't match the class model.
    pub problem: Option<String>,
    pub status: Status,
    pub feedback: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Sequence {
    pub name: String,
    pub use_case: Option<String>,
    pub participants: Vec<String>,
    pub messages: Vec<Message>,
    pub generated_rev: Option<u64>,
    pub status: Status,
    pub feedback: Option<String>,
}

/// A class an expected change would touch, and why.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ImpactedClass {
    pub class: String,
    pub why: Option<String>,
}

/// "What would change if …?" — a change scenario used to stress-test modularity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ChangeScenario {
    pub name: String,
    pub description: Option<String>,
    /// Classes that must change (filled by the agent's analysis).
    pub impacted: Vec<ImpactedClass>,
    /// Class-model revision the analysis was made for.
    pub generated_rev: Option<u64>,
    pub status: Status,
    pub proposal: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Feedback {
    pub target: String,
    pub text: String,
    pub at: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LogEntry {
    pub rev: u64,
    pub at: u64,
    pub actor: String,
    pub summary: String,
}

impl Default for LogEntry {
    fn default() -> Self {
        LogEntry { rev: 0, at: 0, actor: String::new(), summary: String::new() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExaminerMode {
    #[default]
    Lenient,
    Strict,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Settings {
    pub examiner: ExaminerMode,
    pub show_pruned: bool,
    pub partner: PartnerMode,
    /// The user skipped the brief (forces) — don't nag about it.
    pub brief_skipped: bool,
    /// Java root package of the real code (ArchUnit export, code import), e.g. `org.acme.library`.
    pub code_root: Option<String>,
    /// Source folder last imported (re-import with /import).
    pub code_dir: Option<String>,
    /// Granularity per view (`class`, `domain`): 0 packages · 1 names · 2 responsibilities ·
    /// 3 design (default) · 4 full. Diagrams and exports follow it.
    pub detail: BTreeMap<String, u8>,
    /// Packages Spide already suggested marking external (suggested once).
    pub external_suggested: Vec<String>,
}

/// Default granularity: the exam-style "design" level.
pub const DEFAULT_DETAIL: u8 = 3;

impl Settings {
    pub fn detail_for(&self, view: &str) -> u8 {
        self.detail.get(view).copied().unwrap_or(DEFAULT_DETAIL).min(4)
    }
}

/// How the AI partner behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PartnerMode {
    /// Raises decisions, makes the user defend them (default).
    #[default]
    Examiner,
    /// Only asks questions; never proposes changes.
    Socratic,
    /// Argues against the user's choice before examining it.
    DevilsAdvocate,
    /// Proposes freely; decisions only for big forks.
    Sketch,
}

impl PartnerMode {
    pub fn as_str(self) -> &'static str {
        match self {
            PartnerMode::Examiner => "examiner",
            PartnerMode::Socratic => "socratic",
            PartnerMode::DevilsAdvocate => "devils_advocate",
            PartnerMode::Sketch => "sketch",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_lowercase().replace(['-', ' '], "_").as_str() {
            "examiner" | "exam" => PartnerMode::Examiner,
            "socratic" | "questions" => PartnerMode::Socratic,
            "devils_advocate" | "devil" | "advocate" => PartnerMode::DevilsAdvocate,
            "sketch" | "free" => PartnerMode::Sketch,
            _ => return None,
        })
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct Pos {
    pub x: f64,
    pub y: f64,
    /// Pinned by the user: the living layout doesn't move it (and tidy leaves it alone).
    #[serde(default, skip_serializing_if = "is_false")]
    pub pinned: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Design {
    pub version: u32,
    pub name: String,
    pub rev: u64,
    /// Revision of the last change to the class model (drives staleness of derived views).
    pub model_rev: u64,
    /// The brief: goals, quality attributes, constraints, expected changes.
    pub forces: Vec<Force>,
    pub classes: Vec<Class>,
    pub relations: Vec<Relation>,
    pub packages: Vec<Package>,
    /// Deployable units (C4 containers); packages are assigned to them.
    pub containers: Vec<Container>,
    /// External systems (C4 context).
    pub systems: Vec<ExternalSystem>,
    /// Runtime links between containers, systems and actors.
    pub links: Vec<Link>,
    pub proposals: Vec<Proposal>,
    pub decisions: Vec<Decision>,
    pub use_cases: UseCaseModel,
    pub sequences: Vec<Sequence>,
    pub change_scenarios: Vec<ChangeScenario>,
    pub feedback: Vec<Feedback>,
    pub log: Vec<LogEntry>,
    /// Canvas positions per view and element (e.g. `class` → `Order` → pos).
    pub layout: BTreeMap<String, BTreeMap<String, Pos>>,
    pub settings: Settings,
    /// Architecture rules (style preset, forbidden dependencies/types, thresholds).
    pub rules: crate::rules::Rules,
    pub counter: u64,
}

impl Default for Design {
    fn default() -> Self {
        Design {
            version: SCHEMA_VERSION,
            name: "Untitled design".into(),
            rev: 0,
            model_rev: 0,
            forces: vec![],
            classes: vec![],
            relations: vec![],
            packages: vec![],
            containers: vec![],
            systems: vec![],
            links: vec![],
            proposals: vec![],
            decisions: vec![],
            use_cases: UseCaseModel::default(),
            sequences: vec![],
            change_scenarios: vec![],
            feedback: vec![],
            log: vec![],
            layout: BTreeMap::new(),
            settings: Settings::default(),
            rules: crate::rules::Rules::default(),
            counter: 0,
        }
    }
}

impl Design {
    pub fn class(&self, name: &str) -> Option<&Class> {
        self.classes.iter().find(|c| c.name == name)
    }
    /// The class is external itself or lives in an external package.
    pub fn is_external(&self, class: &str) -> bool {
        self.class(class).is_some_and(|c| {
            c.external || c.package.as_deref().is_some_and(|p| self.packages.iter().any(|x| x.name == p && x.external && x.status != Status::Pruned))
        })
    }
    pub fn class_mut(&mut self, name: &str) -> Option<&mut Class> {
        self.classes.iter_mut().find(|c| c.name == name)
    }
    pub fn live_class(&self, name: &str) -> Option<&Class> {
        self.class(name).filter(|c| c.status.is_live())
    }
    pub fn live_classes(&self) -> impl Iterator<Item = &Class> {
        self.classes.iter().filter(|c| c.status.is_live())
    }
    /// Relations whose own status and both endpoints are live.
    pub fn live_relations(&self) -> impl Iterator<Item = &Relation> {
        self.relations.iter().filter(move |r| {
            r.status.is_live() && self.live_class(&r.from).is_some() && self.live_class(&r.to).is_some()
        })
    }
    pub fn force(&self, name: &str) -> Option<&Force> {
        self.forces.iter().find(|f| f.name == name)
    }
    pub fn live_forces(&self) -> impl Iterator<Item = &Force> {
        self.forces.iter().filter(|f| f.status.is_live())
    }
    pub fn next_id(&mut self, prefix: &str) -> String {
        self.counter += 1;
        format!("{prefix}{}", self.counter)
    }
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("design serialises")
    }
    pub fn from_json(s: &str) -> Result<Design, serde_json::Error> {
        serde_json::from_str(s)
    }
}
