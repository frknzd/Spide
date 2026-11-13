// Dev perf counters: render counts per component and frame rate. Enabled with `?perf=1`
// (or localStorage.spidePerf = "1"); otherwise every call is a cheap no-op.

const enabled =
  typeof window !== "undefined" &&
  (new URLSearchParams(window.location.search).get("perf") === "1" || window.localStorage?.getItem("spidePerf") === "1");

const counts = new Map<string, number>();

export const perfEnabled = enabled;

/** Count one render/commit of `name`. Call in a component body. */
export function perfCount(name: string) {
  if (!enabled) return;
  counts.set(name, (counts.get(name) ?? 0) + 1);
}

/** Snapshot and reset the counters (per-second rates in the overlay). */
export function perfTake(): Record<string, number> {
  const out = Object.fromEntries(counts);
  counts.clear();
  return out;
}
