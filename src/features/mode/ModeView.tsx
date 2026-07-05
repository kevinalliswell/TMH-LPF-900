import { useEffect, useState } from "react";
import { listPrograms } from "../../ipc";
import type { ProgramDto, StageKind } from "../../ipc/types";
import { toastError } from "../../stores/toast";
import { Page } from "../../components/Page";

const GAS_NAMES = ["H₂", "N₂", "CO₂", "CO"];
const KIND_LABEL: Record<StageKind, string> = {
  HEATING: "升温",
  STABILIZING: "恒温",
  REDUCING: "还原",
  COOLING: "冷却",
};
const KIND_COLOR: Record<StageKind, string> = {
  HEATING: "var(--heat)",
  STABILIZING: "var(--warn)",
  REDUCING: "var(--gas)",
  COOLING: "var(--ok)",
};

function gasSummary(gas: number[]): string {
  const parts = gas
    .map((v, i) => (v > 0 ? `${GAS_NAMES[i]} ${v}` : null))
    .filter(Boolean);
  return parts.length ? parts.join(" · ") : "—";
}

function ProgramCard({ program }: { program: ProgramDto }) {
  return (
    <div className="panel p-4">
      <div className="flex items-baseline justify-between mb-3">
        <div className="text-[15px] font-semibold">{program.name}</div>
        <div className="mono text-[11px]" style={{ color: "var(--fg-faint)" }}>
          {program.id} · {program.stages.length} 阶段
        </div>
      </div>
      <div className="overflow-x-auto">
        <table className="w-full text-[12px] border-collapse">
          <thead>
            <tr style={{ color: "var(--fg-faint)" }} className="text-left">
              <th className="py-1.5 pr-3 font-medium">阶段</th>
              <th className="py-1.5 pr-3 font-medium">类型</th>
              <th className="py-1.5 pr-3 font-medium text-right">目标温度</th>
              <th className="py-1.5 pr-3 font-medium text-right">偏差</th>
              <th className="py-1.5 pr-3 font-medium text-right">保温</th>
              <th className="py-1.5 pr-3 font-medium">气氛 (L/min)</th>
              <th className="py-1.5 font-medium">说明</th>
            </tr>
          </thead>
          <tbody>
            {program.stages.map((s, i) => (
              <tr key={s.name} style={{ borderTop: "1px solid var(--border)" }}>
                <td className="py-1.5 pr-3">
                  <span className="mono" style={{ color: "var(--fg-faint)" }}>
                    {i + 1}
                  </span>{" "}
                  {s.name}
                </td>
                <td className="py-1.5 pr-3">
                  <span
                    className="px-1.5 py-0.5 rounded text-[11px]"
                    style={{ background: "var(--panel-2)", color: KIND_COLOR[s.kind] }}
                  >
                    {KIND_LABEL[s.kind]}
                  </span>
                </td>
                <td className="py-1.5 pr-3 mono text-right">{s.target_temp}℃</td>
                <td className="py-1.5 pr-3 mono text-right" style={{ color: "var(--fg-faint)" }}>
                  ±{s.tolerance}
                </td>
                <td className="py-1.5 pr-3 mono text-right">
                  {s.duration_min > 0 ? `${s.duration_min} min` : "动态"}
                </td>
                <td className="py-1.5 pr-3 mono" style={{ color: "var(--gas)" }}>
                  {gasSummary(s.gas)}
                </td>
                <td className="py-1.5" style={{ color: "var(--fg-dim)" }}>
                  {s.description}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

export function ModeView() {
  const [programs, setPrograms] = useState<ProgramDto[]>([]);
  useEffect(() => {
    listPrograms().then(setPrograms).catch(toastError);
  }, []);

  return (
    <Page wide>
      <div className="text-[13px]" style={{ color: "var(--fg-dim)" }}>
        国标实验程序（只读）。安全气氛限值在 MFC 总线执行，前端仅展示。
      </div>
      {programs.map((p) => (
        <ProgramCard key={p.id} program={p} />
      ))}
    </Page>
  );
}
