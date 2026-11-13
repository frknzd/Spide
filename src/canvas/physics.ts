// Layout physics: d3-force with custom forces. It only runs after a model change, a drag
// or a tidy, and stops completely once it has settled (no perpetual motion: idle = no work).

import { forceSimulation, type Simulation, type SimulationLinkDatum, type SimulationNodeDatum } from "d3-force";
import type { Pos } from "../state/types";

export interface SimNode extends SimulationNodeDatum {
  id: string;
  w: number;
  h: number;
  pkg: string | null;
  layer: number | null;
  heat: number;
  /** tidy target */
  tx?: number;
  ty?: number;
  /** pinned by the user: fx/fy stay set */
  pinned?: boolean;
}
export interface SimLink extends SimulationLinkDatum<SimNode> {
  kind: string;
}

export interface NodeSpec {
  id: string;
  w: number;
  h: number;
  pkg: string | null;
  layer: number | null;
  heat: number;
  init?: Pos;
}
export interface LinkSpec { from: string; to: string; kind: string }

const LAYER_GAP = 300;

function rectCollide(margin: number) {
  let nodes: SimNode[] = [];
  const force = (alpha: number) => {
    const k = 0.6 * Math.max(alpha, 0.05);
    for (let i = 0; i < nodes.length; i++) {
      const a = nodes[i];
      for (let j = i + 1; j < nodes.length; j++) {
        const b = nodes[j];
        const dx = (b.x! + b.vx!) - (a.x! + a.vx!);
        const dy = (b.y! + b.vy!) - (a.y! + a.vy!);
        const ox = (a.w + b.w) / 2 + margin - Math.abs(dx);
        const oy = (a.h + b.h) / 2 + margin - Math.abs(dy);
        if (ox > 0 && oy > 0) {
          if (ox / (a.w + b.w) < oy / (a.h + b.h)) {
            const s = (dx < 0 ? -1 : 1) * ox * k * 0.5;
            a.vx! -= s;
            b.vx! += s;
          } else {
            const s = (dy < 0 ? -1 : 1) * oy * k * 0.5;
            a.vy! -= s;
            b.vy! += s;
          }
        }
      }
    }
  };
  force.initialize = (n: SimNode[]) => (nodes = n);
  return force;
}

/** Keep package hulls from overlapping: push whole packages apart. */
function packageSeparation(margin: number) {
  let nodes: SimNode[] = [];
  const force = (alpha: number) => {
    const boxes = new Map<string, { x1: number; y1: number; x2: number; y2: number; ns: SimNode[]; layer: number | null }>();
    for (const n of nodes) {
      if (!n.pkg) continue;
      const b = boxes.get(n.pkg) ?? { x1: Infinity, y1: Infinity, x2: -Infinity, y2: -Infinity, ns: [], layer: n.layer };
      b.x1 = Math.min(b.x1, n.x! - n.w / 2);
      b.y1 = Math.min(b.y1, n.y! - n.h / 2);
      b.x2 = Math.max(b.x2, n.x! + n.w / 2);
      b.y2 = Math.max(b.y2, n.y! + n.h / 2);
      b.ns.push(n);
      boxes.set(n.pkg, b);
    }
    const list = [...boxes.values()];
    const k = 0.25 * Math.max(alpha, 0.03);
    for (let i = 0; i < list.length; i++) {
      for (let j = i + 1; j < list.length; j++) {
        const a = list[i];
        const b = list[j];
        const ox = Math.min(a.x2, b.x2) - Math.max(a.x1, b.x1) + margin;
        const oy = Math.min(a.y2, b.y2) - Math.max(a.y1, b.y1) + margin;
        if (ox <= 0 || oy <= 0) continue;
        const acx = (a.x1 + a.x2) / 2, bcx = (b.x1 + b.x2) / 2, acy = (a.y1 + a.y2) / 2, bcy = (b.y1 + b.y2) / 2;
        // different layers separate vertically in layer order (outer layers on top)
        const layered = a.layer !== null && b.layer !== null && a.layer !== b.layer;
        const horizontal = !layered && ox < oy;
        const s = (horizontal ? ox : oy) * k;
        const dir = layered ? (b.layer! > a.layer! ? 1 : -1) : horizontal ? (bcx >= acx ? 1 : -1) : (bcy >= acy ? 1 : -1);
        for (const n of a.ns) horizontal ? (n.vx! -= (dir * s) / a.ns.length * 2) : (n.vy! -= (dir * s) / a.ns.length * 2);
        for (const n of b.ns) horizontal ? (n.vx! += (dir * s) / b.ns.length * 2) : (n.vy! += (dir * s) / b.ns.length * 2);
      }
    }
  };
  force.initialize = (n: SimNode[]) => (nodes = n);
  return force;
}

function tidyPull() {
  let nodes: SimNode[] = [];
  const force = () => {
    for (const n of nodes) {
      if (n.tx === undefined || n.ty === undefined) continue;
      n.vx! += (n.tx - n.x!) * 0.18;
      n.vy! += (n.ty - n.y!) * 0.18;
    }
  };
  force.initialize = (n: SimNode[]) => (nodes = n);
  return force;
}

export class LivingLayout {
  sim: Simulation<SimNode, SimLink>;
  nodes = new Map<string, SimNode>();
  links: SimLink[] = [];
  tension = 0;
  private tidyUntil = 0;
  private onTick: () => void;
  private onSettle: () => void;
  private raf = 0;
  private settleTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(onTick: () => void, onSettle: () => void) {
    this.onTick = onTick;
    this.onSettle = onSettle;
    this.sim = forceSimulation<SimNode, SimLink>([])
      .alphaDecay(0.035)
      .alphaMin(0.005)
      .velocityDecay(0.42)
      // Positions come from ELK (initial layout, new batches, tidy). Physics only resolves
      // overlaps (classes and package hulls), e.g. after a drag or a single new class.
      .force("collide", rectCollide(36))
      .force("pkgsep", packageSeparation(80))
      .force("tidy", tidyPull())
      .on("tick", () => this.tick())
      .on("end", () => this.onSettle());
  }

  private tick() {
    if (this.tidyUntil && performance.now() > this.tidyUntil) {
      this.tidyUntil = 0;
      for (const n of this.nodes.values()) {
        // land exactly on the target (ELK guarantees no overlaps there)
        if (n.tx !== undefined && n.ty !== undefined && !n.pinned) {
          n.x = n.tx;
          n.y = n.ty;
          n.vx = 0;
          n.vy = 0;
        }
        n.tx = undefined;
        n.ty = undefined;
      }
    }
    if (!this.raf) {
      this.raf = requestAnimationFrame(() => {
        this.raf = 0;
        this.onTick();
      });
    }
    if (this.settleTimer) clearTimeout(this.settleTimer);
    this.settleTimer = setTimeout(() => this.onSettle(), 1200);
  }

  update(specs: NodeSpec[], links: LinkSpec[], tension: number) {
    const before = new Set(this.nodes.keys());
    const next = new Map<string, SimNode>();
    let added = 0;
    let changed = false;
    // neighbour lookup for placing new nodes
    const nb = new Map<string, string[]>();
    for (const l of links) {
      nb.set(l.from, [...(nb.get(l.from) ?? []), l.to]);
      nb.set(l.to, [...(nb.get(l.to) ?? []), l.from]);
    }
    for (const s of specs) {
      let n = this.nodes.get(s.id);
      if (!n) {
        added++;
        let x: number, y: number;
        if (s.init) {
          x = s.init.x;
          y = s.init.y;
        } else {
          const ns = (nb.get(s.id) ?? []).map((id) => this.nodes.get(id) ?? next.get(id)).filter(Boolean) as SimNode[];
          if (ns.length) {
            x = ns.reduce((a, b) => a + b.x!, 0) / ns.length + (Math.random() - 0.5) * 260;
            y = ns.reduce((a, b) => a + b.y!, 0) / ns.length + (Math.random() - 0.5) * 200 + 120;
            if (s.layer !== null) {
              const nl = ns.filter((n) => n.layer !== null);
              if (nl.length) y = nl[0].y! + (s.layer - nl[0].layer!) * LAYER_GAP * 0.8;
            }
          } else {
            const i = next.size + added;
            const ang = i * 2.4;
            const r = 140 + 60 * Math.sqrt(i);
            x = Math.cos(ang) * r;
            y = Math.sin(ang) * r * 0.6;
          }
        }
        n = { id: s.id, x, y, vx: 0, vy: 0, w: s.w, h: s.h, pkg: s.pkg, layer: s.layer, heat: s.heat };
        if (s.init?.pinned) {
          n.pinned = true;
          n.fx = x;
          n.fy = y;
        }
      } else {
        if (n.w !== s.w || n.h !== s.h || n.pkg !== s.pkg || n.layer !== s.layer) changed = true;
        Object.assign(n, { w: s.w, h: s.h, pkg: s.pkg, layer: s.layer, heat: s.heat });
      }
      next.set(s.id, n);
    }
    const removed = [...before].filter((id) => !next.has(id)).length;
    this.nodes = next;
    const linkKey = (l: LinkSpec) => `${l.from}|${l.to}|${l.kind}`;
    const oldLinks = new Set(this.links.map((l) => linkKey({ from: (l.source as SimNode).id, to: (l.target as SimNode).id, kind: l.kind })));
    const newLinks = links.filter((l) => next.has(l.from) && next.has(l.to) && l.from !== l.to);
    const linksChanged = newLinks.length !== oldLinks.size || newLinks.some((l) => !oldLinks.has(linkKey(l)));
    this.links = newLinks.map((l) => ({ source: l.from, target: l.to, kind: l.kind }));
    this.sim.nodes([...next.values()]);
    this.tension = tension;
    const kick = added || removed ? 0.6 : linksChanged ? 0.35 : changed ? 0.15 : 0;
    if (kick) this.sim.alpha(Math.max(this.sim.alpha(), kick)).restart();
    this.onTick();
  }

  drag(id: string, x: number, y: number) {
    const n = this.nodes.get(id);
    if (!n) return;
    n.fx = x;
    n.fy = y;
    this.sim.alphaTarget(Math.max(0.15, this.sim.alphaTarget())).restart();
  }

  release(id: string) {
    const n = this.nodes.get(id);
    if (!n) return;
    // a pinned node stays where it was dropped
    if (!n.pinned) {
      n.fx = null;
      n.fy = null;
    }
    this.sim.alphaTarget(0);
  }

  /** Pin / unpin a node where it is. Returns the new state. */
  togglePin(id: string): boolean {
    const n = this.nodes.get(id);
    if (!n) return false;
    n.pinned = !n.pinned;
    n.fx = n.pinned ? n.x : null;
    n.fy = n.pinned ? n.y : null;
    this.sim.alpha(Math.max(this.sim.alpha(), 0.1)).restart();
    return n.pinned;
  }

  pinnedIds(): Set<string> {
    return new Set([...this.nodes.values()].filter((n) => n.pinned).map((n) => n.id));
  }

  /** Animate towards target positions (e.g. ELK), then let physics take over. Pinned nodes stay. */
  tidy(pos: Map<string, Pos>) {
    for (const [id, p] of pos) {
      const n = this.nodes.get(id);
      if (n && !n.pinned) {
        n.tx = p.x;
        n.ty = p.y;
      }
    }
    this.tidyUntil = performance.now() + 1400;
    this.sim.alpha(0.8).restart();
  }

  positions(): Record<string, Pos> {
    const out: Record<string, Pos> = {};
    for (const n of this.nodes.values()) out[n.id] = n.pinned ? { x: Math.round(n.x!), y: Math.round(n.y!), pinned: true } : { x: Math.round(n.x!), y: Math.round(n.y!) };
    return out;
  }

  stop() {
    this.sim.stop();
    if (this.settleTimer) clearTimeout(this.settleTimer);
    cancelAnimationFrame(this.raf);
  }
}
