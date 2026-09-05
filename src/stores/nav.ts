import { create } from "zustand";

export type View = "home" | "control" | "mode" | "history" | "users" | "settings" | "help" | "about";

interface NavState {
  view: View;
  setView: (v: View) => void;
}

export const useNav = create<NavState>((set) => ({
  view: "control",
  setView: (view) => set({ view }),
}));
