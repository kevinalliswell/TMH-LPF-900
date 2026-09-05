import { create } from "zustand";
import { getAuthState } from "../ipc";
import type { AuthSnapshot } from "../ipc/types";

interface AuthStore {
  snapshot: AuthSnapshot | null;
  loading: boolean;
  refresh: () => Promise<void>;
  setSnapshot: (snapshot: AuthSnapshot) => void;
}

export const useAuth = create<AuthStore>((set) => ({
  snapshot: null,
  loading: true,
  refresh: async () => {
    set({ loading: true });
    try {
      set({ snapshot: await getAuthState() });
    } finally {
      set({ loading: false });
    }
  },
  setSnapshot: (snapshot) => set({ snapshot, loading: false }),
}));
