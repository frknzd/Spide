import { useEffect, useRef, useState } from "react";
import { api } from "../state/api";
import { fieldDiffs, nextProposalId, operationLabel, operationTarget, samePlan, loadPlanDraft, storePlanDraft, type PlanDraft } from "../state/proposal-review";
import { ImpactLine, ImpactDetails } from "./Impact";
import { ProposalOperationEditor } from "./ProposalOperationEditor";
import type { Design, Proposal, ProposalReviewData, Snapshot } from "../state/types";

interface Props {
  snap: Snapshot; selected: string | null; active: boolean;
  onSelect: (id: string | null) => void; onPreview: (d: Design | null) => void;
  onReviewData: (r: ProposalReviewData | null) => void; onFocus: (name: string) => void;
  onDirty: (dirty: boolean) => void;
}
export function ProposalReview({ snap, selected, active, onSelect, onPreview, onReviewData, onFocus, onDirty }: Props) {
  const plans = snap.view.proposals;
  const previousIds = useRef<string[]>([]);
  const id = nextProposalId(plans.map((p) => p.id), selected, previousIds.current);
  const saved = plans.find((p) => p.id === id);
  const scope = `${snap.project_dir}:${snap.branch}:${id ?? ""}`;
  const [session, setSession] = useState<PlanDraft | null>(null);
  const [editing, setEditing] = useState<number | null>(null);
  const addButton = useRef<HTMLButtonElement>(null);
  const closeEditor = () => { setEditing(null); requestAnimationFrame(() => addButton.current?.focus()); };
  const editOperation = (index: number) => {
    if (editing !== null && !window.confirm("Discard the unfinished operation form and edit another change?")) return;
    setEditing(index);
  };
  const [review, setReview] = useState<ProposalReviewData | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(false);
  const [baseline, setBaseline] = useState(false);
  const [previewKey, setPreviewKey] = useState("");
  const p = session?.scope === scope ? session.plan : saved;
  const dirty = !!session && session.scope === scope && !samePlan(session.plan, session.original);
  const conflict = !!session && (dirty || editing !== null) && session.rev !== snap.rev;
  const requestKey = JSON.stringify([scope, snap.rev, p?.ops]);
  const validPreview = previewKey === requestKey && !loading && !!review && review.rev === snap.rev && !review.errors.length;
  const at = Math.max(0, plans.findIndex((x) => x.id === id));

  useEffect(() => {
    previousIds.current = plans.map((p) => p.id);
    if (selected !== id) onSelect(id);
  }, [plans, id, selected, onSelect]);
  useEffect(() => {
    setSession(saved ? loadPlanDraft(scope) ?? { scope, plan: structuredClone(saved), original: structuredClone(saved), rev: snap.rev } : null);
    setEditing(null); setError(""); setReview(null); setBaseline(false);
  }, [scope]);
  useEffect(() => {
    if (saved && session?.scope === scope && !dirty && editing === null && snap.rev > session.rev) setSession({ scope, plan: structuredClone(saved), original: structuredClone(saved), rev: snap.rev });
  }, [snap.rev, saved, scope, dirty, editing, session]);
  useEffect(() => { onDirty(dirty || editing !== null); }, [dirty, editing, onDirty]);
  useEffect(() => {
    if (!dirty && editing === null) return;
    const leave = (e: BeforeUnloadEvent) => { e.preventDefault(); e.returnValue = ""; };
    window.addEventListener("beforeunload", leave);
    return () => window.removeEventListener("beforeunload", leave);
  }, [dirty, editing]);
  useEffect(() => {
    let live = true;
    setLoading(true); setReview(null); onPreview(null); onReviewData(null);
    if (!p || !active) { setLoading(false); return; }
    const timer = setTimeout(() => {
      api.reviewProposal(p.id, p.ops).then((r) => {
        if (live) { setReview(r); setPreviewKey(requestKey); setLoading(false); }
      }).catch((e) => { if (live) { setError(String(e)); setLoading(false); } });
    }, 200);
    return () => { live = false; clearTimeout(timer); };
  }, [requestKey, active, onPreview, onReviewData]);
  useEffect(() => {
    const canvas = review ? baseline ? review.baseline : review.design : null;
    onPreview(active && canvas ? { ...canvas, proposals: canvas.proposals.map((x) => p && x.id === p.id ? p : x) } : null);
    onReviewData(active ? review : null);
  }, [review, baseline, active, p, onPreview, onReviewData]);
  const change = (patch: Partial<Proposal>) => {
    if (!p || busy) return;
    const next = { scope, plan: { ...p, ...patch }, original: session?.scope === scope ? session.original : structuredClone(saved!), rev: session?.scope === scope ? session.rev : snap.rev };
    setSession(next); storePlanDraft(next); setError("");
  };
  const select = (next: string) => {
    if (busy || ((dirty || editing !== null) && !window.confirm("Leave this plan? Saved draft changes will be kept locally; unfinished operation form edits will be lost."))) return;
    onSelect(next);
  };
  const discard = () => {
    if (!saved || !window.confirm("Discard draft changes and reload the saved plan?")) return;
    const next = { scope, plan: structuredClone(saved), original: structuredClone(saved), rev: snap.rev };
    setSession(next); storePlanDraft(next); setEditing(null); setError("");
  };
  const save = async () => {
    if (!p || !session || busy || conflict || !validPreview || editing !== null) return;
    setBusy(true); setError("");
    try {
      await api.updateProposal(p, session.rev);
      const next = { ...session, original: structuredClone(p), rev: session.rev + 1 };
      setSession(next); storePlanDraft(next);
    } catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };
  const resolve = async (accept: boolean) => {
    if (!p || busy || (accept && (dirty || session?.rev !== snap.rev || editing !== null || !validPreview || !review?.can_accept))) return;
    if (!accept && (dirty || editing !== null) && !window.confirm("Reject this proposal and discard its unsaved draft and operation form?")) return;
    setBusy(true); setError("");
    try {
      if (accept) await api.accept(p.id, snap.rev); else await api.reject(p.id, snap.rev);
      storePlanDraft({ scope, plan: p, original: p, rev: snap.rev });
      setEditing(null); setSession(null);
      onSelect(nextProposalId(plans.filter((x) => x.id !== p.id).map((x) => x.id), p.id, plans.map((x) => x.id)));
    } catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };
  if (!p) return <div className="inspector-empty"><h2>No ghost proposals</h2><p>All plans reviewed. Undo restores the last change. Any unfinished drafts remain stored locally.</p></div>;
  return <section className="proposal-review" data-ui aria-label="Ghost proposal review" aria-busy={busy}>
    <header><h2>Ghost proposals</h2><label>Plan<select aria-label="Plan" disabled={busy} value={p.id} onChange={(e) => select(e.target.value)}>{plans.map((x) => <option key={x.id} value={x.id}>{x.id} · {x.rationale ?? x.source}</option>)}</select></label>
      <div className="d-actions"><button disabled={busy} aria-label="Previous proposal" onClick={() => select(plans[(at + plans.length - 1) % plans.length].id)}>‹</button><span>{at + 1} of {plans.length}</span><button disabled={busy} aria-label="Next proposal" onClick={() => select(plans[(at + 1) % plans.length].id)}>›</button></div>
    </header>
    <fieldset disabled={busy} className="plan-fields">
    <label>Plan rationale<textarea value={p.rationale ?? ""} onChange={(e) => change({ rationale: e.target.value || null })} /></label><p className="faint small">{p.id} · by {p.source} · {dirty ? "Unsaved user amendment" : "Saved plan"}</p>
    {error && <p role="alert">{error}</p>}
    {loading && <p role="status">Validating draft preview…</p>}
    {conflict && <div role="alert"><p>The workspace changed. Your draft was preserved. Saving is blocked until you explicitly reconcile it.</p><button onClick={() => {
      if (!window.confirm("Revalidate your draft against the latest workspace? Saving will replace this proposal's saved operations with your draft, not merge them.")) return;
      const next = { scope, plan: p, original: structuredClone(saved!), rev: snap.rev }; setSession(next); storePlanDraft(next);
    }}>Revalidate draft against latest workspace</button></div>}
    {review?.errors.map((x) => <p role="alert" key={x}>{x}</p>)}
    {review?.prerequisites.length ? <p>Accept prerequisites first: {review.prerequisites.join(", ")}</p> : null}
    {review?.notices.map((x) => <p key={x}>{x}</p>)}
    <label><input type="checkbox" checked={baseline} onChange={(e) => setBaseline(e.target.checked)} /> Show before this plan on canvas</label>
    <p className="faint small">Earlier pending plans are included as context. Updates and deletions stay highlighted in plan view. Canvas editing is disabled during review.</p>
    <ImpactLine impact={review?.impact} />{review?.impact && <ImpactDetails impact={review.impact} />}
    <h3>Planned changes ({p.ops.length})</h3>
    {p.ops.map((op, i) => <article className="plan-operation" key={i}>
      <h4>{i + 1}. {operationLabel(op)}</h4>
      <div className="d-actions"><button onClick={() => editOperation(i)}>Edit change</button><button onClick={() => { change({ ops: p.ops.filter((_, index) => index !== i) }); setEditing(null); }}>Remove planned change</button></div>
      {operationTarget(op) ? <button className="link" onClick={() => onFocus(operationTarget(op)!)}>Focus {operationTarget(op)} on canvas</button> : <p className="faint small">This change may not have an element on the class diagram.</p>}
      {review?.steps[i]?.error && <p role="alert">{review.steps[i].error}</p>}
      {review?.steps[i]?.changes.map((c) => <section key={c.path}><b>{c.path}</b>{fieldDiffs(c.before, c.after).map((f) => <div className="plan-field" key={f.field}><span>{f.field}</span><div><del>{formatValue(f.before)}</del><ins>{formatValue(f.after)}</ins></div></div>)}</section>)}
      <details><summary>Full operation and values</summary><pre>{JSON.stringify(op, null, 2)}</pre>{review?.steps[i]?.changes.map((c) => <div key={c.path}><h5>{c.path} · Before → After</h5><pre>{JSON.stringify(c.before, null, 2)}</pre><pre>{JSON.stringify(c.after, null, 2)}</pre></div>)}</details>
    </article>)}
    <button ref={addButton} onClick={() => editOperation(-1)}>Add planned change</button>
    {editing !== null && <ProposalOperationEditor key={`${p.id}:${editing}`} initial={editing >= 0 ? p.ops[editing] : undefined} onCancel={closeEditor} onSubmit={(op) => { change({ ops: editing < 0 ? [...p.ops, op] : p.ops.map((x, i) => i === editing ? op : x) }); closeEditor(); }} />}
    <ProposalDecisionContext proposal={p} design={snap.view} onChange={change} />
    <div className="d-actions"><button disabled={!dirty || conflict || !validPreview || editing !== null} onClick={() => void save()}>Save plan</button><button disabled={!dirty && editing === null} onClick={discard}>Discard edits</button></div>
    <div className="d-actions plan-resolution"><button className="primary" disabled={busy || dirty || session?.rev !== snap.rev || editing !== null || !validPreview || !review?.can_accept} onClick={() => void resolve(true)}>Accept proposal</button><button disabled={busy} onClick={() => void resolve(false)}>Reject proposal</button></div>
    {dirty && <p className="faint small">Save or discard your changes before accepting.</p>}
    </fieldset>
  </section>;
}
function formatValue(value: unknown) { return value == null ? "—" : typeof value === "string" ? value : JSON.stringify(value, null, 2); }

/** Context only: changing a ghost plan does not change the examiner's record. */
export function ProposalDecisionContext({ proposal, design, onChange }: { proposal: Proposal; design: Design; onChange: (patch: Partial<Proposal>) => void }) {
  const d = design.decisions.find((x) => x.id === proposal.decision);
  return <section className="proposal-decision">
    <h3>Associated decision</h3>
    <label>Decision<select value={proposal.decision ?? ""} onChange={(e) => onChange({ decision: e.target.value || null, option: null })}>
      <option value="">No associated decision</option>
      {design.decisions.map((x) => <option key={x.id} value={x.id}>{x.id} · {x.question}</option>)}
    </select></label>
    {d && <>
      <p><b>{d.question}</b> · {d.state}</p>{d.context && <p>{d.context}</p>}
      <label>Associated alternative<select value={proposal.option ?? ""} onChange={(e) => onChange({ option: e.target.value || null })}>
        <option value="">No specific alternative</option>{d.options.map((o) => <option key={o.id} value={o.id}>{o.label}</option>)}
      </select></label>
      {d.options.find((o) => o.id === proposal.option)?.summary && <p>{d.options.find((o) => o.id === proposal.option)?.summary}</p>}
      {d.attempts.map((a, i) => <details key={i}><summary>Defence {i + 1} · {a.verdict ?? "pending"}</summary><p>{a.defense}</p><p>{a.feedback}</p></details>)}
      <p className="faint small">Context only. Plan edits are user amendments, not a new examiner approval.</p>
    </>}
  </section>;
}
