import { EXPERIMENT_MODES } from "../../ipc";
import { useExperiment } from "../../stores/experiment";
import { useNav } from "../../stores/nav";
import { useTelemetry } from "../../stores/telemetry";
import { Page, SectionTitle } from "../../components/Page";

function Dot({ ok, label }: { ok: boolean; label: string }) {
  return (
    <div className="flex items-center gap-1.5 text-[12px]">
      <span
        style={{
          width: 8,
          height: 8,
          borderRadius: "50%",
          background: ok ? "var(--ok)" : "var(--err)",
          boxShadow: ok ? "0 0 6px var(--ok)" : "none",
        }}
      />
      <span style={{ color: "var(--fg-dim)" }}>{label}</span>
    </div>
  );
}

export function HomeView() {
  const snap = useTelemetry((s) => s.snapshot);
  const ex = useExperiment((s) => s.state);
  const setView = useNav((s) => s.setView);
  const t8 = snap?.temps[7] ?? null;
  const h = snap?.health;
  const modeName = EXPERIMENT_MODES.find((m) => m.id === ex.mode)?.name ?? "—";

  return (
    <Page wide>
      <div className="panel p-6">
        <div className="text-xl font-bold tracking-tight">铁矿石冶金性能检测系统</div>
        <div className="text-[13px] mt-1" style={{ color: "var(--fg-dim)" }}>
          TMH-LPF-900 · 依据 GB/T 13240 / 13241 / 13242 自动化检测与控制
        </div>
      </div>

      <div className="grid grid-cols-3 gap-4">
        <div className="panel p-4">
          <SectionTitle>样品温度 T8</SectionTitle>
          <div className="mono text-[28px]" style={{ color: "var(--heat)" }}>
            {t8 == null ? "—" : t8.toFixed(1)}
            <span className="text-[14px]"> ℃</span>
          </div>
        </div>
        <div className="panel p-4">
          <SectionTitle>天平读数</SectionTitle>
          <div className="mono text-[28px]" style={{ color: "var(--gas)" }}>
            {snap?.weight == null ? "—" : snap.weight.toFixed(2)}
            <span className="text-[14px]"> g</span>
          </div>
        </div>
        <div className="panel p-4">
          <SectionTitle>实验状态</SectionTitle>
          {ex.running ? (
            <>
              <div className="text-[16px]" style={{ color: "var(--ok)" }}>
                运行中
              </div>
              <div className="text-[12px] mt-1" style={{ color: "var(--fg-faint)" }}>
                {modeName} · {ex.stage_name ?? ""}
              </div>
            </>
          ) : (
            <div className="text-[16px]" style={{ color: "var(--fg-faint)" }}>
              空闲
            </div>
          )}
        </div>
      </div>

      <div className="panel p-4">
        <SectionTitle>设备连接</SectionTitle>
        <div className="flex gap-6">
          <Dot ok={!!h?.temp_valid} label="温度控制器" />
          <Dot ok={!!h?.mfc_valid} label="气体流量 MFC" />
          <Dot ok={!!h?.balance_valid} label="电子天平" />
        </div>
      </div>

      <div className="flex gap-3">
        {[
          ["control", "进入综合控制"],
          ["history", "查看历史记录"],
          ["mode", "实验模式说明"],
        ].map(([v, label]) => (
          <button
            key={v}
            onClick={() => setView(v as "control" | "history" | "mode")}
            className="px-4 py-2 rounded text-[13px]"
            style={{ background: "var(--panel-2)", border: "1px solid var(--border-bright)", color: "var(--fg)" }}
          >
            {label}
          </button>
        ))}
      </div>
    </Page>
  );
}
