import { useSyncExternalStore } from "react";

export interface Point { x: number; y: number }
export interface Hover { key: string; at: Point; pinned?: boolean }

/** Target intent, not pointer stillness. Moving within a target never postpones opening. */
export class HoverStore {
  private value: Hover | null = null;
  private target: string | null = null;
  private at: Point = { x: 0, y: 0 };
  private timer: ReturnType<typeof setTimeout> | undefined;
  private frame: number | undefined;
  private subs = new Set<() => void>();
  subscribe = (f: () => void) => { this.subs.add(f); return () => { this.subs.delete(f); }; };
  snapshot = () => this.value;
  classKey = () => {
    const k = this.value?.key;
    if (k?.startsWith("class:")) return k.slice(6);
    if (k?.startsWith("method:") || k?.startsWith("attribute:")) { const member = k.slice(k.indexOf(":") + 1); return member.slice(0, member.lastIndexOf(".")); }
    return null;
  };
  private publish(v: Hover | null) {
    this.value = v;
    this.subs.forEach((f) => f());
  }
  /** a pinned card stays put and accepts the pointer until it is closed */
  pin = (key: string) => {
    clearTimeout(this.timer);
    if (this.frame !== undefined) { cancelAnimationFrame(this.frame); this.frame = undefined; }
    this.target = key;
    this.publish({ key, at: this.value?.key === key ? this.value.at : this.at, pinned: true });
  };
  move = (key: string | null, at: Point) => {
    if (this.value?.pinned) { this.at = at; return; }
    this.at = at;
    if (key === this.target) {
      if (key && this.value && this.frame === undefined && typeof requestAnimationFrame !== "undefined") {
        this.frame = requestAnimationFrame(() => {
          this.frame = undefined;
          if (this.value && this.target === key) this.publish({ key, at: this.at });
        });
      }
      return;
    }
    if (this.frame !== undefined) { cancelAnimationFrame(this.frame); this.frame = undefined; }
    this.target = key;
    clearTimeout(this.timer);
    if (!key) {
      this.timer = setTimeout(() => this.publish(null), 80);
    } else if (this.value) {
      this.publish({ key, at });
    } else {
      this.timer = setTimeout(() => {
        if (this.target === key) this.publish({ key, at: this.at });
      }, 60);
    }
  };
  clear = () => {
    clearTimeout(this.timer);
    if (this.frame !== undefined) { cancelAnimationFrame(this.frame); this.frame = undefined; }
    this.target = null;
    if (this.value) this.publish(null);
  };
}

export const hoverStore = new HoverStore();
export const useHoveredClass = () => useSyncExternalStore(hoverStore.subscribe, hoverStore.classKey);

/** Fit an actual measured tooltip into the canvas (never the adjacent panes). */
export function placeCard(at: Point, size: { width: number; height: number }, bounds: { left: number; top: number; right: number; bottom: number }) {
  const pad = 8;
  const width = Math.min(size.width, bounds.right - bounds.left - 2 * pad);
  const height = Math.min(size.height, bounds.bottom - bounds.top - 2 * pad);
  const x = at.x + 18 + width > bounds.right - pad ? at.x - width - 18 : at.x + 18;
  return {
    left: Math.max(bounds.left + pad, Math.min(x, bounds.right - width - pad)),
    top: Math.max(bounds.top + pad, Math.min(at.y + 12, bounds.bottom - height - pad)),
    maxWidth: Math.max(80, bounds.right - bounds.left - 2 * pad),
    maxHeight: Math.max(80, bounds.bottom - bounds.top - 2 * pad),
  };
}
