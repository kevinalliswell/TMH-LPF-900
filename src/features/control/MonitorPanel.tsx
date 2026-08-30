import { GAS_NAMES } from "../../ipc";
import { useTelemetry, useTelemetryStale } from "../../stores/telemetry";

const TEMP_LABELS = ["T1", "T2", "T3", "T4", "T5", "T6", "T7", "T8", "T9"];

export function MonitorPanel() {
  const snap = useTelemetry((s) => s.snapshot);
  const h = snap?.health;
  // A stale stream (>5s) dims every panel too — frozen values must not read as live (#10).
  const stale = useTelemetryStale();
  const tempOff = stale || (h ? !h.temp_connected : false);
  const mfcOff = stale || (h ? !h.mfc_connected : false);
  const balOff = stale || (h ? !h.balance_connected : false);
  const offTag = (off: boolean) => (off ? (stale ? " · 数据陈旧" : " · 离线") : "");

  return (
    <div className="flex flex-col gap-3">
      <section className="panel p-3" style={tempOff ? { opacity: 0.4 } : undefined}>
        <div className="section-title mb-2">温度 · °C{offTag(tempOff)}</div>
        <div className="grid grid-cols-3 gap-2">
          {TEMP_LABELS.map((label, i) => {
            const sample = i === 7;
            const v = snap?.temps[i] ?? null;
            return (
              <div
                key={label}
                className="tile px-3 py-2"
                style={
                  sample
                    ? {
                        borderColor: "var(--heat)",
                        boxShadow: "inset 0 0 0 1px rgba(255,140,66,0.12)",
                      }
                    : undefined
                }
              >
                <div
                  className="text-[11px]"
                  style={{ color: sample ? "var(--heat)" : "var(--fg-faint)" }}
                >
                  {label}
                  {sample && " · 样品"}
                </div>
                <div
                  className="mono text-2xl leading-tight"
                  style={{
                    color: v == null ? "var(--fg-faint)" : sample ? "var(--heat)" : "var(--fg)",
                  }}
                >
                  {v == null ? "—" : v.toFixed(1)}
                </div>
              </div>
            );
          })}
        </div>
      </section>

      <div className="grid gap-3" style={{ gridTemplateColumns: "190px 1fr" }}>
        <section className="panel p-3" style={balOff ? { opacity: 0.4 } : undefined}>
          <div className="section-title mb-2">重量 · g{offTag(balOff)}</div>
          <div
            className="mono text-3xl"
            style={{ color: snap?.weight == null ? "var(--fg-faint)" : "var(--fg)" }}
          >
            {snap?.weight == null ? "—" : snap.weight.toFixed(3)}
          </div>
        </section>

        <section className="panel p-3" style={mfcOff ? { opacity: 0.4 } : undefined}>
          <div className="section-title mb-2">气体流量 · L/min（PV 主 / SV 辅）{offTag(mfcOff)}</div>
          <div className="grid grid-cols-4 gap-2">
            {GAS_NAMES.map((name, i) => {
              const g = snap?.gas[i];
              return (
                <div key={name} className="tile px-2 py-2">
                  <div className="flex items-center justify-between">
                    <span className="text-[12px] font-medium" style={{ color: "var(--gas)" }}>
                      {name}
                    </span>
                    <span
                      className="text-[10px] px-1 rounded"
                      style={{
                        background: g?.on ? "rgba(56,214,196,0.15)" : "var(--panel-2)",
                        color: g?.on ? "var(--gas)" : "var(--fg-faint)",
                      }}
                    >
                      {g?.on ? "ON" : "OFF"}
                    </span>
                  </div>
                  <div
                    className="mono text-xl leading-tight mt-0.5"
                    style={{ color: g == null ? "var(--fg-faint)" : "var(--fg)" }}
                  >
                    {g == null ? "—" : g.pv.toFixed(2)}
                  </div>
                  <div className="mono text-[11px]" style={{ color: "var(--fg-faint)" }}>
                    SV {g == null ? "—" : g.sv.toFixed(1)}
                  </div>
                </div>
              );
            })}
          </div>
        </section>
      </div>
    </div>
  );
}
