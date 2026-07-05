import { useToasts } from "../stores/toast";

export function Toasts() {
  const toasts = useToasts((s) => s.toasts);
  const dismiss = useToasts((s) => s.dismiss);
  return (
    <div className="fixed bottom-12 right-4 z-50 flex flex-col gap-2 items-end">
      {toasts.map((t) => (
        <div
          key={t.id}
          onClick={() => dismiss(t.id)}
          className="panel px-3 py-2 text-[13px] max-w-[360px] cursor-default"
          style={{
            borderColor:
              t.kind === "error"
                ? "var(--err)"
                : t.kind === "warn"
                  ? "var(--warn)"
                  : "var(--border-bright)",
          }}
        >
          {t.msg}
        </div>
      ))}
    </div>
  );
}
