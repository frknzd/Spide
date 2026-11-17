// Spide's Lucid read script (run with lucid_run_script against one document; needs the `uml` API loaded).
// Returns every item on the first page with its geometry, text, line ends, Spide shape data, UML
// details and sequence markup, plus the comment threads. Spide normalises this into a LucidSnapshot.
const page = skills.basecanvasediting.getPages()[0];
if (page && !page.current) skills.basecanvasediting.navigateToPage(page.id);
const read = await skills.basecanvasediting.read({});
const ids = new Set();
const walk = (v) => {
  if (Array.isArray(v)) { v.forEach(walk); return; }
  if (!v || typeof v !== "object") return;
  for (const [k, x] of Object.entries(v)) {
    if (typeof x === "string" && ["id", "itemId", "shapeId", "containerId", "generatorId", "lineId", "sourceId", "targetId"].includes(k)) ids.add(x);
    else if (k === "itemIds" && Array.isArray(x)) x.forEach((i) => typeof i === "string" && ids.add(i));
    else walk(x);
  }
};
for (const p of read.pages || []) walk(p.data);
for (const s of skills.uml.list()) ids.add(s.id);
for (const r of skills.uml.listRelationships()) ids.add(r.id);
const gens = skills.uml.listSequenceDiagrams();
const generated = new Set();
for (const g of gens) { ids.add(g.generatorId); for (const m of g.memberIds || []) generated.add(m); }
const data = (id) => {
  const out = {};
  try { for (const f of skills.formulas.listShapeDataFields(id)) if (f.name.startsWith("spide.")) out[f.name] = f.value; } catch (e) {}
  return out;
};
const list = [...ids].filter((i) => !generated.has(i));
const inspected = list.length ? skills.basecanvasediting.inspect(list) : [];
const items = [];
for (const it of inspected) {
  if (!it || it.kind === "error" || it.kind === "group" && !(it.memberIds || []).length) continue;
  const o = { id: it.id, kind: it.kind, className: it.className, label: it.label || "", bbox: it.boundingBox || null, data: data(it.id) };
  if (it.kind === "line") {
    o.from = typeof it.from === "string" ? it.from : null;
    o.to = typeof it.to === "string" ? it.to : null;
    o.labels = it.labels || [];
    const r = skills.uml.getRelationship(it.id);
    if (r) o.rel = { type: r.type, label: r.label || "", fromMultiplicity: r.fromMultiplicity || "", toMultiplicity: r.toMultiplicity || "", from: typeof r.from === "string" ? r.from : null, to: typeof r.to === "string" ? r.to : null };
  } else if (it.kind === "generator") {
    const m = skills.uml.getSequenceMarkup(it.id);
    if (m !== undefined) o.markup = m;
  } else {
    const u = skills.uml.read(it.id);
    if (u) {
      o.umlType = u.shapeType || null;
      o.textAreas = u.textAreas || {};
      if (u.compartments) o.compartments = u.compartments;
    }
  }
  items.push(o);
}
const threads = [];
for (const t of await skills.comments.listThreads({ status: "all" })) {
  const d = await skills.comments.getThread(t.id);
  threads.push({ id: t.id, itemId: t.itemId || null, resolved: !!t.resolved, bbox: t.boundingBox || null,
    comments: ((d && d.comments) || []).map((c) => ({ id: c.id, author: c.authorName || "", text: c.text || "", at: c.createdAt || "" })) });
}
return { pageId: page ? page.id : null, items, threads };
