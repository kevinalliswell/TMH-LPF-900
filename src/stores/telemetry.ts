// Telemetry store (docs/07 §perf): the Channel writes straight here at 5 Hz. Tiles
// read via the hook; the chart subscribes imperatively (store.subscribe → ECharts)
// so high-frequency updates never go through React render state.

import { create } from "zustand";
import type { TelemetrySnapshot } from "../ipc/types";

interface TelemetryState {
  snapshot: TelemetrySnapshot | null;
  setSnapshot: (s: TelemetrySnapshot) => void;
}

export const useTelemetry = create<TelemetryState>((set) => ({
  snapshot: null,
  setSnapshot: (s) => set({ snapshot: s }),
}));
