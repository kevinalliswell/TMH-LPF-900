// Telemetry store (docs/07 §perf): the Channel writes straight here at 5 Hz. Tiles
// read via the hook; the chart subscribes imperatively (store.subscribe → ECharts)
// so high-frequency updates never go through React render state.

import { create } from "zustand";
import { useEffect, useState } from "react";
import type { TelemetrySnapshot } from "../ipc/types";

interface TelemetryState {
  snapshot: TelemetrySnapshot | null;
  /** Wall-clock ms when the last frame arrived (for staleness detection). */
  lastAt: number | null;
  /** Staleness window, hydrated from the backend's `heartbeat_timeout` (#44). */
  staleMs: number;
  setSnapshot: (s: TelemetrySnapshot) => void;
  setStaleMs: (ms: number) => void;
}

/** Fallback staleness window (docs/06 §6.3) used until the backend reports its config. */
export const DEFAULT_STALE_MS = 5000;

export const useTelemetry = create<TelemetryState>((set) => ({
  snapshot: null,
  lastAt: null,
  staleMs: DEFAULT_STALE_MS,
  setSnapshot: (s) => set({ snapshot: s, lastAt: Date.now() }),
  // Ignore nonsense from a corrupt config: a zero window would grey the whole UI forever.
  setStaleMs: (ms) => set({ staleMs: Number.isFinite(ms) && ms > 0 ? ms : DEFAULT_STALE_MS }),
}));

/** Reactive staleness: true when the stream has frozen, so tiles and status dots can grey
 *  out instead of showing frozen values as live. */
export function useTelemetryStale(): boolean {
  const lastAt = useTelemetry((s) => s.lastAt);
  const staleMs = useTelemetry((s) => s.staleMs);
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, []);
  return lastAt == null || now - lastAt > staleMs;
}
