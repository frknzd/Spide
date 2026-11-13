import { memo } from "react";
import type { Class, ClassMetrics, Status, Target } from "../state/types";
import { badges, C, F, FONT_MONO, FONT_SANS, heatBucket, heatColor, PAD_X, statusOpacity, textWidth, type ClassBox } from "./visuals";

export interface ClassNodeProps {
  cls: Class;
  box: ClassBox;
  x: number;
  y: number;
  scale: number;
  m?: ClassMetrics;
  dim?: boolean;
  focus?: boolean;
  /** outline colour (stress test, drift) */
  ring?: string | null;
  ringDashed?: boolean;
  /** exists only in the code (drift view) */
  phantom?: boolean;
  pinned?: boolean;
  showMethods?: boolean;
  /** static rendering (previews/export): no animation classes */
  still?: boolean;
  layerColor?: string | null;
  /** zoomed far out: draw the frame and a large name only (same box size, so nothing moves) */
  compact?: boolean;
}

const T = (t: Target) => JSON.stringify(t);

function rowColor(s: Status): string {
  switch (s) {
    case "proposed": return C.ghost;
    case "removing": return C.removing;
    case "pruned": return C.faint;
    case "overruled": return C.warn;
    default: return C.text;
  }
}

export const ClassNode = memo(function ClassNode(p: ClassNodeProps) {
  const { cls, box, m } = p;
  const { w, h } = box;
  const st = cls.status;
  const ghost = st === "proposed";
  const removing = st === "removing";
  const pruned = st === "pruned";
  const heat = ghost ? 0 : m?.heat ?? 0;
  const hc = heatColor(heat);
  const border = ghost ? C.ghost : removing ? C.removing : pruned ? C.faint : heat > 0.18 ? hc : C.border;
  const target: Target = ghost && cls.proposal ? { kind: "proposal", id: cls.proposal } : { kind: "class", name: cls.name };
  const italic = cls.is_abstract || cls.stereotype === "interface";
  const bs = ghost || pruned ? [] : badges(m);
  const opacity = statusOpacity(st) * (p.dim ? 0.28 : 1) * (p.phantom ? 0.5 : 1);
  const hb = heatBucket(heat);

  let bx = 0;
  return (
    <g transform={`translate(${p.x.toFixed(1)},${p.y.toFixed(1)}) scale(${p.scale.toFixed(3)}) translate(${-w / 2},${-h / 2})`} opacity={opacity}
      style={{ transition: p.still ? undefined : "opacity 220ms" }}>
      <g className={p.still ? undefined : ghost ? "node-enter node-ghost" : "node-enter"}>
        {/* heat halo: plain strokes (no blur filter, which is expensive to repaint) */}
        {hb > 0 && !pruned && (
          <g pointerEvents="none" className={p.still ? undefined : heat > 0.55 ? "halo-pulse" : undefined}>
            <rect x={-7} y={-7} width={w + 14} height={h + 14} rx={14} fill="none" stroke={hc} strokeWidth={8 + heat * 6} strokeOpacity={0.07 + heat * 0.12} />
            <rect x={-3} y={-3} width={w + 6} height={h + 6} rx={10} fill="none" stroke={hc} strokeWidth={3} strokeOpacity={0.15 + heat * 0.3} />
          </g>
        )}
        {p.focus && <rect x={-5} y={-5} width={w + 10} height={h + 10} rx={12} fill="none" stroke={C.accent} strokeWidth={1.5} opacity={0.7} pointerEvents="none" />}
        {p.ring && <rect x={-7} y={-7} width={w + 14} height={h + 14} rx={13} fill="none" stroke={p.ring} strokeWidth={2.2} opacity={0.9} pointerEvents="none"
          className="halo-pulse" strokeDasharray={p.ringDashed ? "7 5" : undefined} />}
        {p.pinned && <text x={w - 8} y={14} textAnchor="end" fontSize={11} fill={C.dim} pointerEvents="none">📌</text>}
        {p.phantom && <Chip x={w - 4} y={-23} text="only in code" color={C.dim} anchor="end" />}
        <rect width={w} height={h} rx={8} fill={C.node} stroke={border} strokeWidth={ghost ? 1.5 : 1.2 + heat * 1.6}
          strokeDasharray={ghost || removing ? "6 4" : undefined}
          data-target={T(target)} data-node={cls.name} data-hover={`class:${cls.name}`} style={{ cursor: ghost ? "pointer" : "grab" }} />
        {p.compact ? (
          <>
            <rect width={w} height={h} rx={8} fill={p.layerColor ?? C.nodeHeader} opacity={p.layerColor ? 0.14 : 0.6} pointerEvents="none" />
            <text x={w / 2} y={h / 2 + 8} textAnchor="middle" fontFamily={FONT_SANS} fontSize={Math.min(30, Math.max(16, (w - 16) / Math.max(4, cls.name.length) * 1.7))}
              fontWeight={600} fontStyle={italic ? "italic" : undefined} fill={ghost ? C.ghost : C.text} pointerEvents="none">{cls.name}</text>
          </>
        ) : (<>
        {/* header */}
        <path d={`M0,8 a8,8 0 0 1 8,-8 h${w - 16} a8,8 0 0 1 8,8 v${box.headerH - 8} h${-w} z`} fill={p.layerColor ? p.layerColor : C.nodeHeader}
          opacity={p.layerColor ? 0.16 : 1} pointerEvents="none" />
        {box.stereo && (
          <text x={w / 2} y={15} textAnchor="middle" fontFamily={FONT_SANS} fontSize={11} fill={C.dim} pointerEvents="none">{box.stereo}</text>
        )}
        <text x={w / 2} y={box.stereo ? 31 : 20} textAnchor="middle" fontFamily={FONT_SANS} fontSize={14} fontWeight={600}
          fontStyle={italic ? "italic" : undefined} fill={ghost ? C.ghost : C.text} pointerEvents="none">
          {cls.name}
        </text>
        <line x1={0} x2={w} y1={box.attrY} y2={box.attrY} stroke={border} strokeWidth={1} opacity={0.7} pointerEvents="none" />
        {p.showMethods !== false && <line x1={0} x2={w} y1={box.methY} y2={box.methY} stroke={border} strokeWidth={1} opacity={0.7} pointerEvents="none" />}
        {box.rows.map((r) => {
          if (r.kind === "note") {
            return (
              <text key={"note" + r.y} x={PAD_X} y={r.y} fontFamily={FONT_SANS} fontSize={12} fontStyle="italic" fill={C.dim} pointerEvents="none">{r.text}</text>
            );
          }
          const member = r.kind === "attribute" ? cls.attributes.find((a) => a.name === r.name) : cls.methods.find((x) => x.name === r.name);
          const kind = r.kind;
          const tgt: Target = r.status === "proposed" && member?.proposal ? { kind: "proposal", id: member.proposal } : { kind, class: cls.name, name: r.name };
          const tw = textWidth(r.text, F.member);
          return (
            <g key={r.kind + r.name + r.y} data-target={T(tgt)} data-hover={`${r.kind}:${cls.name}.${r.name}`} style={{ cursor: r.status === "pruned" ? "pointer" : "context-menu" }}>
              <rect x={2} y={r.y - 12} width={w - 4} height={16} fill="transparent">
                {r.status === "pruned" && <title>pruned · click to restore</title>}
              </rect>
              <text x={PAD_X} y={r.y} fontFamily={FONT_MONO} fontSize={12} fill={rowColor(r.status)}
                fontStyle={r.isAbstract ? "italic" : undefined} textDecoration={r.isStatic ? "underline" : undefined}>
                {r.text}
              </text>
              {(r.status === "removing" || r.status === "pruned") && (
                <line x1={PAD_X} x2={PAD_X + tw} y1={r.y - 4} y2={r.y - 4} stroke={rowColor(r.status)} strokeWidth={1.2} />
              )}
            </g>
          );
        })}
        </>)}
        {/* status chips */}
        {ghost && <Chip x={w - 4} y={-23} text="+ proposed" color={C.ghost} anchor="end" />}
        {removing && <Chip x={w - 4} y={-23} text="− proposed removal" color={C.removing} anchor="end" />}
        {st === "overruled" && <Chip x={w - 4} y={-23} text="⚠ overruled" color={C.warn} anchor="end" />}
        {cls.aggregate_root && st !== "pruned" && <Chip x={4} y={-23} text="◆ aggregate" color={C.dim} />}
        {pruned && (
          <>
            <line x1={-6} y1={h / 2} x2={w + 6} y2={h / 2} stroke={C.cycle} strokeWidth={2} />
            <Chip x={w - 4} y={-23} text="pruned · click to restore" color={C.faint} anchor="end" />
          </>
        )}
        {removing && <line x1={-6} y1={-6} x2={w + 6} y2={h + 6} stroke={C.removing} strokeWidth={1.5} opacity={0.7} pointerEvents="none" />}
        {bs.map((b) => {
          const cw = textWidth(b.text, F.small) + 12;
          const el = <Chip key={b.text} x={bx} y={h + 6} text={b.text} color={b.color} />;
          bx += cw + 4;
          return el;
        })}
      </g>
    </g>
  );
});

export function Chip({ x, y, text, color, anchor }: { x: number; y: number; text: string; color: string; anchor?: "end" }) {
  const cw = textWidth(text, F.small) + 12;
  const x0 = anchor === "end" ? x - cw : x;
  return (
    <g transform={`translate(${x0},${y})`} pointerEvents="none">
      <rect width={cw} height={17} rx={8.5} fill={C.bg} stroke={color} strokeWidth={1} opacity={0.95} />
      <text x={cw / 2} y={12} textAnchor="middle" fontFamily={FONT_SANS} fontSize={10.5} fill={color}>{text}</text>
    </g>
  );
}

/** SVG defs (blur filters) used by the few large highlights in the package and system views. */
export function NodeDefs() {
  return (
    <>
      {[1, 2, 3, 4, 5].map((b) => (
        <filter key={b} id={`halo${b}`} x="-50%" y="-50%" width="200%" height="200%">
          <feGaussianBlur stdDeviation={4 + b * 3.2} />
        </filter>
      ))}
    </>
  );
}
