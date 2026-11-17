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
    /// Portable source identity; local repository path is stored separately.
    pub source: Option<SourceSymbol>,
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
    /// What the implementation actually does (imported code only).
    pub code: Option<CodeFacts>,
    pub status: Status,
    pub proposal: Option<String>,
    /// Where it came from: decision id (D…), proposal id (P…), `user` or `import`.
    pub origin: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SourceSymbol {
    /// Qualified symbol: dotted container path + name (`org.shop.Order`, `crate.orders.Order`,
    /// `shop.orders`), methods as `type#name(parameter types)`. Java links are fully qualified names.
    pub symbol: String,
    /// Workspace-relative file.
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
    /// rust | typescript | python | java | c | cpp | clojure (absent in schema-1 designs: java).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// module | namespace | class | struct | enum | trait | interface | protocol | record | union |
    /// function | method | field | macro
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Stable persisted id (`symbols::stable_id`): positions are locations, never identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// 0-based UTF-8 byte column of the declaration's start (navigation).
    #[serde(skip_serializing_if = "is_zero")]
    pub start_col: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

impl SourceSymbol {
    /// Language of the symbol (schema-1 links are Java).
    pub fn lang(&self) -> &str {
        self.language.as_deref().unwrap_or("java")
    }
}

/// Facts about a method body, from the imported source (never part of an intended design).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CodeFacts {
    /// Source file, relative to the imported folder.
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
    /// Lines of code (non-blank, non-comment) and statements in the body.
    pub loc: u32,
    pub statements: u32,
    /// Types it instantiates (`new X(…)`).
    pub creates: Vec<String>,
    /// Longest receiver chain (`a.getB().getC().x()` = 3) and the chains of length ≥ 3.
    pub max_chain: u32,
    pub chains: Vec<String>,
    /// `switch` statements/expressions and `instanceof` checks.
    pub switches: u32,
    pub instanceofs: u32,
    /// Enum constants or types it switches on (`case BCL:`).
    pub switch_on: Vec<String>,
    /// Arithmetic and comparison operators, loops.
    pub arithmetic: u32,
    pub comparisons: u32,
    pub loops: u32,
    /// UI toolkit calls/types it uses (javax.swing, JOptionPane, …).
    pub ui_api: Vec<String>,
    /// Winnowed k-gram fingerprint of the normalised body (identifiers → ID, literals → LIT).
    pub fingerprint: Vec<u64>,
    /// Hash of the body text (assessment staleness).
    pub hash: String,
    /// Decision points (if/loop/case/catch/&&/||/ternary): cyclomatic complexity = branches + 1.
    pub branches: u32,
    /// Deepest nesting of blocks inside the body.
    pub max_nesting: u32,
    /// Parameters the function takes (incl. receiver-less functions).
    pub params: u32,
    /// Constructs a profile treats specially (`unsafe`, `unwrap`, `goto`, `any`, `eval`, `atom`, …): name → count.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub idioms: std::collections::BTreeMap<String, u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Class {
    pub name: String,
    pub source: Option<SourceSymbol>,
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
    /// A contract: interface, Rust trait or Clojure/Python protocol.
    pub fn is_interface(&self) -> bool {
        matches!(self.stereotype.as_deref(), Some("interface" | "trait" | "protocol"))
    }
    /// A module/namespace node (free functions), not an object type.
    pub fn is_module(&self) -> bool {
        matches!(self.stereotype.as_deref(), Some("module" | "namespace"))
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
    /// Informational provenance, never an examiner verdict on amended operations.
    pub decision: Option<String>,
    pub option: Option<String>,
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
    /// Internal editable operation plan (legacy P IDs remain stable aliases).
    pub plan: Option<String>,
    /// Optional user guidance, never an examiner gate.
    pub notes: String,
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

/// A concept of the problem domain (conceptual class): attributes only, no behaviour.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Concept {
    pub name: String,
    pub description: Option<String>,
    pub attributes: Vec<ConceptAttribute>,
    pub status: Status,
    pub feedback: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ConceptAttribute {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: Option<String>,
    pub multiplicity: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConceptLinkKind {
    #[default]
    Association,
    Generalization,
    Composition,
    Aggregation,
}

impl ConceptLinkKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ConceptLinkKind::Association => "association",
            ConceptLinkKind::Generalization => "generalization",
            ConceptLinkKind::Composition => "composition",
            ConceptLinkKind::Aggregation => "aggregation",
        }
    }
}

/// An association between concepts. Generalization: from = specific, to = general.
/// Composition/aggregation: from = whole, to = part.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ConceptLink {
    pub id: String,
    pub from: String,
    pub to: String,
    pub kind: ConceptLinkKind,
    /// Verb phrase, e.g. "records".
    pub label: Option<String>,
    pub from_mult: Option<String>,
    pub to_mult: Option<String>,
    pub status: Status,
    pub feedback: Option<String>,
}

/// The conceptual domain model, built by the agent. Separate from the class design: no references to it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DomainModel {
    pub concepts: Vec<Concept>,
    pub associations: Vec<ConceptLink>,
    /// Class-model revision this was generated from.
    pub generated_rev: Option<u64>,
}

impl DomainModel {
    pub fn is_empty(&self) -> bool {
        !self.concepts.iter().any(|c| c.status != Status::Pruned)
    }
    /// The model as plain classes and relations for drawing (descriptions become the one
    /// responsibility line; generalization → inheritance).
    pub fn as_class_model(&self) -> (Vec<Class>, Vec<Relation>) {
        let classes = self
            .concepts
            .iter()
            .map(|c| Class {
                name: c.name.clone(),
                attributes: c
                    .attributes
                    .iter()
                    .map(|a| Attribute { name: a.name.clone(), ty: a.ty.clone().unwrap_or_default(), multiplicity: a.multiplicity.clone(), status: Status::Accepted, ..Default::default() })
                    .collect(),
                responsibilities: c.description.iter().cloned().collect(),
                status: c.status,
                ..Default::default()
            })
            .collect();
        let relations = self
            .associations
            .iter()
            .map(|a| Relation {
                id: a.id.clone(),
                from: a.from.clone(),
                to: a.to.clone(),
                kind: match a.kind {
                    ConceptLinkKind::Association => RelationKind::Association,
                    ConceptLinkKind::Generalization => RelationKind::Inheritance,
                    ConceptLinkKind::Composition => RelationKind::Composition,
                    ConceptLinkKind::Aggregation => RelationKind::Aggregation,
                },
                label: a.label.clone(),
                from_mult: a.from_mult.clone(),
                to_mult: a.to_mult.clone(),
                status: a.status,
                ..Default::default()
            })
            .collect();
        (classes, relations)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    #[default]
    Sync,
    Async,
    Return,
    Create,
    /// «destroy»: ends the target's lifeline with ✗ (`from == to`: the object ends itself).
    Destroy,
    /// Interaction use: `ref <method>` (another sequence) over the lifelines `from`..`to`.
    Ref,
}

impl MessageKind {
    /// A call that makes the receiver do something (and is numbered).
    pub fn is_call(self) -> bool {
        matches!(self, MessageKind::Sync | MessageKind::Async | MessageKind::Create)
    }
}

/// A lifeline: an object (`name : Class`) or an actor (`class == name`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Lifeline {
    /// Instance name and the id messages use in `from`/`to`.
    pub name: String,
    /// The class (or the actor's name). Empty: same as `name`.
    pub class: String,
    /// A multi-object (`loans : Loan[*]`).
    pub multi: bool,
}

impl Lifeline {
    pub fn class(&self) -> &str {
        if self.class.is_empty() { &self.name } else { &self.class }
    }
    /// `l : Loan`, `: Loan` when the name is the class, `loans : Loan[*]`.
    pub fn label(&self, actor: bool) -> String {
        if actor {
            return self.name.clone();
        }
        let multi = if self.multi { "[*]" } else { "" };
        if self.name == self.class() { format!(": {}{multi}", self.class()) } else { format!("{} : {}{multi}", self.name, self.class()) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FragmentKind {
    #[default]
    Loop,
    Alt,
    Opt,
    Par,
    Break,
    Critical,
}

impl FragmentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            FragmentKind::Loop => "loop",
            FragmentKind::Alt => "alt",
            FragmentKind::Opt => "opt",
            FragmentKind::Par => "par",
            FragmentKind::Break => "break",
            FragmentKind::Critical => "critical",
        }
    }
}

/// One operand of a combined fragment: messages `first..=last` (indices into `Sequence.messages`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Operand {
    pub guard: Option<String>,
    pub first: usize,
    pub last: usize,
}

/// A combined fragment (`loop`, `alt`, …); fragments nest by containment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Fragment {
    pub kind: FragmentKind,
    pub operands: Vec<Operand>,
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
    /// Lifeline ids, left → right.
    pub participants: Vec<String>,
    /// Instance names and classes of the lifelines (missing: the participant is the class).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lifelines: Vec<Lifeline>,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fragments: Vec<Fragment>,
    pub generated_rev: Option<u64>,
    pub status: Status,
    pub feedback: Option<String>,
}

impl Sequence {
    /// The lifeline with this id (a plain participant is its own class).
    pub fn lifeline(&self, id: &str) -> Lifeline {
        self.lifelines.iter().find(|l| l.name == id).cloned().unwrap_or_else(|| Lifeline { name: id.to_string(), class: id.to_string(), multi: false })
    }
    /// The class a lifeline id stands for.
    pub fn class_of<'a>(&'a self, id: &'a str) -> &'a str {
        self.lifelines.iter().find(|l| l.name == id).map(Lifeline::class).unwrap_or(id)
    }
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

/// A comment from outside Spide (a Lucid comment thread or a colleague's sticky note), kept so the
/// agent sees it in later sessions and can link the design change that answers it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Comment {
    /// `C1`, `C2` …
    pub id: String,
    /// `lucid`
    pub source: String,
    /// The diagram it was made on (`class`, `sequence:Borrow` …).
    pub diagram: String,
    /// The source's id: the Lucid thread id, or `note:<item id>` for a sticky note.
    pub thread: String,
    /// The element it is about (Spide id such as `class:Order`), when pinned to or next to one.
    pub target: Option<String>,
    pub entries: Vec<CommentEntry>,
    pub resolved: bool,
    /// The decision or proposal that answers it (set by the agent).
    pub addressed_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CommentEntry {
    pub author: String,
    pub text: String,
    /// ISO-8601
    pub at: String,
}

impl Comment {
    pub fn is_open(&self) -> bool {
        !self.resolved && self.addressed_by.is_none()
    }
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
    /// Version of the unified review migration, independent of source schema.
    #[serde(default)]
    pub review_version: u32,
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
    /// The conceptual domain model (agent-built, independent of the classes).
    #[serde(default)]
    pub domain: DomainModel,
    pub sequences: Vec<Sequence>,
    pub change_scenarios: Vec<ChangeScenario>,
    pub feedback: Vec<Feedback>,
    /// Comments from collaborators (Lucid threads and notes).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<Comment>,
    pub log: Vec<LogEntry>,
    /// Canvas positions per view and element (e.g. `class` → `Order` → pos).
    pub layout: BTreeMap<String, BTreeMap<String, Pos>>,
    pub settings: Settings,
    /// Architecture rules (style preset, forbidden dependencies/types, thresholds).
    pub rules: crate::rules::Rules,
    pub counter: u64,
    /// The agent's judgement per class (purpose, role, per-method verdicts), layered on Spide's signals.
    pub assessments: BTreeMap<String, crate::assess::Assessment>,
    /// The agent's judgement per health dimension (extensibility, modularity, simplicity,
    /// implementation_quality): evidence-linked, stale when what it cites changes.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub health_assessments: BTreeMap<String, crate::health::DimensionAssessment>,
    /// Contextual exceptions to health rules: proposed by agents, approved only by the owner.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub health_exceptions: Vec<crate::health::HealthException>,
    /// Owner's quality priorities (dimension → weight); empty: equal weights.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub health_priorities: BTreeMap<String, f64>,
}

impl Default for Design {
    fn default() -> Self {
        Design {
            version: SCHEMA_VERSION,
            review_version: 1,
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
            domain: DomainModel::default(),
            sequences: vec![],
            change_scenarios: vec![],
            feedback: vec![],
            comments: vec![],
            log: vec![],
            layout: BTreeMap::new(),
            settings: Settings::default(),
            rules: crate::rules::Rules::default(),
            counter: 0,
            assessments: BTreeMap::new(),
            health_assessments: BTreeMap::new(),
            health_exceptions: vec![],
            health_priorities: BTreeMap::new(),
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
