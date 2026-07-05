import { create } from "zustand";

export type ToastKind = "info" | "warn" | "error";
export interface Toast {
  id: number;
  msg: string;
  kind: ToastKind;
}

interface Store {
  toasts: Toast[];
  push: (msg: string, kind?: ToastKind) => void;
  dismiss: (id: number) => void;
}

let nextId = 1;

export const useToasts = create<Store>((set) => ({
  toasts: [],
  push: (msg, kind = "info") => {
    const id = nextId++;
    set((s) => ({ toasts: [...s.toasts, { id, msg, kind }] }));
    window.setTimeout(() => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })), 3600);
  },
  dismiss: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}));

/** Map a command error to a Chinese toast (docs/07 §global). */
export function toastError(e: unknown): void {
  const raw = e instanceof Error ? e.message : String(e);
  const map: Record<string, string> = {
    PORT_IN_USE: "串口被其他程序占用,请关闭旧版软件后重试",
    DEVICE_OFFLINE: "设备离线,请检查连接",
    EXPERIMENT_RUNNING: "实验运行中,禁止此操作",
    AUTH_FAILED: "口令错误",
    INVALID_PARAMS: "参数校验失败",
  };
  useToasts.getState().push(map[raw] ?? raw, "error");
}
