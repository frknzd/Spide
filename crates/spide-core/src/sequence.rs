//! UML sequence diagrams: the agent's nested steps ↔ the stored flat form, and the **timeline** both the
//! canvas and the PlantUML export draw from (activations from a call stack, implicit returns, combined
//! fragments, creation and destruction).

use crate::model::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

// ---------------------------------------------------------------- nested steps (agent input)

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Step {
    Fragment(FragmentStep),
    Ref(RefStep),
    Message(Message),
}

#[derive(Debug, Clone, Deserialize)]
pub struct FragmentStep {
    pub fragment: FragmentKind,
    /// guard of a single-operand fragment (loop, opt, break, critical)
    #[serde(default)]
    pub guard: Option<String>,
    /// steps of a single-operand fragment
    #[serde(default)]
    pub steps: Vec<Step>,
    /// operands of alt / par
    #[serde(default)]
    pub operands: Vec<OperandStep>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OperandStep {
    #[serde(default)]
    pub guard: Option<String>,
    #[serde(default)]
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RefStep {
    /// the referenced interaction (another sequence's name)
    #[serde(rename = "ref")]
    pub sequence: String,
    /// lifelines it covers (first and last are enough)
    #[serde(default)]
    pub over: Vec<String>,
}

/// Store nested steps flat: messages in order, fragments as message ranges (outer before inner).
/// Operands without messages and fragments without operands are dropped.
pub fn flatten(steps: &[Step]) -> (Vec<Message>, Vec<Fragment>) {
    fn walk(steps: &[Step], msgs: &mut Vec<Message>, frags: &mut Vec<Fragment>) {
        for s in steps {
            match s {
                Step::Message(m) => msgs.push(m.clone()),
                Step::Ref(r) => msgs.push(Message {
                    from: r.over.first().cloned().unwrap_or_default(),
                    to: r.over.last().cloned().unwrap_or_default(),
                    method: r.sequence.clone(),
                    kind: MessageKind::Ref,
                    ..Default::default()
                }),
                Step::Fragment(f) => {
                    let ops: Vec<OperandStep> = if f.operands.is_empty() { vec![OperandStep { guard: f.guard.clone(), steps: f.steps.clone() }] } else { f.operands.clone() };
                    let at = frags.len();
                    frags.push(Fragment { kind: f.fragment, operands: vec![] });
                    let mut operands = vec![];
                    for o in &ops {
                        let first = msgs.len();
                        walk(&o.steps, msgs, frags);
                        if msgs.len() > first {
                            operands.push(Operand { guard: o.guard.clone().filter(|g| !g.trim().is_empty()), first, last: msgs.len() - 1 });
                        }
                    }
                    frags[at].operands = operands;
                }
            }
        }
    }
    let (mut msgs, mut frags) = (vec![], vec![]);
    walk(steps, &mut msgs, &mut frags);
    frags.retain(|f| !f.operands.is_empty());
    (msgs, frags)
}

fn span(f: &Fragment) -> (usize, usize) {
    (f.operands.first().map(|o| o.first).unwrap_or(0), f.operands.last().map(|o| o.last).unwrap_or(0))
}

/// The fragments that can be drawn: operands in range and in order, and no two fragments crossing.
/// Sorted outer before inner.
pub fn sane_fragments(s: &Sequence) -> Vec<Fragment> {
    let n = s.messages.len();
    let mut ok: Vec<Fragment> = vec![];
    let mut cands: Vec<&Fragment> = s.fragments.iter().collect();
    cands.sort_by_key(|f| {
        let (a, b) = span(f);
        (a, std::cmp::Reverse(b))
    });
    for f in cands {
        if f.operands.is_empty() || f.operands.iter().any(|o| o.first > o.last || o.last >= n) || f.operands.windows(2).any(|w| w[0].last >= w[1].first) {
            continue;
        }
        let (a, b) = span(f);
        // either disjoint from, or nested inside a single operand of, every kept fragment
        let fits = ok.iter().all(|g| {
            let (c, d) = span(g);
            b < c || a > d || g.operands.iter().any(|o| o.first <= a && b <= o.last) || (a <= c && d <= b && f.operands.iter().any(|o| o.first <= c && d <= o.last))
        });
        if fits {
            ok.push(f.clone());
        }
    }
    ok
}

/// The nested steps of a stored sequence (what `get_view` shows the agent).
pub fn to_steps(s: &Sequence) -> Value {
    let frags = sane_fragments(s);
    fn msg(m: &Message) -> Value {
        if m.kind == MessageKind::Ref {
            return json!({"ref": m.method, "over": [m.from, m.to]});
        }
        let mut v = json!({"from": m.from, "to": m.to, "method": m.method, "kind": m.kind});
        for (k, x) in [("args", &m.args), ("returns", &m.returns), ("note", &m.note), ("problem", &m.problem)] {
            if let Some(x) = x {
                v[k] = json!(x);
            }
        }
        v
    }
    fn build(s: &Sequence, frags: &[Fragment], used: &mut Vec<bool>, lo: usize, hi: usize) -> Vec<Value> {
        let mut out = vec![];
        let mut i = lo;
        while i <= hi && i < s.messages.len() {
            // the outermost unused fragment starting here and inside the range
            let f = frags.iter().enumerate().find(|(k, f)| !used[*k] && span(f).0 == i && span(f).1 <= hi);
            match f {
                Some((k, f)) => {
                    used[k] = true;
                    let ops: Vec<Value> = f.operands.iter().map(|o| json!({"guard": o.guard, "steps": build(s, frags, used, o.first, o.last)})).collect();
                    out.push(if ops.len() == 1 && f.kind != FragmentKind::Alt && f.kind != FragmentKind::Par {
                        json!({"fragment": f.kind, "guard": f.operands[0].guard, "steps": ops[0]["steps"]})
                    } else {
                        json!({"fragment": f.kind, "operands": ops})
                    });
                    i = span(f).1 + 1;
                }
                None => {
                    out.push(msg(&s.messages[i]));
                    i += 1;
                }
            }
        }
        out
    }
    let mut used = vec![false; frags.len()];
    let steps = if s.messages.is_empty() { vec![] } else { build(s, &frags, &mut used, 0, s.messages.len() - 1) };
    json!({"name": s.name, "use_case": s.use_case,
        "lifelines": s.participants.iter().map(|p| { let l = s.lifeline(p); json!({"name": l.name, "class": l.class(), "multi": l.multi}) }).collect::<Vec<_>>(),
        "steps": steps})
}

// ---------------------------------------------------------------- timeline

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Event {
    /// a message row (index into `messages`)
    Message(usize),
    /// an implicit reply when an activation ends without an explicit return
    Return { from: String, to: String, label: Option<String>, call: usize },
    /// a combined fragment starts (index into `Timeline::fragments`)
    FragmentStart(usize),
    /// operand `operand` (> 0) of a fragment starts: a separator with its guard
    OperandStart { fragment: usize, operand: usize },
    FragmentEnd(usize),
    Activate { lifeline: String, depth: usize },
    Deactivate { lifeline: String },
    /// the lifeline's header appears here (create message)
    Create(String),
    /// ✗: the lifeline ends here
    Destroy(String),
}

#[derive(Debug, Clone, Default)]
pub struct Timeline {
    pub events: Vec<Event>,
    /// the drawable fragments (outer before inner); `FragmentStart(k)` indexes these
    pub fragments: Vec<Fragment>,
}

struct Frame {
    lifeline: String,
    caller: Option<String>,
    call: Option<usize>,
    self_call: bool,
}

/// Walk the messages with a call stack: who is active when, which replies are implied, where fragments
/// start, split and end, where objects are created and destroyed. Robust to whatever the input says.
pub fn timeline(s: &Sequence, is_actor: &dyn Fn(&str) -> bool) -> Timeline {
    let frags = sane_fragments(s);
    let mut ev: Vec<Event> = vec![];
    let mut stack: Vec<Frame> = vec![];
    let mut marks: Vec<usize> = vec![0; frags.len()];
    let known = |p: &str| s.participants.iter().any(|x| x == p);
    let actor = |p: &str| is_actor(s.class_of(p)) || is_actor(p);
    let depth_of = |stack: &Vec<Frame>, l: &str| stack.iter().filter(|f| f.lifeline == l).count();

    fn pop(stack: &mut Vec<Frame>, ev: &mut Vec<Event>, msgs: &[Message]) {
        let Some(f) = stack.pop() else { return };
        if let (false, Some(caller), Some(call)) = (f.self_call, f.caller.clone(), f.call) {
            ev.push(Event::Return { from: f.lifeline.clone(), to: caller, label: msgs.get(call).and_then(|m| m.returns.clone()).filter(|r| !r.is_empty()), call });
        }
        ev.push(Event::Deactivate { lifeline: f.lifeline });
    }

    for (i, m) in s.messages.iter().enumerate() {
        let acts = m.status != Status::Pruned && m.kind != MessageKind::Ref && known(&m.from) && known(&m.to);
        // the sender must be the active one: what it called before replies first — before a frame opens,
        // so replies to earlier calls aren't drawn inside it
        let mut on_stack = false;
        if acts {
            match stack.iter().rposition(|f| f.lifeline == m.from) {
                Some(pos) => {
                    while stack.len() > pos + 1 {
                        pop(&mut stack, &mut ev, &s.messages);
                    }
                    on_stack = true;
                }
                // a new initiative: everything still active replies first
                None => {
                    while !stack.is_empty() {
                        pop(&mut stack, &mut ev, &s.messages);
                    }
                }
            }
        }
        // fragments (outer first) whose operand starts here
        for (k, f) in frags.iter().enumerate() {
            if f.operands[0].first == i {
                marks[k] = stack.len();
                ev.push(Event::FragmentStart(k));
            }
        }
        if !acts {
            // pruned messages and refs keep their row (pruned ones drawn faded) but don't act;
            // messages between unknown lifelines can't be drawn
            if m.status == Status::Pruned || m.kind == MessageKind::Ref {
                ev.push(Event::Message(i));
            }
        } else {
            if m.kind == MessageKind::Return {
                ev.push(Event::Message(i));
                if on_stack {
                    if let Some(f) = stack.pop() {
                        ev.push(Event::Deactivate { lifeline: f.lifeline });
                    }
                }
                close_operands(i, &frags, &marks, &mut stack, &mut ev, &s.messages);
                continue;
            }
            if !on_stack && !actor(&m.from) {
                ev.push(Event::Activate { lifeline: m.from.clone(), depth: 0 });
                stack.push(Frame { lifeline: m.from.clone(), caller: None, call: None, self_call: false });
            }
            match m.kind {
                MessageKind::Create => {
                    ev.push(Event::Create(m.to.clone()));
                    ev.push(Event::Message(i));
                    ev.push(Event::Activate { lifeline: m.to.clone(), depth: depth_of(&stack, &m.to) });
                    // a constructor's completion isn't drawn as a reply
                    stack.push(Frame { lifeline: m.to.clone(), caller: Some(m.from.clone()), call: Some(i), self_call: true });
                }
                MessageKind::Sync => {
                    ev.push(Event::Message(i));
                    let self_call = m.from == m.to;
                    ev.push(Event::Activate { lifeline: m.to.clone(), depth: depth_of(&stack, &m.to) });
                    stack.push(Frame { lifeline: m.to.clone(), caller: Some(m.from.clone()), call: Some(i), self_call });
                }
                MessageKind::Async => {
                    ev.push(Event::Message(i));
                    // the receiver works on its own: a short bar
                    ev.push(Event::Activate { lifeline: m.to.clone(), depth: depth_of(&stack, &m.to) });
                    ev.push(Event::Deactivate { lifeline: m.to.clone() });
                }
                MessageKind::Destroy => {
                    ev.push(Event::Message(i));
                    // the destroyed object's activations end with it (no replies from a dead object)
                    while let Some(pos) = stack.iter().rposition(|f| f.lifeline == m.to) {
                        while stack.len() > pos + 1 {
                            pop(&mut stack, &mut ev, &s.messages);
                        }
                        let f = stack.pop().unwrap();
                        ev.push(Event::Deactivate { lifeline: f.lifeline });
                    }
                    ev.push(Event::Destroy(m.to.clone()));
                }
                MessageKind::Return | MessageKind::Ref => ev.push(Event::Message(i)),
            }
        }
        close_operands(i, &frags, &marks, &mut stack, &mut ev, &s.messages);
    }
    while !stack.is_empty() {
        pop(&mut stack, &mut ev, &s.messages);
    }

    /// Operands ending at message `i` (inner first): what started inside replies inside the frame.
    fn close_operands(i: usize, frags: &[Fragment], marks: &[usize], stack: &mut Vec<Frame>, ev: &mut Vec<Event>, msgs: &[Message]) {
        for (k, f) in frags.iter().enumerate().rev() {
            for (j, o) in f.operands.iter().enumerate() {
                if o.last != i {
                    continue;
                }
                while stack.len() > marks[k] {
                    pop(stack, ev, msgs);
                }
                if j + 1 < f.operands.len() {
                    ev.push(Event::OperandStart { fragment: k, operand: j + 1 });
                } else {
                    ev.push(Event::FragmentEnd(k));
                }
            }
        }
    }
    Timeline { events: ev, fragments: frags }
}

/// How deeply each fragment is nested (0 = outermost). Fragments are sorted outer before inner, so of
/// two with the same span the earlier one is the outer.
pub fn fragment_depths(t: &Timeline) -> Vec<usize> {
    (0..t.fragments.len())
        .map(|k| {
            let (a, b) = span(&t.fragments[k]);
            t.fragments[..k].iter().filter(|g| {
                let (c, d) = span(g);
                c <= a && b <= d
            }).count()
        })
        .collect()
}

/// Message indices of a fragment (all operands).
pub fn fragment_messages(f: &Fragment) -> std::ops::RangeInclusive<usize> {
    let (a, b) = span(f);
    a..=b
}

/// Lifelines that are created during the interaction (their header starts at the create row).
pub fn created(t: &Timeline) -> Vec<String> {
    t.events.iter().filter_map(|e| if let Event::Create(l) = e { Some(l.clone()) } else { None }).collect()
}

/// `set_sequence` arguments: lifelines + nested `steps` (or the old flat `participants` + `messages`).
pub fn from_input(v: &Value) -> Result<Sequence, String> {
    #[derive(Deserialize)]
    struct Input {
        name: String,
        #[serde(default)]
        use_case: Option<String>,
        #[serde(default)]
        lifelines: Vec<Lifeline>,
        #[serde(default)]
        participants: Vec<String>,
        #[serde(default)]
        messages: Vec<Message>,
        #[serde(default)]
        steps: Vec<Step>,
        #[serde(default)]
        fragments: Vec<Fragment>,
    }
    let i: Input = serde_json::from_value(v.clone()).map_err(|e| format!("invalid arguments: {e}"))?;
    let (messages, fragments) = if i.steps.is_empty() { (i.messages, i.fragments) } else { flatten(&i.steps) };
    let mut participants: Vec<String> = i.lifelines.iter().map(|l| l.name.clone()).collect();
    for p in i.participants {
        if !participants.contains(&p) {
            participants.push(p);
        }
    }
    let lifelines = i.lifelines.into_iter().filter(|l| !l.name.trim().is_empty()).map(|mut l| {
        if l.class.trim().is_empty() {
            l.class = l.name.clone();
        }
        l
    }).collect();
    Ok(Sequence { name: i.name, use_case: i.use_case, participants, lifelines, messages, fragments, ..Default::default() })
}
