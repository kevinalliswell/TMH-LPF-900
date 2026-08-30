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
  setSnapshot: (s: TelemetrySnapshot) => void;
}

export const useTelemetry = create<TelemetryState>((set) => ({
  snapshot: null,
  lastAt: null,
  setSnapshot: (s) => set({ snapshot: s, lastAt: Date.now() }),
}));

/** Telemetry is stale when no frame has arrived within this window (docs/06 §6.3, docs/07). */
export const STALE_MS = 5000;

/** Reactive staleness: true when the stream has frozen (>5s since the last frame), so tiles
 *  and status dots can grey out instead of showing frozen values as live. */
export function useTelemetryStale(): boolean {
  const lastAt = useTelemetry((s) => s.lastAt);
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, []);
  return lastAt == null || now - lastAt > STALE_MS;
}
