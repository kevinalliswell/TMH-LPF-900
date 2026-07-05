import { create } from "zustand";
import type { ExperimentState } from "../ipc/types";

interface Store {
  state: ExperimentState;
  setState: (s: ExperimentState) => void;
}

export const useExperiment = create<Store>((set) => ({
  state: { running: false },
  setState: (s) => set({ state: s }),
}));
