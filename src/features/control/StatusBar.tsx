import { runtimeMode } from "../../ipc";
import { useTelemetry } from "../../stores/telemetry";

function Dot({ ok, label }: { ok: boolean | undefined; label: string }) {
  // undefined = never read (amber/stale), true = green, false = red
  const color = ok === undefined ? "var(--warn)" : ok ? "var(--ok)" : "var(--err)";
  return (
    <span className="flex items-center gap-1.5 text-[12px]">
      <span
        style={{
          width: 8,
          height: 8,
          borderRadius: "50%",
          background: color,
          boxShadow: `0 0 7px ${color}`,
        }}
      />
      <span style={{ color: "var(--fg-dim)" }}>{label}</span>
    </span>
  );
}

export function StatusBar() {
  const snap = useTelemetry((s) => s.snapshot);
  const h = snap?.health;
  const mode = runtimeMode();
  return (
    <footer
      className="flex items-center gap-5 px-4 h-9 border-t text-[12px]"
      style={{ borderColor: "var(--border)", background: "var(--panel)" }}
    >
      <Dot ok={h?.temp_connected} label="温度总线" />
      <Dot ok={h?.mfc_connected} label="气体总线" />
      <Dot ok={h?.balance_connected} label="天平" />
      <div className="flex-1" />
      <span
        className="px-2 py-0.5 rounded text-[11px]"
        style={{
          background: "rgba(214,164,25,0.14)",
          color: "var(--warn)",
          border: "1px solid rgba(214,164,25,0.32)",
        }}
      >
        {mode === "web" ? "浏览器模拟数据" : "模拟模式"}
      </span>
      <span className="mono" style={{ color: "var(--fg-faint)" }}>
        seq {snap?.seq ?? 0}
      </span>
    </footer>
  );
}
