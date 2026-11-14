import { useEffect, useSyncExternalStore } from "react";
import { hoverStore } from "../state/hover";
import type { Design, DriftReport, Metrics, Snapshot } from "../state/types";
import { HoverCard } from "./HoverCard";

export function HoverLayer(p: { snap: Snapshot; design: Design; metrics: Metrics; drift?: DriftReport | null; scope: string; disabled?: boolean }) {
  const hover = useSyncExternalStore(hoverStore.subscribe, hoverStore.snapshot);
  useEffect(() => { hoverStore.clear(); return hoverStore.clear; }, [p.scope, p.disabled]);
  useEffect(() => {
    if (!hover) return;
    // Don't keep a card for an element removed by an incoming snapshot.
    const exists = [...document.querySelectorAll("[data-hover]")].some((e) => e.getAttribute("data-hover") === hover.key);
    if (!exists) hoverStore.clear();
  }, [p.design, hover]);
  if (!hover || p.disabled) return null;
  return <HoverCard hoverKey={hover.key} at={hover.at} snap={p.snap} design={p.design} metrics={p.metrics} drift={p.drift} pinned={!!hover.pinned} />;
}
