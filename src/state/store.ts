import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import { api, inTauri } from "./api";
import type { Snapshot } from "./types";

/** Tauri event listener; a no-op in a plain browser. */
export function on<T>(event: string, f: (e: { payload: T }) => void): Promise<() => void> {
  if (!inTauri) return Promise.resolve(() => {});
  return listen<T>(event, f);
}

export function useSnapshot() {
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const retry = useCallback(() => { setError(null); setAttempt((n) => n + 1); }, []);
  useEffect(() => {
    let alive = true;
    let eventVersion = 0;
    let requestVersion = 0;
    const fetchSnapshot = async () => {
      const event = eventVersion, request = ++requestVersion;
      try {
        const s = await api.snapshot();
        if (alive && event === eventVersion && request === requestVersion) {
          setSnap((prev) => same(prev, s) ? prev : s); setError(null);
        }
      } catch (e) { if (alive && event === eventVersion && request === requestVersion) setError(String(e)); }
    };
    const same = (a: Snapshot | null, b: Snapshot) => !!a && a.rev === b.rev && a.pi_status === b.pi_status && a.pi_error === b.pi_error && a.branch === b.branch && a.project_dir === b.project_dir;
    const poll = inTauri ? null : setInterval(() => void fetchSnapshot(), 1500);
    const un = on<Snapshot>("snapshot", (e) => {
      if (alive) {
        eventVersion++; setError(null);
        setSnap((prev) => prev && prev.project_dir === e.payload.project_dir && prev.rev > e.payload.rev && prev.pi_status === e.payload.pi_status ? prev : e.payload);
      }
    });
    void un.then(() => { if (alive) void fetchSnapshot(); }, (e) => { if (alive) setError(`Cannot subscribe to workspace updates: ${String(e)}`); });
    return () => { alive = false; if (poll) clearInterval(poll); void un.then((f) => f()); };
  }, [attempt]);
  return { snap, error, retry };
}
