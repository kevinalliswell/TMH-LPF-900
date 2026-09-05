import { useTelemetry, useTelemetryStale } from "../../stores/telemetry";
import { useRuntimeMode } from "../../stores/runtimeMode";

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
  const stale = useTelemetryStale();
  // A stale stream (>5s without a frame) shows amber/stale dots regardless of last-known
  // health, so frozen data is never presented as live (issue #10).
  const h = stale ? undefined : snap?.health;
  const mode = useRuntimeMode();
  return (
    <footer
      className="flex items-center gap-5 px-4 h-9 border-t text-[12px]"
      style={{ borderColor: "var(--border)", background: "var(--panel)" }}
    >
      <Dot ok={h?.temp_valid} label="温度总线" />
      <Dot ok={h?.mfc_valid} label="气体总线" />
      <Dot ok={h?.balance_valid} label="天平" />
      {stale && snap && (
        <span className="text-[11px]" style={{ color: "var(--warn)" }}>
          数据陈旧（&gt;5s）
        </span>
      )}
      <div className="flex-1" />
      {mode !== null && mode !== "real" && (
        // Simulation watermark only in non-real modes (docs/07, docs/08 §8.1); real
        // hardware shows no watermark so it is never mislabelled as a simulation (#13).
        <span
          className="px-2 py-0.5 rounded text-[11px]"
          style={{
            background: "rgba(214,164,25,0.14)",
            color: "var(--warn)",
            border: "1px solid rgba(214,164,25,0.32)",
          }}
        >
          {mode === "web" ? "浏览器模拟数据" : mode === "replay" ? "回放模式" : "模拟模式"}
        </span>
      )}
      <span className="mono" style={{ color: "var(--fg-faint)" }}>
        seq {snap?.seq ?? 0}
      </span>
    </footer>
  );
}
