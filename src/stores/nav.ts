import { create } from "zustand";

export type View = "home" | "control" | "mode" | "groups" | "history" | "users" | "settings" | "help" | "about";
interface NavState {
  view: View;
  leaveBlockReason: string | null;
  setLeaveBlockReason: (reason: string | null) => void;
  setView: (view: View) => void;
}
export const useNav = create<NavState>((set, get) => ({
  view: "control",
  leaveBlockReason: null,
  setLeaveBlockReason: (leaveBlockReason) => set({ leaveBlockReason }),
  setView: (view) => { if (!get().leaveBlockReason || get().view === view) set({ view }); },
}));
