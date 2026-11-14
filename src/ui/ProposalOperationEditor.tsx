import { useEffect, useRef, useState } from "react";
import type { Op } from "../state/types";
import { patchOperation } from "../state/proposal-review";

const attribute = { name: "", type: "String", visibility: "private", is_static: false, multiplicity: null, status: "accepted", proposal: null, origin: null };
const method = { name: "", params: [], returns: null, visibility: "public", is_static: false, is_abstract: false, uses: [], calls: [], description: null, status: "accepted", proposal: null, origin: null };
export const operationTemplates: Record<string, Op> = {
  add_class: { op: "add_class", name: "", package: null },
  update_class: { op: "update_class", name: "" }, rename_class: { op: "rename_class", from: "", to: "" }, remove_class: { op: "remove_class", name: "" },
  add_attribute: { op: "add_attribute", class: "", attribute }, update_attribute: { op: "update_attribute", class: "", name: "", attribute }, remove_attribute: { op: "remove_attribute", class: "", name: "" },
  add_method: { op: "add_method", class: "", method }, update_method: { op: "update_method", class: "", name: "", method }, remove_method: { op: "remove_method", class: "", name: "" },
  add_relation: { op: "add_relation", from: "", to: "", kind: "association" }, update_relation: { op: "update_relation", from: "", to: "", kind: "association" }, remove_relation: { op: "remove_relation", from: "", to: "", kind: null },
  add_package: { op: "add_package", name: "", layer: null }, update_package: { op: "update_package", name: "" }, remove_package: { op: "remove_package", name: "" },
  move_to_package: { op: "move_to_package", class: "", package: null }, set_layer: { op: "set_layer", package: "", layer: null },
};
const fields: Record<string, string[]> = {
  add_class: ["name", "package", "stereotype", "is_abstract", "aggregate_root", "external", "role", "rationale", "responsibilities", "attributes", "methods"],
  update_class: ["name", "package", "stereotype", "is_abstract", "aggregate_root", "external", "role", "rationale", "responsibilities"],
  rename_class: ["from", "to"], remove_class: ["name"],
  add_attribute: ["class"], update_attribute: ["class", "name"], remove_attribute: ["class", "name"],
  add_method: ["class"], update_method: ["class", "name"], remove_method: ["class", "name"],
  add_relation: ["from", "to", "kind", "label", "from_mult", "to_mult", "directed", "interaction", "rationale"],
  update_relation: ["from", "to", "kind", "label", "from_mult", "to_mult", "directed", "interaction"], remove_relation: ["from", "to", "kind"],
  add_package: ["name", "layer", "description", "external", "system"], update_package: ["name", "description", "external", "system"], remove_package: ["name"], move_to_package: ["class", "package"], set_layer: ["package", "layer"],
};
const arrays = new Set(["params", "uses", "calls", "responsibilities", "attributes", "methods"]);
const booleans = new Set(["is_static", "is_abstract", "aggregate_root", "external", "directed"]);
const enums: Record<string, string[]> = { visibility: ["public", "private", "protected", "package"], kind: ["association", "aggregation", "composition", "inheritance", "realization", "dependency"], layer: ["ui", "infrastructure", "application", "domain"], interaction: ["sync", "async", "event"] };

export function ProposalOperationEditor({ initial, onSubmit, onCancel }: { initial?: Op; onSubmit: (op: Op) => void; onCancel: () => void }) {
  const root = useRef<HTMLElement>(null);
  useEffect(() => { root.current?.querySelector<HTMLElement>("select, input, textarea")?.focus(); }, []);
  const [op, setOp] = useState<Op>(() => structuredClone(initial ?? operationTemplates.add_class));
  const [advanced, setAdvanced] = useState<boolean>(!!initial && !Object.hasOwn(fields, initial.op));
  const [raw, setRaw] = useState(() => JSON.stringify(initial ?? operationTemplates.add_class, null, 2));
  const [jsonFields, setJsonFields] = useState<Record<string, string>>({});
  const [error, setError] = useState("");
  const update = (field: string, value: unknown, nested?: string) => setOp((old) => patchOperation(old, field, value, nested));
  const field = (name: string, nested?: string) => {
    const data = nested ? op[nested] as Record<string, unknown> : op;
    const value = data?.[name]; const key = `${nested ?? ""}.${name}`;
    return <label key={key}>{nested ? `${nested} · ` : ""}{name.replaceAll("_", " ")}
      {arrays.has(name) ? <textarea rows={3} value={jsonFields[key] ?? JSON.stringify(value ?? [], null, 2)} onChange={(e) => { setJsonFields((old) => ({ ...old, [key]: e.target.value })); }} />
        : booleans.has(name) ? <select value={value == null ? "" : String(value)} onChange={(e) => update(name, e.target.value === "" ? null : e.target.value === "true", nested)}><option value="">Unspecified</option><option value="true">Yes</option><option value="false">No</option></select>
        : enums[name] ? <select value={String(value ?? "")} onChange={(e) => update(name, e.target.value || null, nested)}><option value="">Unspecified</option>{enums[name].map((x) => <option key={x}>{x}</option>)}</select>
        : <input value={String(value ?? "")} onChange={(e) => update(name, e.target.value || null, nested)} />}
    </label>;
  };
  const materialize = (): Op => {
    let result = op;
    for (const [path, text] of Object.entries(jsonFields)) {
      const [nested, name] = path.split("."); const value = JSON.parse(text);
      if (!Array.isArray(value)) throw new Error(`${name} must be a JSON array`);
      result = patchOperation(result, name, value, nested || undefined);
    }
    return result;
  };
  const submit = () => {
    try { const value = advanced ? JSON.parse(raw) : materialize(); if (!value || Array.isArray(value) || typeof value.op !== "string") throw new Error("An operation object with an op name is required"); onSubmit(value); }
    catch (e) { setError(String(e)); }
  };
  return <section ref={root} className="plan-operation" aria-label="Edit planned operation">
    <h3>{initial ? "Edit planned change" : "Add planned change"}</h3>
    {!initial && <label>Change type<select value={fields[op.op] ? op.op : "advanced"} onChange={(e) => { const next = structuredClone(operationTemplates[e.target.value] ?? { op: "" }); setOp(next); setRaw(JSON.stringify(next, null, 2)); setAdvanced(e.target.value === "advanced"); setJsonFields({}); setError(""); }}>
      {Object.keys(operationTemplates).map((x) => <option key={x} value={x}>{x.replaceAll("_", " ")}</option>)}<option value="advanced">Other operation (advanced JSON)</option>
    </select></label>}
    <button className="link" onClick={() => { try { if (!advanced) { const next = materialize(); setOp(next); setRaw(JSON.stringify(next, null, 2)); } else { const next = JSON.parse(raw); if (!fields[next.op]) throw new Error("Use advanced JSON for this operation family"); setOp(next); setJsonFields({}); } setAdvanced(!advanced); setError(""); } catch (e) { setError(String(e)); } }}>{advanced ? "Structured fields" : "Advanced JSON"}</button>
    {advanced ? <label>Operation JSON<textarea rows={14} value={raw} onChange={(e) => setRaw(e.target.value)} spellCheck={false} /></label> : <>
      {fields[op.op]?.map((name) => field(name))}
      {op.attribute != null && Object.keys(attribute).filter((x) => !["status", "proposal", "origin"].includes(x)).map((name) => field(name, "attribute"))}
      {op.method != null && Object.keys(method).filter((x) => !["status", "proposal", "origin"].includes(x)).map((name) => field(name, "method"))}
    </>}
    <p className="faint small">Deletes plan an element deletion. “Remove planned change” only removes an operation. Backend validation runs before saving.</p>
    {error && <p role="alert">{error}</p>}
    <div className="d-actions"><button onClick={submit}>Use change in draft</button><button onClick={onCancel}>Cancel edit</button></div>
  </section>;
}
