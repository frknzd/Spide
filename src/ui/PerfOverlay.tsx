import { useEffect, useState } from "react";
import { perfEnabled, perfTake } from "../state/perf";

/** `?perf=1`: frames per second, renders per second per component, last snapshot build time. */
export function PerfOverlay({ buildMs }: { buildMs?: number }) {
  const [stats, setStats] = useState<{ fps: number; renders: Record<string, number> }>({ fps: 0, renders: {} });
  useEffect(() => {
    if (!perfEnabled) return;
    let frames = 0;
    let raf = 0;
    const loop = () => {
      frames++;
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    const iv = setInterval(() => {
      setStats({ fps: frames, renders: perfTake() });
      frames = 0;
    }, 1000);
    return () => {
      cancelAnimationFrame(raf);
      clearInterval(iv);
    };
  }, []);
  if (!perfEnabled) return null;
  return (
    <div className="perf-overlay" data-ui>
      <div>{stats.fps} fps</div>
      {Object.entries(stats.renders).sort().map(([k, v]) => (
        <div key={k}>{k} {v}/s</div>
      ))}
      {buildMs !== undefined && <div>snapshot {buildMs.toFixed(1)} ms</div>}
    </div>
  );
}
